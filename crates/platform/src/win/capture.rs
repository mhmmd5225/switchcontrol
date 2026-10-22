//! Input capture.
//!
//! A dedicated thread installs `WH_MOUSE_LL` and `WH_KEYBOARD_LL` hooks. Hook
//! callbacks must return quickly (Windows silently removes hooks that exceed
//! `LowLevelHooksTimeout`), so they only push onto a channel.
//!
//! While grabbing, hooks swallow every event so the local OS never sees it.
//! The cursor is parked at the centre of the primary monitor and each mouse
//! move is reported as a `MouseDelta` from the parking spot, which keeps the
//! user's pointer speed and acceleration. A nearly invisible window with an
//! empty cursor sits under the parking spot, so the parked cursor is hidden.
//! Ctrl+Alt+Shift+Esc always ends a grab.

use std::cell::{Cell, RefCell};
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;

use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    VK_ESCAPE, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_RCONTROL, VK_RMENU, VK_RSHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateCursor, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    GetMessageW, HC_ACTION, HHOOK, HWND_TOPMOST, KBDLLHOOKSTRUCT, LLKHF_EXTENDED, LLKHF_UP,
    LWA_ALPHA, MSG, MSLLHOOKSTRUCT, PostThreadMessageW, RegisterClassW, SW_HIDE, SWP_NOACTIVATE,
    SWP_SHOWWINDOW, SetLayeredWindowAttributes, SetWindowPos, SetWindowsHookExW, ShowWindow,
    TranslateMessage, UnhookWindowsHookEx, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_APP, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL,
    WM_QUIT, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_XBUTTONDOWN, WM_XBUTTONUP, WNDCLASSW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{Result, w};

use super::{INJECT_MARKER, Injector, monitors, virtual_desktop};
use crate::{CaptureEvent, InputEvent, MouseButton};

/// Running capture. Dropping it removes the hooks and stops the thread.
pub struct Capture {
    shared: Arc<Shared>,
    thread_id: u32,
    thread: Option<JoinHandle<()>>,
}

/// State shared between the owner and the hook thread.
struct Shared {
    grab: AtomicBool,
    /// Set once the hook has seen our parking move, so deltas can be trusted.
    parked: AtomicBool,
    park_x: AtomicI32,
    park_y: AtomicI32,
}

