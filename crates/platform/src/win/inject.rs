//! Input injection via `SendInput`.

use std::io;

use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBD_EVENT_FLAGS, KEYBDINPUT,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, MOUSE_EVENT_FLAGS,
    MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
    MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN,
    MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL, MOUSEEVENTF_XDOWN,
    MOUSEEVENTF_XUP, MOUSEINPUT, SendInput, VIRTUAL_KEY,
};
use windows::core::{Error, Result};

use super::{INJECT_MARKER, virtual_desktop};
use crate::{InputEvent, MouseButton, Rect};

pub struct Injector {
    desktop: Rect,
    marker: usize,
}

impl Injector {
    /// Injected events are tagged so this process's own capture ignores them.
    pub fn new() -> Self {
        Self { desktop: virtual_desktop(), marker: INJECT_MARKER }
    }

    /// Injected events look like any other software input to our capture.
    /// Only useful for self-tests of the capture path.
    pub fn untagged() -> Self {
        Self { desktop: virtual_desktop(), marker: 0 }
    }

    /// Re-reads monitor geometry; call after a display configuration change.
    pub fn refresh_geometry(&mut self) {
        self.desktop = virtual_desktop();
    }

    pub fn inject(&mut self, ev: &InputEvent) -> io::Result<()> {
        self.inject_event(ev).map_err(io::Error::other)
    }

    fn inject_event(&self, ev: &InputEvent) -> Result<()> {
        match *ev {
            InputEvent::MouseMove { x, y } => self.send(&[self.mouse(
                normalize(x, self.desktop.left, self.desktop.width()),
                normalize(y, self.desktop.top, self.desktop.height()),
                0,
                MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
            )]),
            // Relative moves go through the local pointer acceleration curve.
            InputEvent::MouseDelta { dx, dy } => {
                self.send(&[self.mouse(dx, dy, 0, MOUSEEVENTF_MOVE)])
            }
            InputEvent::Button { button, down } => {
                let (flags, data) = button_flags(button, down);
                self.send(&[self.mouse(0, 0, data, flags)])
            }
            InputEvent::Wheel { dx, dy } => {
                let mut inputs = Vec::with_capacity(2);
                if dy != 0 {
                    inputs.push(self.mouse(0, 0, dy as u32, MOUSEEVENTF_WHEEL));
                }
                if dx != 0 {
                    inputs.push(self.mouse(0, 0, dx as u32, MOUSEEVENTF_HWHEEL));
                }
                self.send(&inputs)
            }
            InputEvent::Key { usage, down } => {
                // Keys Windows has no scan code for are dropped.
                let Some(scan) = sc_keymap::win_from_hid(usage) else { return Ok(()) };
                let mut flags = KEYEVENTF_SCANCODE;
                if scan & 0xFF00 == 0xE000 {
                    flags |= KEYEVENTF_EXTENDEDKEY;
                }
                if !down {
                    flags |= KEYEVENTF_KEYUP;
                }
                self.send(&[self.key(scan & 0xFF, flags)])
            }
        }
    }

    fn mouse(&self, dx: i32, dy: i32, data: u32, flags: MOUSE_EVENT_FLAGS) -> INPUT {
        INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx,
                    dy,
                    mouseData: data,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: self.marker,
                },
            },
        }
    }

    fn key(&self, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0),
                    wScan: scan,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: self.marker,
                },
            },
        }
    }

    fn send(&self, inputs: &[INPUT]) -> Result<()> {
        if inputs.is_empty() {
            return Ok(());
        }
        let sent = unsafe { SendInput(inputs, size_of::<INPUT>() as i32) };
        // A short count means UIPI blocked us (elevated foreground window) or
        // the input desktop is secure (UAC, lock screen).
        if sent as usize != inputs.len() { Err(Error::from_win32()) } else { Ok(()) }
    }
}

impl Default for Injector {
    fn default() -> Self {
        Self::new()
    }
}

/// Maps a pixel coordinate onto the 0..=65535 range `MOUSEEVENTF_ABSOLUTE` expects.
fn normalize(value: i32, origin: i32, extent: i32) -> i32 {
    if extent <= 1 {
        return 0;
    }
    ((value - origin) as i64 * 65535 / (extent - 1) as i64) as i32
}

fn button_flags(button: MouseButton, down: bool) -> (MOUSE_EVENT_FLAGS, u32) {
    match (button, down) {
        (MouseButton::Left, true) => (MOUSEEVENTF_LEFTDOWN, 0),
        (MouseButton::Left, false) => (MOUSEEVENTF_LEFTUP, 0),
        (MouseButton::Right, true) => (MOUSEEVENTF_RIGHTDOWN, 0),
        (MouseButton::Right, false) => (MOUSEEVENTF_RIGHTUP, 0),
        (MouseButton::Middle, true) => (MOUSEEVENTF_MIDDLEDOWN, 0),
        (MouseButton::Middle, false) => (MOUSEEVENTF_MIDDLEUP, 0),
        (MouseButton::Back, true) => (MOUSEEVENTF_XDOWN, 1),
        (MouseButton::Back, false) => (MOUSEEVENTF_XUP, 1),
        (MouseButton::Forward, true) => (MOUSEEVENTF_XDOWN, 2),
        (MouseButton::Forward, false) => (MOUSEEVENTF_XUP, 2),
    }
}

#[cfg(test)]
mod tests {
    use super::normalize;

    #[test]
    fn normalize_maps_edges_to_full_range() {
        assert_eq!(normalize(0, 0, 1920), 0);
        assert_eq!(normalize(1919, 0, 1920), 65535);
        // Monitor left of the primary: virtual desktop starts at a negative x.
        assert_eq!(normalize(-1920, -1920, 3840), 0);
        assert_eq!(normalize(1919, -1920, 3840), 65535);
    }
}
