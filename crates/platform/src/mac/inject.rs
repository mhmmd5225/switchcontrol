//! Input injection with `CGEventPost` at the HID level.
//!
//! macOS needs more context than Windows to make synthetic input look real:
//! drags are a separate event type, double clicks carry a click count, and
//! every event carries the current modifier flags. The injector tracks that
//! state itself.

use std::collections::BTreeSet;
use std::io;
use std::ptr;
use std::time::{Duration, Instant};

use sc_keymap::usage;

use super::INJECT_MARKER;
use super::capture::device_mask;
use super::ffi::*;
use crate::{InputEvent, MouseButton};

const DOUBLE_CLICK_TIME: Duration = Duration::from_millis(500);
const DOUBLE_CLICK_DISTANCE: f64 = 4.0;

pub struct Injector {
    buttons: BTreeSet<MouseButton>,
    modifiers: BTreeSet<u16>,
    last_click: Option<(MouseButton, Instant, CGPoint, i64)>,
    /// Trackpad scroll amounts below one point, carried over.
    wheel_rest: (i32, i32),
}

impl Injector {
    pub fn new() -> Self {
        Injector {
            buttons: BTreeSet::new(),
            modifiers: BTreeSet::new(),
            last_click: None,
            wheel_rest: (0, 0),
        }
    }

    pub fn place_cursor(&mut self, x: i32, y: i32) -> io::Result<()> {
        self.wheel_rest = (0, 0);
        let point = CGPoint { x: x as f64, y: y as f64 };
        // A handoff changes the absolute location without a synthetic delta
        // from wherever the Mac cursor was left during the previous session.
        let status = unsafe { CGWarpMouseCursorPosition(point) };
        if status != 0 {
            return self.mouse_move(point);
        }
        let event = unsafe {
            CGEventCreateMouseEvent(ptr::null_mut(), kCGEventMouseMoved, point, kCGMouseButtonLeft)
        };
        self.post(event, |e| unsafe {
            CGEventSetIntegerValueField(e, kCGMouseEventDeltaX, 0);
            CGEventSetIntegerValueField(e, kCGMouseEventDeltaY, 0);
        })
    }

    pub fn inject(&mut self, ev: &InputEvent) -> io::Result<()> {
        match *ev {
            InputEvent::MouseMove { x, y } => self.mouse_move(CGPoint { x: x as f64, y: y as f64 }),
            InputEvent::MouseDelta { dx, dy } => {
                let p = current_location().ok_or_else(|| io::Error::other("no cursor location"))?;
                self.mouse_move(CGPoint { x: p.x + dx as f64, y: p.y + dy as f64 })
            }
            InputEvent::Button { button, down } => self.button(button, down),
            InputEvent::Wheel { dx, dy } => self.wheel(dx, dy),
            InputEvent::Key { usage, down } => self.key(usage, down),
        }
    }

    fn mouse_move(&mut self, to: CGPoint) -> io::Result<()> {
        let from = current_location().unwrap_or(to);
        let (ty, button) = if self.buttons.contains(&MouseButton::Left) {
            (kCGEventLeftMouseDragged, kCGMouseButtonLeft)
        } else if self.buttons.contains(&MouseButton::Right) {
            (kCGEventRightMouseDragged, kCGMouseButtonRight)
        } else if !self.buttons.is_empty() {
            (kCGEventOtherMouseDragged, kCGMouseButtonCenter)
        } else {
            (kCGEventMouseMoved, kCGMouseButtonLeft)
        };
        let event = unsafe { CGEventCreateMouseEvent(ptr::null_mut(), ty, to, button) };
        // Apps that read relative motion (games, 3D tools) look at these.
        self.post(event, |e| unsafe {
            CGEventSetIntegerValueField(e, kCGMouseEventDeltaX, (to.x - from.x) as i64);
            CGEventSetIntegerValueField(e, kCGMouseEventDeltaY, (to.y - from.y) as i64);
        })
    }

    fn button(&mut self, button: MouseButton, down: bool) -> io::Result<()> {
        let at = current_location().unwrap_or_default();
        let (ty, cg_button, number) = match (button, down) {
            (MouseButton::Left, true) => (kCGEventLeftMouseDown, kCGMouseButtonLeft, 0),
            (MouseButton::Left, false) => (kCGEventLeftMouseUp, kCGMouseButtonLeft, 0),
            (MouseButton::Right, true) => (kCGEventRightMouseDown, kCGMouseButtonRight, 1),
            (MouseButton::Right, false) => (kCGEventRightMouseUp, kCGMouseButtonRight, 1),
            (other, down) => {
                let number = match other {
                    MouseButton::Middle => 2,
                    MouseButton::Back => 3,
                    _ => 4,
                };
                let ty = if down { kCGEventOtherMouseDown } else { kCGEventOtherMouseUp };
                (ty, kCGMouseButtonCenter, number)
            }
        };

        let clicks = if down {
            let now = Instant::now();
            let count = match self.last_click {
                Some((b, t, p, n))
                    if b == button
                        && now - t < DOUBLE_CLICK_TIME
                        && (p.x - at.x).hypot(p.y - at.y) <= DOUBLE_CLICK_DISTANCE =>
                {
                    n + 1
                }
                _ => 1,
            };
            self.last_click = Some((button, now, at, count));
            self.buttons.insert(button);
            count
        } else {
            self.buttons.remove(&button);
            self.last_click.map_or(1, |(.., n)| n)
        };

        let event = unsafe { CGEventCreateMouseEvent(ptr::null_mut(), ty, at, cg_button) };
        self.post(event, |e| unsafe {
            CGEventSetIntegerValueField(e, kCGMouseEventButtonNumber, number);
            CGEventSetIntegerValueField(e, kCGMouseEventClickState, clicks);
        })
    }

