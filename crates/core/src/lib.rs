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
use std::time::{Duration, Instant};

use sc_keymap::swap_ctrl_gui;
use sc_proto::{Hello, InputEvent, Message, MouseButton, Os, PROTOCOL_VERSION, Rect};

pub use layout::Side;
use layout::{clamp_to_screens, map_span, on_edge};
pub use layout::{edge_span, edge_x};

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    /// Where the peer's screens sit relative to ours.
    pub peer_side: Side,
    /// Swap Ctrl and Windows/Command when receiving from the other OS.
    pub swap_ctrl_cmd: bool,
    /// Multiplier for motion sent to the peer.
    pub speed: f64,
    /// How long the cursor must keep pushing against the edge before control
    /// passes to the peer; zero crosses on touch.
    pub edge_delay: Duration,
}

/// Distance from the shared edge (in screen units) at which the edge bar starts to show.
pub const EDGE_NEAR: i32 = 64;
/// Steps in which [`EdgeHint`] values change, so the bar is not redrawn on every move.
pub const EDGE_STEPS: u8 = 10;

/// The part of the shared edge the cursor is near, for drawing a bar there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EdgeHint {
    pub side: Side,
    /// The outermost pixel column on that side.
    pub x: i32,
    /// Vertical extent of the monitor the cursor is on.
    pub top: i32,
    pub bottom: i32,
    /// How close the cursor is, from 1 (just in range) to [`EDGE_STEPS`] (touching).
    pub near: u8,
    /// How far a push against the edge has got, 0 to [`EDGE_STEPS`].
    pub push: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Send(Message),
    Inject(InputEvent),
    /// Start or stop swallowing local input. Starting must also release, on
    /// this machine, whatever keys are held at that moment: their key-ups
    /// will be swallowed.
    Grab(bool),
    /// Show the edge bar, or hide it.
    Edge(Option<EdgeHint>),
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
    /// Physical buttons currently held on this machine.
    local_buttons: BTreeSet<MouseButton>,
    /// Since when the cursor has been pushing against the shared edge.
    edge_since: Option<Instant>,
    /// The edge bar as last reported.
    edge_hint: Option<EdgeHint>,
    /// Buttons we pressed on the peer and have not released yet.
    forwarded_buttons: BTreeSet<MouseButton>,
    /// Whether the peer is currently driving this machine.
    controlled: bool,
    injected_keys: BTreeSet<u16>,
    injected_buttons: BTreeSet<MouseButton>,
    /// A Command key from the Mac, injected here as Ctrl, with nothing else
    /// pressed since. Released alone, it becomes a Windows key tap.
    lone_command: Option<u16>,
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
            local_buttons: BTreeSet::new(),
            edge_since: None,
            edge_hint: None,
            forwarded_buttons: BTreeSet::new(),
            controlled: false,
            injected_keys: BTreeSet::new(),
            injected_buttons: BTreeSet::new(),
            lone_command: None,
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
        self.edge_since = None;
        self.set_hint(None, &mut out);
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
        self.local_input_at(ev, Instant::now())
    }

    /// [`Self::local_input`] at a given time, for tests.
    pub fn local_input_at(&mut self, ev: InputEvent, now: Instant) -> Vec<Action> {
        let mut out = Vec::new();
        if let InputEvent::Button { button, down } = ev {
            set(&mut self.local_buttons, button, down);
        }

        match self.mode {
            Mode::Local => match ev {
                InputEvent::MouseMove { x, y } => self.maybe_enter(x, y, now, &mut out),
                // A drag hides the bar: it cannot cross.
                InputEvent::Button { down: true, .. } => {
                    self.edge_since = None;
                    self.set_hint(None, &mut out);
                }
                _ => {}
            },
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
                    // Cmd+click is a Ctrl+click, not a Windows key.
                    self.lone_command = None;
                    self.injected_buttons.insert(button);
                    out.push(Action::Inject(InputEvent::Button { button, down }));
                } else if self.injected_buttons.remove(&button) {
                    out.push(Action::Inject(InputEvent::Button { button, down }));
                }
            }
            Message::Wheel { dx, dy } => {
                self.lone_command = None;
                out.push(Action::Inject(InputEvent::Wheel { dx, dy }));
            }
            Message::Key { usage: sent, down } => {
                let usage = self.remap(sent);
                let command_as_ctrl = usage != sent && self.os == Os::Windows && is_gui(sent);
                if down {
                    self.lone_command = command_as_ctrl.then_some(sent);
                    self.injected_keys.insert(usage);
                    out.push(Action::Inject(InputEvent::Key { usage, down }));
                } else if self.injected_keys.remove(&usage) {
                    out.push(Action::Inject(InputEvent::Key { usage, down }));
                    // Cmd pressed and released alone: what a Windows key would do.
                    if self.lone_command.take() == Some(sent) {
                        out.push(Action::Inject(InputEvent::Key { usage: sent, down: true }));
                        out.push(Action::Inject(InputEvent::Key { usage: sent, down: false }));
                    }
                } else {
                    self.lone_command = None;
                }
            }
        }
        out
    }

    fn maybe_enter(&mut self, x: i32, y: i32, now: Instant, out: &mut Vec<Action>) {
        let Some(peer) = &self.peer else { return };
        let side = self.config.peer_side;
        let (Some(from), Some(to), Some(peer_x), Some(edge)) = (
            edge_span(&self.screens, side),
            edge_span(&peer.screens, side.opposite()),
            edge_x(&peer.screens, side.opposite()),
            edge_x(&self.screens, side),
        ) else {
            return;
        };
        // No crossing mid-drag: the button-up would land on the wrong machine.
        let dragging = !self.local_buttons.is_empty();
        if dragging || !on_edge(&self.screens, side, x, y) {
            self.edge_since = None;
            let hint = if dragging { None } else { self.hint_near(x, y, edge, 0) };
            self.set_hint(hint, out);
            return;
        }

        let delay = self.config.edge_delay;
        if !delay.is_zero() {
            let pushed = now.saturating_duration_since(*self.edge_since.get_or_insert(now));
            if pushed < delay {
                let push = (pushed.as_secs_f64() / delay.as_secs_f64() * EDGE_STEPS as f64) as u8;
                let hint = self.hint_near(x, y, edge, push);
                self.set_hint(hint, out);
                return;
            }
        }
        self.edge_since = None;
        self.set_hint(None, out);

        let peer_y = map_span(y, from, to);
        out.push(Action::Grab(true));
        out.push(Action::Send(Message::Enter { x: peer_x, y: peer_y }));
        out.push(Action::Log(format!("-> {} at ({peer_x}, {peer_y})", self.peer_name())));
        self.mode = Mode::Remote { x: peer_x as f64, y: peer_y as f64 };
    }

    /// The bar for a cursor at (x, y), if it is close to the shared edge.
    fn hint_near(&self, x: i32, y: i32, edge: i32, push: u8) -> Option<EdgeHint> {
        let side = self.config.peer_side;
        // Positions past the edge count as touching it.
        let screen = self.screens.iter().find(|r| {
            let reaches = match side {
                Side::Left => r.left == edge && x < r.right,
                Side::Right => r.right - 1 == edge && x >= r.left,
            };
            reaches && r.top <= y && y < r.bottom
        })?;
        let distance = match side {
            Side::Left => x - edge,
            Side::Right => edge - x,
        }
        .max(0);
        if distance >= EDGE_NEAR {
            return None;
        }
        let near = EDGE_STEPS - (distance * EDGE_STEPS as i32 / EDGE_NEAR) as u8;
        Some(EdgeHint { side, x: edge, top: screen.top, bottom: screen.bottom, near, push })
    }

    fn set_hint(&mut self, hint: Option<EdgeHint>, out: &mut Vec<Action>) {
        if hint != self.edge_hint {
            self.edge_hint = hint;
            out.push(Action::Edge(hint));
        }
    }

    fn peer_name(&self) -> &str {
        self.peer.as_ref().map_or("peer", |p| p.name.as_str())
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
        self.lone_command = None;
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

fn is_gui(usage: u16) -> bool {
    usage == sc_keymap::usage::LEFT_GUI || usage == sc_keymap::usage::RIGHT_GUI
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
