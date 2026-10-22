//! Decision logic for one SwitchControl node, free of I/O and OS calls.
//!
//! Every node runs the same logic. Whichever machine has the physical
//! keyboard and mouse acts as the controller: when its cursor hits the edge
//! facing the peer, it grabs local input and drives a virtual cursor on the
//! peer's screens. The peer, while controlled, injects what it receives.
//!
//! The app feeds a [`Node`] local input, peer messages and connection
//! changes; the node answers with [`Action`]s to carry out in order.

mod layout;

use std::collections::BTreeSet;

use sc_keymap::swap_ctrl_gui;
use sc_proto::{Hello, InputEvent, Message, MouseButton, Os, PROTOCOL_VERSION, Rect};

pub use layout::Side;
use layout::{clamp_to_screens, edge_span, edge_x, map_span, on_edge};

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    /// Where the peer's screens sit relative to ours.
    pub peer_side: Side,
    /// Swap Ctrl and Windows/Command when receiving from the other OS.
    pub swap_ctrl_cmd: bool,
    /// Multiplier for motion sent to the peer.
    pub speed: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Send(Message),
    Inject(InputEvent),
    /// Start or stop swallowing local input.
    Grab(bool),
    Log(String),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Mode {
    Local,
    /// Our input drives the peer; (x, y) is the virtual cursor in its space.
    Remote {
        x: f64,
        y: f64,
    },
}

#[derive(Debug, Clone)]
struct Peer {
    name: String,
    os: Os,
    screens: Vec<Rect>,
}

pub struct Node {
    config: Config,
    os: Os,
    name: String,
    screens: Vec<Rect>,
    peer: Option<Peer>,
    mode: Mode,
    /// Physical keys and buttons currently held on this machine.
    local_keys: BTreeSet<u16>,
    local_buttons: BTreeSet<MouseButton>,
    /// Buttons we pressed on the peer and have not released yet.
    forwarded_buttons: BTreeSet<MouseButton>,
    /// Whether the peer is currently driving this machine.
    controlled: bool,
    injected_keys: BTreeSet<u16>,
    injected_buttons: BTreeSet<MouseButton>,
}

impl Node {
    pub fn new(config: Config, os: Os, name: String, screens: Vec<Rect>) -> Self {
        Node {
            config,
            os,
            name,
            screens,
            peer: None,
            mode: Mode::Local,
            local_keys: BTreeSet::new(),
            local_buttons: BTreeSet::new(),
            forwarded_buttons: BTreeSet::new(),
            controlled: false,
            injected_keys: BTreeSet::new(),
            injected_buttons: BTreeSet::new(),
        }
    }

    pub fn hello(&self) -> Hello {
        Hello {
            version: PROTOCOL_VERSION,
            name: self.name.clone(),
            os: self.os,
            screens: self.screens.clone(),
        }
    }

    pub fn is_remote(&self) -> bool {
        matches!(self.mode, Mode::Remote { .. })
    }

    pub fn peer_connected(&mut self, hello: Hello) -> Vec<Action> {
        let log =
            format!("connected to {} ({:?}, screens {:?})", hello.name, hello.os, hello.screens);
        self.peer = Some(Peer { name: hello.name, os: hello.os, screens: hello.screens });
        vec![Action::Log(log)]
    }

    pub fn peer_disconnected(&mut self) -> Vec<Action> {
        let mut out = Vec::new();
        if self.is_remote() {
            self.leave_remote(&mut out, false, None);
        }
        self.release_injected(&mut out);
        self.controlled = false;
        if let Some(peer) = self.peer.take() {
            out.push(Action::Log(format!("disconnected from {}", peer.name)));
        }
        out
    }

    /// The platform ended a grab on its own (emergency hotkey).
    pub fn emergency_release(&mut self) -> Vec<Action> {
        let mut out = Vec::new();
        if self.is_remote() {
            self.leave_remote(&mut out, true, None);
            out.push(Action::Log("emergency release".into()));
        }
        out
    }

    pub fn local_input(&mut self, ev: InputEvent) -> Vec<Action> {
        let mut out = Vec::new();
        match ev {
            InputEvent::Key { usage, down } => set(&mut self.local_keys, usage, down),
            InputEvent::Button { button, down } => set(&mut self.local_buttons, button, down),
            _ => {}
        }

        match self.mode {
            Mode::Local => {
                if let InputEvent::MouseMove { x, y } = ev {
                    self.maybe_enter(x, y, &mut out);
                }
            }
            Mode::Remote { .. } => match ev {
                InputEvent::MouseDelta { dx, dy } => self.move_remote(dx, dy, &mut out),
                InputEvent::Button { button, down } => {
                    set(&mut self.forwarded_buttons, button, down);
                    out.push(Action::Send(Message::Button { button, down }));
                }
                InputEvent::Wheel { dx, dy } => out.push(Action::Send(Message::Wheel { dx, dy })),
                InputEvent::Key { usage, down } => {
                    out.push(Action::Send(Message::Key { usage, down }))
                }
                // Absolute moves are not reported while grabbing.
                InputEvent::MouseMove { .. } => {}
            },
        }
        out
    }