impl Capture {
    pub fn start() -> io::Result<(Capture, Receiver<CaptureEvent>)> {
        let (tx, rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        let shared = Arc::new(Shared {
            grab: AtomicBool::new(false),
            parked: AtomicBool::new(false),
            park_x: AtomicI32::new(0),
            park_y: AtomicI32::new(0),
        });
        let thread_shared = shared.clone();

        let thread = std::thread::Builder::new().name("sc-capture".into()).spawn(move || {
            match Hooks::install(tx, thread_shared) {
                Ok(hooks) => {
                    let _ = ready_tx.send(Ok(unsafe { GetCurrentThreadId() }));
                    hooks.run_message_loop();
                    drop(hooks);
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                }
            }
        })?;

        let thread_id = ready_rx
            .recv()
            .map_err(|_| io::Error::other("capture thread exited during setup"))?
            .map_err(io::Error::other)?;
        Ok((Capture { shared, thread_id, thread: Some(thread) }, rx))
    }

    /// While grabbing, local input is swallowed instead of reaching the OS and
    /// mouse motion arrives as `MouseDelta`.
    pub fn set_grab(&self, on: bool) {
        if on == self.is_grabbing() {
            return;
        }
        if on {
            self.shared.parked.store(false, Ordering::SeqCst);
        }
        self.shared.grab.store(on, Ordering::SeqCst);
        // Parking and the cover window are handled on the hook thread.
        let _ = unsafe {
            PostThreadMessageW(self.thread_id, WM_SC_GRAB, WPARAM(usize::from(on)), LPARAM(0))
        };
    }

    pub fn is_grabbing(&self) -> bool {
        self.shared.grab.load(Ordering::SeqCst)
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.shared.grab.store(false, Ordering::SeqCst);
        let _ = unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) };
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Centre of the primary monitor: far from every edge, so deltas never clip.
fn park_point() -> (i32, i32) {
    let area = monitors()
        .into_iter()
        .find(|m| m.primary)
        .map(|m| m.bounds)
        .unwrap_or_else(virtual_desktop);
    (area.left + area.width() / 2, area.top + area.height() / 2)
}

/// Thread message to the hook thread: a grab started (wparam 1) or ended (0).
const WM_SC_GRAB: u32 = WM_APP + 1;

/// Side of the square window that hides the parked cursor.
const COVER_SIZE: i32 = 64;

fn begin_grab(cover: HWND) {
    let (x, y) = park_point();
    let half = COVER_SIZE / 2;
    unsafe {
        let _ = SetWindowPos(
            cover,
            Some(HWND_TOPMOST),
            x - half,
            y - half,
            COVER_SIZE,
            COVER_SIZE,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
    }
    // Lands on the cover, which switches to its empty cursor. The hook
    // records where this tagged move actually ends up as the parking spot.
    let _ = Injector::new().inject(&InputEvent::MouseMove { x, y });
}

fn end_grab(cover: HWND) {
    // The cursor reappears with the next move.
    let _ = unsafe { ShowWindow(cover, SW_HIDE) };
}

/// A topmost, almost fully transparent window whose class cursor is empty.
fn create_cover(hinstance: HINSTANCE) -> Result<HWND> {
    // A 32x32 cursor whose AND mask keeps every pixel and XOR mask changes none.
    let and_mask = [0xFFu8; 32 * 32 / 8];
    let xor_mask = [0u8; 32 * 32 / 8];
    let cursor = unsafe {
        CreateCursor(
            Some(hinstance),
            0,
            0,
            32,
            32,
            and_mask.as_ptr().cast(),
            xor_mask.as_ptr().cast(),
        )?
    };
    let class_name = w!("SwitchControlCover");
    let class = WNDCLASSW {
        lpfnWndProc: Some(cover_wndproc),
        hInstance: hinstance,
        hCursor: cursor,
        lpszClassName: class_name,
        ..Default::default()
    };
    // Registration fails if the class already exists, which is fine.
    unsafe { RegisterClassW(&class) };
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED,
            class_name,
            w!(""),
            WS_POPUP,
            0,
            0,
            COVER_SIZE,
            COVER_SIZE,
            None,
            None,
            Some(hinstance),
            None,
        )?
    };
    // Alpha 1 of 255: invisible to the eye, but still under the cursor.
    unsafe { SetLayeredWindowAttributes(hwnd, COLORREF(0), 1, LWA_ALPHA)? };
    Ok(hwnd)
}

unsafe extern "system" fn cover_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// Per-thread state reachable from the hook callbacks, which take no context.
struct HookState {
    tx: Sender<CaptureEvent>,
    shared: Arc<Shared>,
    modifiers: Cell<u8>,
    cover: HWND,
}

const MOD_CTRL: u8 = 1;
const MOD_ALT: u8 = 2;
const MOD_SHIFT: u8 = 4;
const EMERGENCY_MODS: u8 = MOD_CTRL | MOD_ALT | MOD_SHIFT;

thread_local! {
    static STATE: RefCell<Option<HookState>> = const { RefCell::new(None) };
}

struct Hooks {
    mouse: HHOOK,
    keyboard: HHOOK,
    cover: HWND,
}