    fn wheel(&mut self, dx: i32, dy: i32) -> io::Result<()> {
        let (unit, x, y) = if dx % 120 == 0 && dy % 120 == 0 {
            (kCGScrollEventUnitLine, dx / 120, dy / 120)
        } else {
            // Quartz defaults to ten points per line. Keep the remaining
            // sub-point distance rather than waiting for a full wheel step.
            let x = crate::scroll::to_points(dx, &mut self.wheel_rest.0);
            let y = crate::scroll::to_points(dy, &mut self.wheel_rest.1);
            (kCGScrollEventUnitPixel, x, y)
        };
        if x == 0 && y == 0 {
            return Ok(());
        }
        // Wheel 2 is horizontal with positive = left.
        let event = unsafe { CGEventCreateScrollWheelEvent2(ptr::null_mut(), unit, 2, y, -x, 0) };
        self.post(event, |_| {})
    }

    fn key(&mut self, usage: u16, down: bool) -> io::Result<()> {
        // Keys macOS has no code for are dropped.
        let Some(keycode) = sc_keymap::mac_from_hid(usage) else { return Ok(()) };
        let is_modifier = usage::is_modifier(usage);
        if is_modifier {
            if down {
                self.modifiers.insert(usage);
            } else {
                self.modifiers.remove(&usage);
            }
        }

        let event = unsafe { CGEventCreateKeyboardEvent(ptr::null_mut(), keycode, down) };
        let extra = key_flags(usage);
        self.post(event, |e| unsafe {
            if is_modifier {
                CGEventSetType(e, kCGEventFlagsChanged);
            }
            CGEventSetFlags(e, CGEventGetFlags(e) | extra);
        })
    }

    fn modifier_flags(&self) -> CGEventFlags {
        self.modifiers.iter().fold(0, |flags, &m| {
            let common = match m {
                usage::LEFT_CTRL | usage::RIGHT_CTRL => kCGEventFlagMaskControl,
                usage::LEFT_SHIFT | usage::RIGHT_SHIFT => kCGEventFlagMaskShift,
                usage::LEFT_ALT | usage::RIGHT_ALT => kCGEventFlagMaskAlternate,
                _ => kCGEventFlagMaskCommand,
            };
            flags | common | device_mask(m).unwrap_or(0)
        })
    }

    /// Applies modifier flags and our marker, lets `adjust` finish the event,
    /// then posts and releases it.
    fn post(&self, event: CGEventRef, adjust: impl FnOnce(CGEventRef)) -> io::Result<()> {
        if event.is_null() {
            return Err(io::Error::other("CoreGraphics could not create the event"));
        }
        unsafe {
            CGEventSetFlags(event, self.modifier_flags());
            CGEventSetIntegerValueField(event, kCGEventSourceUserData, INJECT_MARKER);
            adjust(event);
            CGEventPost(kCGHIDEventTap, event);
            CFRelease(event.cast_const());
        }
        Ok(())
    }
}

impl Default for Injector {
    fn default() -> Self {
        Self::new()
    }
}

/// Extra flags real keyboards set on arrow, navigation and function keys;
/// some apps ignore these keys without them.
fn key_flags(usage: u16) -> CGEventFlags {
    match usage {
        0x4F..=0x52 => kCGEventFlagMaskNumericPad | kCGEventFlagMaskSecondaryFn, // arrows
        0x49..=0x4E => kCGEventFlagMaskSecondaryFn, // Insert/Help .. Page Down
        0x3A..=0x45 | 0x68..=0x6F => kCGEventFlagMaskSecondaryFn, // F1-F12, F13-F20
        0x53..=0x63 | 0x67 => kCGEventFlagMaskNumericPad, // keypad
        _ => 0,
    }
}

pub(super) fn current_location() -> Option<CGPoint> {
    unsafe {
        let event = CGEventCreate(ptr::null_mut());
        if event.is_null() {
            return None;
        }
        let p = CGEventGetLocation(event);
        CFRelease(event.cast_const());
        Some(p)
    }
}
