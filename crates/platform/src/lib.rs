//! Platform layer: capture physical keyboard/mouse input and inject synthetic input.
//!
//! Each backend module exposes the same API; [`sys`] is the one for the
//! current OS:
//!
//! - `init()`: once at startup (DPI awareness, permission prompts)
//! - `Capture::start() -> (Capture, Receiver<CaptureEvent>)`, `set_grab(bool)`
//! - `Injector::new()`, `inject(&InputEvent)`
//! - `monitors()`, `cursor_pos()`, `secure_input_active()`

mod event;

pub use event::{CaptureEvent, Monitor};
pub use sc_proto::{InputEvent, MouseButton, Rect};

#[cfg(target_os = "macos")]
pub mod mac;
#[cfg(windows)]
pub mod win;

#[cfg(target_os = "macos")]
pub use mac as sys;
#[cfg(windows)]
pub use win as sys;
