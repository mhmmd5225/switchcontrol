//! The running app: capture, the connection and the hand-over logic, on a
//! thread of its own so that a window (or the terminal) can drive it.
//!
//! Everything the user should see comes out through a [`Reporter`].

use std::error::Error;
use std::net::SocketAddr;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use sc_core::{Action, Config, EdgeHint, Node};
use sc_platform::sys::{self, Capture, Injector};
use sc_platform::{CaptureEvent, InputEvent};
use sc_proto::{Message, Os};

use crate::config::Settings;
use crate::identity::{Identity, TrustStore};
use crate::net::{self, Link, NetContext, NetEvent, PinSource, Waiting};
use crate::tls::TlsConfig;

pub enum AppEvent {
    Capture(CaptureEvent),
    Net(NetEvent),
    Tick,
    Stop,
}

/// Where the connection stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkState {
    Waiting(Waiting),
    Connected { peer: String, addr: SocketAddr },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Report {
    Log(String),
    Link(LinkState),
    /// Keyboard and mouse now drive the other computer (true) or this one.
    Remote(bool),
    /// The cursor is near the edge to the other computer (or no longer is).
    Edge(Option<EdgeHint>),
    /// Pairing: show this PIN so the user can type it on the other computer.
    ShowPin(String),
    /// Pairing: the user must type the PIN shown on the other computer.
    AskPin(SocketAddr),
    /// The engine has ended, on request (`Ok`) or because of an error.
    Stopped(Result<(), String>),
}

pub type Reporter = Arc<dyn Fn(Report) + Send + Sync>;

/// A running engine. Dropping it stops it.
pub struct Engine {
    tx: Sender<AppEvent>,
    thread: Option<JoinHandle<()>>,
}

impl Engine {
    /// Starts the engine thread. Setup errors arrive as `Report::Stopped`.
    pub fn start(settings: Settings, pin: PinSource, report: Reporter) -> Engine {
        let (tx, rx) = mpsc::channel();
        let thread_tx = tx.clone();
        let thread = thread::Builder::new()
            .name("sc-engine".into())
            .spawn(move || {
                let result = run(settings, pin, &report, thread_tx, rx).map_err(|e| e.to_string());
                report(Report::Stopped(result));
            })
            .expect("spawn engine thread");
        Engine { tx, thread: Some(thread) }
    }

    /// Blocks until the engine has ended.
    pub fn wait(mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }

    /// Asks the engine to end and waits for it. A pending `AskPin` must be
    /// answered or cancelled first.
    pub fn stop(self) {
        let _ = self.tx.send(AppEvent::Stop);
        self.wait();
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = self.tx.send(AppEvent::Stop);
            let _ = thread.join();
        }
    }
}

