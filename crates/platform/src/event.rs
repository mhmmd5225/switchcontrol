use sc_proto::{InputEvent, Rect};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureEvent {
    Input(InputEvent),
    /// The user pressed the emergency hotkey during a grab; the grab has ended.
    EmergencyRelease,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Monitor {
    pub name: String,
    pub bounds: Rect,
    pub primary: bool,
}

/// Whether the OS lets this process see and post input (macOS privacy settings).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Permissions {
    /// Input Monitoring: needed to capture keyboard and mouse.
    pub listen: bool,
    /// Accessibility: needed to post input and to block it while grabbing.
    pub post: bool,
}

impl Permissions {
    pub fn all_granted(self) -> bool {
        self.listen && self.post
    }
}
