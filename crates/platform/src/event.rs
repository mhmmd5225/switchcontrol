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