    pub fn peer_message(&mut self, msg: Message) -> Vec<Action> {
        let mut out = Vec::new();
        match msg {
            Message::Hello(_) | Message::Ping => {}
            Message::Enter { x, y } => {
                // Both sides tried to take control; the peer's claim wins.
                if self.is_remote() {
                    self.leave_remote(&mut out, true, None);
                }
                self.controlled = true;
                out.push(Action::Inject(InputEvent::MouseMove { x, y }));
                out.push(Action::Log("peer took control".into()));
            }
            Message::Leave => {
                self.release_injected(&mut out);
                if std::mem::take(&mut self.controlled) {
                    out.push(Action::Log("peer released control".into()));
                }
            }
            Message::ReleaseAll => self.release_injected(&mut out),
            _ if !self.controlled => {}
            Message::MouseMove { x, y } => out.push(Action::Inject(InputEvent::MouseMove { x, y })),
            Message::Button { button, down } => {
                if down {
                    self.injected_buttons.insert(button);
                    out.push(Action::Inject(InputEvent::Button { button, down }));
                } else if self.injected_buttons.remove(&button) {
                    out.push(Action::Inject(InputEvent::Button { button, down }));
                }
            }
            Message::Wheel { dx, dy } => out.push(Action::Inject(InputEvent::Wheel { dx, dy })),
            Message::Key { usage, down } => {
                let usage = self.remap(usage);
                if down {
                    self.injected_keys.insert(usage);
                    out.push(Action::Inject(InputEvent::Key { usage, down }));
                } else if self.injected_keys.remove(&usage) {
                    out.push(Action::Inject(InputEvent::Key { usage, down }));
                }
            }
        }
        out
    }

    fn maybe_enter(&mut self, x: i32, y: i32, out: &mut Vec<Action>) {
        let Some(peer) = &self.peer else { return };
        let side = self.config.peer_side;
        // No crossing mid-drag: the button-up would land on the wrong machine.
        if !self.local_buttons.is_empty() || !on_edge(&self.screens, side, x, y) {
            return;
        }
        let (Some(from), Some(to), Some(peer_x)) = (
            edge_span(&self.screens, side),
            edge_span(&peer.screens, side.opposite()),
            edge_x(&peer.screens, side.opposite()),
        ) else {
            return;
        };
        let peer_y = map_span(y, from, to);

        out.push(Action::Grab(true));
        // Keys held while crossing would never see their key-up locally.
        for &usage in &self.local_keys {
            out.push(Action::Inject(InputEvent::Key { usage, down: false }));
        }
        out.push(Action::Send(Message::Enter { x: peer_x, y: peer_y }));
        out.push(Action::Log(format!("-> {} at ({peer_x}, {peer_y})", peer.name)));
        self.mode = Mode::Remote { x: peer_x as f64, y: peer_y as f64 };
    }

    fn move_remote(&mut self, dx: i32, dy: i32, out: &mut Vec<Action>) {
        let (Mode::Remote { x, y }, Some(peer)) = (self.mode, &self.peer) else { return };
        let nx = x + dx as f64 * self.config.speed;
        let ny = y + dy as f64 * self.config.speed;

        // The peer's edge facing us.
        let back = self.config.peer_side.opposite();
        if let Some(edge) = edge_x(&peer.screens, back) {
            let crossed = match back {
                Side::Left => nx < edge as f64,
                Side::Right => nx >= (edge + 1) as f64,
            };
            let row = ny.floor() as i32;
            if crossed
                && self.forwarded_buttons.is_empty()
                && on_edge(&peer.screens, back, edge, row)
            {
                self.leave_remote(out, true, Some(row));
                return;
            }
        }

        let (cx, cy) = clamp_to_screens(&peer.screens, nx, ny);
        self.mode = Mode::Remote { x: cx, y: cy };
        let (px, py) = (cx.floor() as i32, cy.floor() as i32);
        if (px, py) != (x.floor() as i32, y.floor() as i32) {
            out.push(Action::Send(Message::MouseMove { x: px, y: py }));
        }
    }

    /// Returns control to this machine. With `peer_y`, the local cursor is
    /// placed at the matching height just inside our edge facing the peer.
    fn leave_remote(&mut self, out: &mut Vec<Action>, notify: bool, peer_y: Option<i32>) {
        self.mode = Mode::Local;
        self.forwarded_buttons.clear();
        if notify {
            out.push(Action::Send(Message::Leave));
        }
        out.push(Action::Grab(false));

        let side = self.config.peer_side;
        let target = peer_y.zip(self.peer.as_ref()).and_then(|(py, peer)| {
            let from = edge_span(&peer.screens, side.opposite())?;
            let to = edge_span(&self.screens, side)?;
            let edge = edge_x(&self.screens, side)?;
            let x = match side {
                Side::Left => edge + 1,
                Side::Right => edge - 1,
            };
            Some((x, map_span(py, from, to)))
        });
        if let Some((x, y)) = target {
            out.push(Action::Inject(InputEvent::MouseMove { x, y }));
        }
        out.push(Action::Log("<- back to local".into()));
    }

    fn release_injected(&mut self, out: &mut Vec<Action>) {
        // Ascending order releases modifiers (0xE0..) after ordinary keys.
        for usage in std::mem::take(&mut self.injected_keys) {
            out.push(Action::Inject(InputEvent::Key { usage, down: false }));
        }
        for button in std::mem::take(&mut self.injected_buttons) {
            out.push(Action::Inject(InputEvent::Button { button, down: false }));
        }
    }

    fn remap(&self, usage: u16) -> u16 {
        let cross_os = self.peer.as_ref().is_some_and(|p| p.os != self.os);
        if self.config.swap_ctrl_cmd && cross_os { swap_ctrl_gui(usage) } else { usage }
    }
}

fn set<T: Ord>(set: &mut BTreeSet<T>, item: T, present: bool) {
    if present {
        set.insert(item);
    } else {
        set.remove(&item);
    }
}

#[cfg(test)]
mod tests;
