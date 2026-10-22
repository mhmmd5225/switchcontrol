//! Types shared by every crate, and the wire protocol between two peers.
//!
//! A connection runs inside mutually authenticated TLS. It starts with
//! [`Auth`] frames (trust check, then PIN pairing if needed), followed by
//! [`Message`] frames beginning with [`Hello`].
//!
//! Every frame on the wire is a little-endian `u32` length followed by a
//! postcard-encoded body. Coordinates in messages are always in the
//! receiver's own screen space (physical pixels on Windows, points on macOS);
//! the sender learns that space from the receiver's [`Hello`].

use std::io::{self, Read, Write};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u16 = 2;
pub const DEFAULT_PORT: u16 = 24800;

/// Upper bound on a frame body; real messages are a few dozen bytes.
const MAX_FRAME: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Os {
    Windows,
    MacOs,
    Other,
}

impl Os {
    pub const fn current() -> Os {
        if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::MacOs
        } else {
            Os::Other
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Back,
    Forward,
}

/// A rectangle with exclusive `right`/`bottom` edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub fn width(&self) -> i32 {
        self.right - self.left
    }

    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }
}

/// One input event, as captured locally or to be injected locally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEvent {
    /// Absolute cursor position in local screen coordinates.
    MouseMove {
        x: i32,
        y: i32,
    },
    /// Relative motion while input is grabbed, after OS pointer acceleration.
    MouseDelta {
        dx: i32,
        dy: i32,
    },
    Button {
        button: MouseButton,
        down: bool,
    },
    /// Wheel motion, 120 per notch. Positive `dy` scrolls away from the user,
    /// positive `dx` scrolls right.
    Wheel {
        dx: i32,
        dy: i32,
    },
    /// A physical key, as a USB HID usage on the keyboard page (0x07).
    Key {
        usage: u16,
        down: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub version: u16,
    pub name: String,
    pub os: Os,
    pub screens: Vec<Rect>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Message {
    /// First message in each direction after connecting.
    Hello(Hello),
    /// The sender's cursor crossed onto the receiver's screen at (x, y);
    /// the receiver is now controlled until [`Message::Leave`].
    Enter {
        x: i32,
        y: i32,
    },
    /// The cursor went back; the receiver releases everything it holds.
    Leave,
    MouseMove {
        x: i32,
        y: i32,
    },
    Button {
        button: MouseButton,
        down: bool,
    },
    Wheel {
        dx: i32,
        dy: i32,
    },
    Key {
        usage: u16,
        down: bool,
    },
    /// Release every key and button injected so far, without leaving.
    ReleaseAll,
    /// Heartbeat, sent every second by both sides.
    Ping,
}

/// Frames exchanged right after the TLS handshake, before [`Message::Hello`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Auth {
    /// Whether the sender already trusts the receiver's certificate.
    /// Unless both sides do, they pair with a PIN.
    Status { trusted: bool },
    /// SPAKE2 message derived from the PIN.
    Spake(Vec<u8>),
    /// Key confirmation: proves the sender derived the same key.
    Confirm(Vec<u8>),
}

pub fn write_frame<T: Serialize>(w: &mut impl Write, msg: &T) -> io::Result<()> {
    let body = postcard::to_stdvec(msg).map_err(io::Error::other)?;
    let mut frame = Vec::with_capacity(4 + body.len());
    frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
    frame.extend_from_slice(&body);
    w.write_all(&frame)
}

pub fn read_frame<T: DeserializeOwned>(r: &mut impl Read) -> io::Result<T> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len)?;
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("frame too large: {len} bytes"),
        ));
    }
    let mut body = vec![0; len];
    r.read_exact(&mut body)?;
    postcard::from_bytes(&body).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip() {
        let messages = [
            Message::Hello(Hello {
                version: PROTOCOL_VERSION,
                name: "mac".into(),
                os: Os::MacOs,
                screens: vec![Rect { left: 0, top: 0, right: 1512, bottom: 982 }],
            }),
            Message::Enter { x: 0, y: -40 },
            Message::MouseMove { x: 1511, y: 981 },
            Message::Button { button: MouseButton::Forward, down: true },
            Message::Wheel { dx: -120, dy: 360 },
            Message::Key { usage: 0xE3, down: false },
            Message::Leave,
            Message::ReleaseAll,
            Message::Ping,
        ];
        let mut wire = Vec::new();
        for msg in &messages {
            write_frame(&mut wire, msg).unwrap();
        }
        let mut reader = wire.as_slice();
        for msg in &messages {
            assert_eq!(&read_frame::<Message>(&mut reader).unwrap(), msg);
        }
        assert!(reader.is_empty());
    }

    #[test]
    fn oversized_frame_is_rejected() {
        let wire = (MAX_FRAME as u32 + 1).to_le_bytes();
        let err = read_frame::<Message>(&mut wire.as_slice()).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn truncated_frame_is_an_error() {
        let mut wire = Vec::new();
        write_frame(&mut wire, &Message::MouseMove { x: 5, y: 5 }).unwrap();
        wire.pop();
        assert!(read_frame::<Message>(&mut wire.as_slice()).is_err());
    }
}
