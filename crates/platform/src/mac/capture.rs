//! Input capture through a session-level CGEventTap on a dedicated run loop.
//!
//! While grabbing, the tap drops every event and the cursor is detached from
//! the mouse (`CGAssociateMouseAndMouseCursorPosition(false)`) and hidden, so
//! it stays put and out of sight while the tap still reports motion deltas.
//! Ctrl+Option+Shift+Esc always ends a grab.

use std::cell::Cell;
use std::ffi::c_void;
use std::io;
use std::ptr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;

use super::INJECT_MARKER;
use super::ffi::*;
use crate::{CaptureEvent, InputEvent, MouseButton};

pub struct Capture {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

struct Shared {
    grab: AtomicBool,
    stop: AtomicBool,
}

impl Capture {
    pub fn start() -> io::Result<(Capture, Receiver<CaptureEvent>)> {
        let (tx, rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        let shared =
            Arc::new(Shared { grab: AtomicBool::new(false), stop: AtomicBool::new(false) });
        let thread_shared = shared.clone();
        allow_background_cursor_control();

        let thread = std::thread::Builder::new()
            .name("sc-capture".into())
            .spawn(move || run_tap(tx, thread_shared, ready_tx))?;

        ready_rx.recv().map_err(|_| io::Error::other("capture thread exited during setup"))??;
        Ok((Capture { shared, thread: Some(thread) }, rx))
    }

    pub fn set_grab(&self, on: bool) {
        if self.shared.grab.swap(on, Ordering::SeqCst) != on {
            set_cursor_detached(on);
        }
    }

    pub fn is_grabbing(&self) -> bool {
        self.shared.grab.load(Ordering::SeqCst)
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.set_grab(false);
        self.shared.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Detaches and hides the cursor, or brings it back. Call only on changes:
/// macOS counts hide calls.
fn set_cursor_detached(detached: bool) {
    unsafe {
        CGAssociateMouseAndMouseCursorPosition(u32::from(!detached));
        let display = CGMainDisplayID();
        if detached {
            CGDisplayHideCursor(display);
        } else {
            CGDisplayShowCursor(display);
        }
    }
}

/// Without this, macOS only lets the frontmost app hide the cursor.
fn allow_background_cursor_control() {
    unsafe {
        let key = CFStringCreateWithCString(
            ptr::null(),
            c"SetsCursorInBackground".as_ptr(),
            kCFStringEncodingUTF8,
        );
        if key.is_null() {
            return;
        }
        let connection = _CGSDefaultConnection();
        CGSSetConnectionProperty(connection, connection, key, kCFBooleanTrue);
        CFRelease(key);
    }
}

/// State owned by the tap thread; the callback reaches it through `user_info`.
struct TapState {
    tx: Sender<CaptureEvent>,
    shared: Arc<Shared>,
    port: AtomicPtr<c_void>,
    /// Scroll amounts below one HID unit, carried over (in 1/65536 units).
    scroll_rest: Cell<(i64, i64)>,
}

fn run_tap(tx: Sender<CaptureEvent>, shared: Arc<Shared>, ready: Sender<io::Result<()>>) {
    let state = Box::into_raw(Box::new(TapState {
        tx,
        shared: shared.clone(),
        port: AtomicPtr::new(ptr::null_mut()),
        scroll_rest: Cell::new((0, 0)),
    }));

    let types = [
        kCGEventLeftMouseDown,
        kCGEventLeftMouseUp,
        kCGEventRightMouseDown,
        kCGEventRightMouseUp,
        kCGEventMouseMoved,
        kCGEventLeftMouseDragged,
        kCGEventRightMouseDragged,
        kCGEventKeyDown,
        kCGEventKeyUp,
        kCGEventFlagsChanged,
        kCGEventScrollWheel,
        kCGEventOtherMouseDown,
        kCGEventOtherMouseUp,
        kCGEventOtherMouseDragged,
    ];
    let mask = types.iter().fold(0u64, |m, &t| m | (1 << t));

    let port = unsafe {
        CGEventTapCreate(
            kCGSessionEventTap,
            kCGHeadInsertEventTap,
            kCGEventTapOptionDefault,
            mask,
            tap_callback,
            state.cast(),
        )
    };
    if port.is_null() {
        drop(unsafe { Box::from_raw(state) });
        let _ = ready.send(Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "cannot create event tap: grant Accessibility and Input Monitoring \
             in System Settings > Privacy & Security, then restart",
        )));
        return;
    }
    unsafe { (*state).port.store(port, Ordering::SeqCst) };

    unsafe {
        let source = CFMachPortCreateRunLoopSource(ptr::null(), port, 0);
        CFRunLoopAddSource(CFRunLoopGetCurrent(), source, kCFRunLoopCommonModes);
        CGEventTapEnable(port, true);
        let _ = ready.send(Ok(()));

        // Short slices so Drop can stop us without racing CFRunLoopStop.
        while !shared.stop.load(Ordering::SeqCst) {
            CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.25, 0);
        }

        CGEventTapEnable(port, false);
        CFMachPortInvalidate(port);
        CFRelease(source.cast_const());
        CFRelease(port.cast_const());
        drop(Box::from_raw(state));
    }
}

unsafe extern "C" fn tap_callback(
    _proxy: CGEventTapProxy,
    ty: CGEventType,
    event: CGEventRef,
    user_info: *mut c_void,
) -> CGEventRef {
    let state = unsafe { &*(user_info as *const TapState) };

    // macOS disables slow taps; turn ourselves back on.
    if ty == kCGEventTapDisabledByTimeout || ty == kCGEventTapDisabledByUserInput {
        let port = state.port.load(Ordering::SeqCst);
        if !port.is_null() {
            unsafe { CGEventTapEnable(port, true) };
        }
        return event;
    }
    if unsafe { CGEventGetIntegerValueField(event, kCGEventSourceUserData) } == INJECT_MARKER {
        return event;
    }

    let grabbing = state.shared.grab.load(Ordering::SeqCst);
    if grabbing && ty == kCGEventKeyDown && is_emergency(event) {
        // swap: only the winner of a race with set_grab(false) shows the cursor.
        if state.shared.grab.swap(false, Ordering::SeqCst) {
            set_cursor_detached(false);
        }
        let _ = state.tx.send(CaptureEvent::EmergencyRelease);
        return ptr::null_mut();
    }

    let emit = |ev| {
        let _ = state.tx.send(CaptureEvent::Input(ev));
    };
    unsafe { translate(state, ty, event, grabbing, emit) };

    if grabbing { ptr::null_mut() } else { event }
}

fn is_emergency(event: CGEventRef) -> bool {
    let keycode = unsafe { CGEventGetIntegerValueField(event, kCGKeyboardEventKeycode) };
    let flags = unsafe { CGEventGetFlags(event) };
    let wanted = kCGEventFlagMaskControl | kCGEventFlagMaskAlternate | kCGEventFlagMaskShift;
    keycode == 0x35 && flags & wanted == wanted
}

unsafe fn translate(
    state: &TapState,
    ty: CGEventType,
    event: CGEventRef,
    grabbing: bool,
    mut emit: impl FnMut(InputEvent),
) {
    let field = |f| unsafe { CGEventGetIntegerValueField(event, f) };
    let button = |button, down| InputEvent::Button { button, down };
    match ty {
        kCGEventMouseMoved
        | kCGEventLeftMouseDragged
        | kCGEventRightMouseDragged
        | kCGEventOtherMouseDragged => {
            if grabbing {
                let (dx, dy) =
                    (field(kCGMouseEventDeltaX) as i32, field(kCGMouseEventDeltaY) as i32);
                if dx != 0 || dy != 0 {
                    emit(InputEvent::MouseDelta { dx, dy });
                }
            } else {
                let p = unsafe { CGEventGetLocation(event) };
                emit(InputEvent::MouseMove { x: p.x.floor() as i32, y: p.y.floor() as i32 });
            }
        }
        kCGEventLeftMouseDown => emit(button(MouseButton::Left, true)),
        kCGEventLeftMouseUp => emit(button(MouseButton::Left, false)),
        kCGEventRightMouseDown => emit(button(MouseButton::Right, true)),
        kCGEventRightMouseUp => emit(button(MouseButton::Right, false)),
        kCGEventOtherMouseDown | kCGEventOtherMouseUp => {
            let which = match field(kCGMouseEventButtonNumber) {
                2 => MouseButton::Middle,
                3 => MouseButton::Back,
                4 => MouseButton::Forward,
                _ => return,
            };
            emit(button(which, ty == kCGEventOtherMouseDown));
        }
        kCGEventScrollWheel => {
            // Fixed-point (16.16) line deltas also cover fractional trackpad scrolls.
            // Axis 1 is vertical (positive = away from the user). Axis 2 is
            // horizontal with positive = left, the opposite of the wire format.
            let (mut rest_y, mut rest_x) = state.scroll_rest.get();
            rest_y += field(kCGScrollWheelEventFixedPtDeltaAxis1) * 120;
            rest_x -= field(kCGScrollWheelEventFixedPtDeltaAxis2) * 120;
            let (dy, dx) = (rest_y / 65536, rest_x / 65536);
            state.scroll_rest.set((rest_y - dy * 65536, rest_x - dx * 65536));
            if dx != 0 || dy != 0 {
                emit(InputEvent::Wheel { dx: dx as i32, dy: dy as i32 });
            }
        }
        kCGEventKeyDown | kCGEventKeyUp => {
            if let Some(usage) = sc_keymap::hid_from_mac(field(kCGKeyboardEventKeycode) as u16) {
                emit(InputEvent::Key { usage, down: ty == kCGEventKeyDown });
            }
        }
        kCGEventFlagsChanged => {
            let keycode = field(kCGKeyboardEventKeycode) as u16;
            let Some(usage) = sc_keymap::hid_from_mac(keycode) else { return };
            let flags = unsafe { CGEventGetFlags(event) };
            if usage == sc_keymap::usage::CAPS_LOCK {
                // Caps Lock reports a toggle, not press and release.
                emit(InputEvent::Key { usage, down: true });
                emit(InputEvent::Key { usage, down: false });
            } else if let Some(mask) = device_mask(usage) {
                emit(InputEvent::Key { usage, down: flags & mask != 0 });
            }
        }
        _ => {}
    }
}

/// Device-dependent flag bit for a modifier usage.
pub(super) fn device_mask(usage: u16) -> Option<CGEventFlags> {
    use sc_keymap::usage::*;
    Some(match usage {
        LEFT_CTRL => NX_DEVICELCTLKEYMASK,
        LEFT_SHIFT => NX_DEVICELSHIFTKEYMASK,
        LEFT_ALT => NX_DEVICELALTKEYMASK,
        LEFT_GUI => NX_DEVICELCMDKEYMASK,
        RIGHT_CTRL => NX_DEVICERCTLKEYMASK,
        RIGHT_SHIFT => NX_DEVICERSHIFTKEYMASK,
        RIGHT_ALT => NX_DEVICERALTKEYMASK,
        RIGHT_GUI => NX_DEVICERCMDKEYMASK,
        _ => return None,
    })
}