impl Hooks {
    fn install(tx: Sender<CaptureEvent>, shared: Arc<Shared>) -> Result<Hooks> {
        let hinstance: HINSTANCE = unsafe { GetModuleHandleW(None)? }.into();
        let cover = create_cover(hinstance)?;
        STATE.set(Some(HookState { tx, shared, modifiers: Cell::new(0), cover }));
        let mouse =
            unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), Some(hinstance), 0)? };
        let keyboard =
            unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), Some(hinstance), 0)? };
        Ok(Hooks { mouse, keyboard, cover })
    }

    fn run_message_loop(&self) {
        let mut msg = MSG::default();
        unsafe {
            while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                if msg.hwnd.0.is_null() && msg.message == WM_SC_GRAB {
                    if msg.wParam.0 == 1 {
                        begin_grab(self.cover)
                    } else {
                        end_grab(self.cover)
                    }
                    continue;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}

impl Drop for Hooks {
    fn drop(&mut self) {
        unsafe {
            let _ = UnhookWindowsHookEx(self.mouse);
            let _ = UnhookWindowsHookEx(self.keyboard);
            let _ = DestroyWindow(self.cover);
        }
        STATE.set(None);
    }
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
        if handle_mouse(wparam.0 as u32, info) {
            return LRESULT(1);
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// Returns whether the event should be swallowed.
fn handle_mouse(msg: u32, info: &MSLLHOOKSTRUCT) -> bool {
    STATE.with_borrow(|state| {
        let Some(s) = state else { return false };
        let grabbing = s.shared.grab.load(Ordering::SeqCst);

        if info.dwExtraInfo == INJECT_MARKER {
            if grabbing && msg == WM_MOUSEMOVE && !s.shared.parked.load(Ordering::SeqCst) {
                s.shared.park_x.store(info.pt.x, Ordering::SeqCst);
                s.shared.park_y.store(info.pt.y, Ordering::SeqCst);
                s.shared.parked.store(true, Ordering::SeqCst);
            }
            return false;
        }

        let event = if grabbing && msg == WM_MOUSEMOVE {
            // Moves queued before the cursor reached the parking spot would
            // produce bogus deltas.
            if !s.shared.parked.load(Ordering::SeqCst) {
                return true;
            }
            let dx = info.pt.x - s.shared.park_x.load(Ordering::SeqCst);
            let dy = info.pt.y - s.shared.park_y.load(Ordering::SeqCst);
            (dx != 0 || dy != 0).then_some(InputEvent::MouseDelta { dx, dy })
        } else {
            translate_mouse(msg, info)
        };
        if let Some(ev) = event {
            let _ = s.tx.send(CaptureEvent::Input(ev));
        }
        grabbing
    })
}

fn translate_mouse(msg: u32, info: &MSLLHOOKSTRUCT) -> Option<InputEvent> {
    let high = (info.mouseData >> 16) as u16;
    let button = |button, down| Some(InputEvent::Button { button, down });
    match msg {
        WM_MOUSEMOVE => Some(InputEvent::MouseMove { x: info.pt.x, y: info.pt.y }),
        WM_LBUTTONDOWN => button(MouseButton::Left, true),
        WM_LBUTTONUP => button(MouseButton::Left, false),
        WM_RBUTTONDOWN => button(MouseButton::Right, true),
        WM_RBUTTONUP => button(MouseButton::Right, false),
        WM_MBUTTONDOWN => button(MouseButton::Middle, true),
        WM_MBUTTONUP => button(MouseButton::Middle, false),
        WM_XBUTTONDOWN | WM_XBUTTONUP => {
            let which = if high == 1 { MouseButton::Back } else { MouseButton::Forward };
            button(which, msg == WM_XBUTTONDOWN)
        }
        WM_MOUSEWHEEL => Some(InputEvent::Wheel { dx: 0, dy: high as i16 as i32 }),
        WM_MOUSEHWHEEL => Some(InputEvent::Wheel { dx: high as i16 as i32, dy: 0 }),
        _ => None,
    }
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        if info.dwExtraInfo != INJECT_MARKER && handle_key(info) {
            return LRESULT(1);
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// Returns whether the key event should be swallowed.
fn handle_key(info: &KBDLLHOOKSTRUCT) -> bool {
    STATE.with_borrow(|state| {
        let Some(s) = state else { return false };
        let down = info.flags.0 & LLKHF_UP.0 == 0;

        let bit = modifier_bit(info.vkCode);
        if bit != 0 {
            let mods = s.modifiers.get();
            s.modifiers.set(if down { mods | bit } else { mods & !bit });
        }
        let emergency = down
            && info.vkCode == VK_ESCAPE.0 as u32
            && s.modifiers.get() & EMERGENCY_MODS == EMERGENCY_MODS
            && s.shared.grab.swap(false, Ordering::SeqCst);
        if emergency {
            end_grab(s.cover);
            let _ = s.tx.send(CaptureEvent::EmergencyRelease);
            // Keep the Esc from reaching the app under the cursor.
            return true;
        }

        let mut scan = info.scanCode as u16 & 0xFF;
        if info.flags.0 & LLKHF_EXTENDED.0 != 0 {
            scan |= 0xE000;
        }
        // Keys without a HID usage are not forwarded, but are still
        // swallowed while grabbing.
        if let Some(usage) = sc_keymap::hid_from_win(scan) {
            let _ = s.tx.send(CaptureEvent::Input(InputEvent::Key { usage, down }));
        }
        s.shared.grab.load(Ordering::SeqCst)
    })
}

fn modifier_bit(vk: u32) -> u8 {
    match vk {
        v if v == VK_LCONTROL.0 as u32 || v == VK_RCONTROL.0 as u32 => MOD_CTRL,
        v if v == VK_LMENU.0 as u32 || v == VK_RMENU.0 as u32 => MOD_ALT,
        v if v == VK_LSHIFT.0 as u32 || v == VK_RSHIFT.0 as u32 => MOD_SHIFT,
        _ => 0,
    }
}
