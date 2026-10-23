//! macOS backend: a CoreGraphics event tap for capture, `CGEventPost` for
//! injection.
//!
//! Needs the Accessibility and Input Monitoring permissions (System Settings >
//! Privacy & Security). Untested on real hardware yet: this module is only
//! type-checked from Windows.

// Matches on Apple's constant names, e.g. `kCGEventMouseMoved`.
#[allow(non_upper_case_globals)]
mod capture;
mod ffi;
mod inject;

pub use capture::Capture;
pub use inject::Injector;

use crate::{Monitor, Permissions, Rect};

/// `kCGEventSourceUserData` tag on every event we post, so our tap skips them.
pub const INJECT_MARKER: i64 = 0x5343_4B56;

/// Asks for the Input Monitoring and Accessibility permissions. macOS shows
/// each prompt once; afterwards the user must change them in System Settings.
pub fn init() {
    unsafe {
        ffi::CGRequestListenEventAccess();
        ffi::CGRequestPostEventAccess();
    }
}

/// Current state of the two privacy permissions, without prompting.
pub fn permissions() -> Permissions {
    unsafe {
        Permissions {
            listen: ffi::CGPreflightListenEventAccess(),
            post: ffi::CGPreflightPostEventAccess(),
        }
    }
}

/// While a password field has focus, macOS hides keystrokes from event taps.
pub fn secure_input_active() -> bool {
    unsafe { ffi::IsSecureEventInputEnabled() != 0 }
}

/// Displays in global coordinates (points, origin at the main display's top-left).
pub fn monitors() -> Vec<Monitor> {
    let mut ids = [0u32; 16];
    let mut count = 0u32;
    if unsafe { ffi::CGGetActiveDisplayList(ids.len() as u32, ids.as_mut_ptr(), &mut count) } != 0 {
        return Vec::new();
    }
    let main = unsafe { ffi::CGMainDisplayID() };
    ids[..count as usize]
        .iter()
        .map(|&id| {
            let b = unsafe { ffi::CGDisplayBounds(id) };
            let left = b.origin.x.round() as i32;
            let top = b.origin.y.round() as i32;
            Monitor {
                name: format!("display {id}"),
                bounds: Rect {
                    left,
                    top,
                    right: left + b.size.width.round() as i32,
                    bottom: top + b.size.height.round() as i32,
                },
                primary: id == main,
            }
        })
        .collect()
}

/// HID usages of the keys held on the keyboard right now.
pub fn held_keys() -> Vec<u16> {
    keys_down(ffi::kCGEventSourceStateHIDSystemState)
}

/// HID usages of the keys macOS treats as down, including injected ones.
pub fn logical_keys() -> Vec<u16> {
    keys_down(ffi::kCGEventSourceStateCombinedSessionState)
}

fn keys_down(state: ffi::CGEventSourceStateID) -> Vec<u16> {
    (0u16..0x80)
        .filter(|&code| unsafe { ffi::CGEventSourceKeyState(state, code) })
        .filter_map(sc_keymap::hid_from_mac)
        .collect()
}

pub fn cursor_pos() -> Option<(i32, i32)> {
    let p = inject::current_location()?;
    Some((p.x.floor() as i32, p.y.floor() as i32))
}
