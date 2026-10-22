//! Windows backend: low-level hooks for capture, `SendInput` for injection.

mod capture;
mod inject;
mod screen;

pub use capture::Capture;
pub use inject::Injector;
pub use screen::{cursor_image, cursor_pos, monitors, virtual_desktop};

use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};

/// `dwExtraInfo` tag on every event we inject, so our own hooks can skip them.
pub const INJECT_MARKER: usize = 0x5343_4B56;

/// Opts the process into per-monitor DPI awareness so every coordinate is in
/// physical pixels. Call once at startup, before creating hooks or windows.
pub fn init() {
    // Fails harmlessly if awareness was already set (e.g. by a manifest).
    let _ = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
}

/// macOS-only concept; Windows hooks always see the keyboard.
pub fn secure_input_active() -> bool {
    false
}

/// Windows needs no permission for hooks or `SendInput`.
pub fn permissions() -> crate::Permissions {
    crate::Permissions { listen: true, post: true }
}