fn run(
    settings: Settings,
    pin: PinSource,
    report: &Reporter,
    tx: Sender<AppEvent>,
    rx: mpsc::Receiver<AppEvent>,
) -> Result<(), Box<dyn Error>> {
    let log = |text: String| report(Report::Log(text));
    sys::init();
    let screens: Vec<_> = sys::monitors().into_iter().map(|m| m.bounds).collect();
    if screens.is_empty() {
        return Err("no monitors found".into());
    }

    let mut node = Node::new(
        Config {
            peer_side: settings.side,
            swap_ctrl_cmd: settings.swap_ctrl_cmd,
            speed: settings.speed,
            edge_delay: settings.edge_delay,
        },
        Os::current(),
        settings.name.clone(),
        screens.clone(),
    );

    let mut io = if settings.dry_run {
        Io { capture: None, injector: None }
    } else {
        Io { capture: Some(start_capture(tx.clone())?), injector: Some(Injector::new()) }
    };

    let data_dir = &settings.data_dir;
    let identity = Identity::load_or_create(data_dir)
        .map_err(|e| format!("identity in {}: {e}", data_dir.display()))?;
    let mut trust = TrustStore::load(data_dir)?;
    if settings.forget_peers {
        trust.clear()?;
    }
    log(format!(
        "{} ({:?}) screens {:?}; peer on the {:?}; {:?}{}",
        settings.name,
        Os::current(),
        screens,
        settings.side,
        settings.net,
        if settings.dry_run { "; dry run" } else { "" }
    ));
    log(format!(
        "certificate {}; {} paired computer(s); data in {}",
        identity.fingerprint,
        trust.len(),
        data_dir.display()
    ));

    let ctx =
        NetContext { tls: TlsConfig::new(&identity)?, trust: Arc::new(Mutex::new(trust)), pin };
    let net = net::spawn(settings.net.clone(), node.hello(), ctx, tx.clone())
        .map_err(|e| format!("{:?}: {e}", settings.net))?;
    spawn_ticker(tx);

    let mut link: Option<Link> = None;
    let mut secure_input = false;
    let mut out = Out { link: &mut link, io: &mut io, settings: &settings, report };
    let mut result = Ok(());
    for event in rx {
        let actions = match event {
            AppEvent::Stop => break,
            AppEvent::Capture(CaptureEvent::Input(ev)) => node.local_input(ev),
            AppEvent::Capture(CaptureEvent::EmergencyRelease) => node.emergency_release(),
            AppEvent::Net(NetEvent::Waiting(waiting)) => {
                report(Report::Link(LinkState::Waiting(waiting)));
                Vec::new()
            }
            AppEvent::Net(NetEvent::ShowPin(pin)) => {
                report(Report::ShowPin(pin));
                Vec::new()
            }
            AppEvent::Net(NetEvent::Connected { hello, link: new_link, addr, fingerprint }) => {
                if let Some(old) = out.link.replace(new_link) {
                    old.close();
                }
                log(format!("encrypted connection to {addr} (certificate {fingerprint})"));
                report(Report::Link(LinkState::Connected { peer: hello.name.clone(), addr }));
                node.peer_connected(hello)
            }
            AppEvent::Net(NetEvent::Log(text)) => vec![Action::Log(text)],
            AppEvent::Net(NetEvent::Fatal(reason)) => {
                result = Err(reason.into());
                break;
            }
            AppEvent::Net(NetEvent::Message(msg)) => {
                if settings.verbose && msg != Message::Ping {
                    log(format!("recv {msg:?}"));
                }
                node.peer_message(msg)
            }
            AppEvent::Net(NetEvent::Disconnected(reason)) => {
                *out.link = None;
                log(format!("connection lost: {reason}"));
                node.peer_disconnected()
            }
            AppEvent::Tick => {
                let mut actions = Vec::new();
                if out.link.is_some() {
                    actions.push(Action::Send(Message::Ping));
                }
                let now_secure = sys::secure_input_active();
                if now_secure != secure_input {
                    secure_input = now_secure;
                    actions.push(Action::Log(if now_secure {
                        "secure input is on (password field?): keystrokes cannot be captured".into()
                    } else {
                        "secure input is off".into()
                    }));
                }
                actions
            }
        };

        for action in actions {
            out.apply(action);
        }
    }

    // Give keyboard and mouse back and release anything held on the peer.
    for action in node.peer_disconnected() {
        out.apply(action);
    }
    if let Some(l) = link.take() {
        l.close();
    }
    net.stop();
    result
}

struct Io {
    capture: Option<Capture>,
    injector: Option<Injector>,
}

/// Carries out the node's actions.
struct Out<'a> {
    link: &'a mut Option<Link>,
    io: &'a mut Io,
    settings: &'a Settings,
    report: &'a Reporter,
}

impl Out<'_> {
    fn log(&self, text: String) {
        (self.report)(Report::Log(text));
    }

    fn apply(&mut self, action: Action) {
        match action {
            Action::Send(msg) => {
                if self.settings.verbose && msg != Message::Ping {
                    self.log(format!("send {msg:?}"));
                }
                if let Some(l) = self.link.as_ref()
                    && let Err(e) = l.send(&msg)
                {
                    // The reader thread notices too and reports the disconnect.
                    self.log(format!("send failed: {e}"));
                    l.close();
                }
            }
            Action::Inject(ev) => match self.io.injector.as_mut() {
                Some(injector) => {
                    if let Err(e) = injector.inject(&ev) {
                        self.log(format!("inject {ev:?} failed: {e}"));
                    }
                }
                None => {
                    if self.settings.verbose || !matches!(ev, InputEvent::MouseMove { .. }) {
                        self.log(format!("inject {ev:?}"));
                    }
                }
            },
            Action::Grab(on) => {
                match &self.io.capture {
                    Some(capture) => {
                        // Their key-ups would be swallowed, leaving them stuck down here.
                        let held = if on { sys::held_keys() } else { Vec::new() };
                        capture.set_grab(on);
                        if let Some(injector) = self.io.injector.as_mut() {
                            for usage in held {
                                let _ = injector.inject(&InputEvent::Key { usage, down: false });
                            }
                        }
                    }
                    None => self.log(format!("grab {on}")),
                }
                (self.report)(Report::Remote(on));
            }
            Action::Edge(hint) => (self.report)(Report::Edge(hint)),
            Action::Log(text) => self.log(text),
        }
    }
}

/// Starts capture and forwards its events into the main loop.
fn start_capture(tx: Sender<AppEvent>) -> Result<Capture, Box<dyn Error>> {
    let (capture, events) = Capture::start()?;
    thread::Builder::new().name("sc-capture-fwd".into()).spawn(move || {
        for ev in events {
            if tx.send(AppEvent::Capture(ev)).is_err() {
                return;
            }
        }
    })?;
    Ok(capture)
}

fn spawn_ticker(tx: Sender<AppEvent>) {
    thread::spawn(move || {
        while tx.send(AppEvent::Tick).is_ok() {
            thread::sleep(Duration::from_secs(1));
        }
    });
}
