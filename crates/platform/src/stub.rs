//! Fallback for other systems (e.g. Linux): no capture or injection, one
//! fixed screen. Lets the app build and run with `--dry-run` for development
//! and tests.

use std::io;
use std::sync::mpsc::Receiver;

use crate::{CaptureEvent, InputEvent, Monitor, Permissions, Rect};

fn unsupported() -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, "input capture is not supported on this system")
}

pub fn init() {}

pub fn secure_input_active() -> bool {
    false
}

pub fn permissions() -> Permissions {
    Permissions { listen: true, post: true }
}

pub fn monitors() -> Vec<Monitor> {
    vec![Monitor {
        name: "virtual".into(),
        bounds: Rect { left: 0, top: 0, right: 1920, bottom: 1080 },
        primary: true,
    }]
}

pub fn cursor_pos() -> Option<(i32, i32)> {
    None
}

pub struct Capture;

impl Capture {
    pub fn start() -> io::Result<(Capture, Receiver<CaptureEvent>)> {
        Err(unsupported())
    }

    pub fn set_grab(&self, _on: bool) {}

    pub fn is_grabbing(&self) -> bool {
        false
    }
}

#[derive(Default)]
pub struct Injector;

impl Injector {
    pub fn new() -> Self {
        Injector
    }

    pub fn inject(&mut self, _ev: &InputEvent) -> io::Result<()> {
        Err(unsupported())
    }
}
