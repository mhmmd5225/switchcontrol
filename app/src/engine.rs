//! The running app: capture, the connection and the hand-over logic, on a
//! thread of its own so that a window (or the terminal) can drive it.
//!
//! Everything the user should see comes out through a [`Reporter`].

use std::error::Error;
use std::net::SocketAddr;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use sc_core::{Action, Config, EdgeHint, Node};
use sc_platform::sys::{self, Capture, Injector};
use sc_platform::{CaptureEvent, InputEvent};
use sc_proto::{DEFAULT_PORT, Message, Os, Power};

use crate::clipboard;
use crate::config::{NetMode, Settings};
use crate::discovery::{Announcer, Beacon};
use crate::identity::{Identity, TrustStore};
use crate::keyguard::KeyGuard;
use crate::net::{self, Link, NetContext, NetEvent, PinSource, Waiting};
use crate::power;
use crate::tls::TlsConfig;
use crate::wake;

pub enum AppEvent {
    Capture(CaptureEvent),
    Net(NetEvent),
    Tick,
    /// Ask the peer to sleep or shut down.
    SendPower(Power),
    Stop,
}

/// Where the connection stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkState {
    Waiting(Waiting),
    Connected {
        peer: String,
        addr: SocketAddr,
        os: Os,
        /// The peer's network card, for waking it later.
        mac: Option<[u8; 6]>,
    },
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

    /// Asks the peer to sleep or shut down.
    pub fn send_power(&self, power: Power) {
        let _ = self.tx.send(AppEvent::SendPower(power));
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
            scroll_speed: settings.scroll_speed,
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

    // Lets the other computer find this one; stops with the engine.
    let _announcer = match &settings.net {
        NetMode::Listen(addr) if settings.announce => {
            let port = addr.rsplit(':').next().and_then(|p| p.parse().ok()).unwrap_or(DEFAULT_PORT);
            let beacon = Beacon::new(&settings.name, port, identity.fingerprint);
            Announcer::start(beacon)
                .map_err(|e| log(format!("cannot announce on the network: {e}")))
                .ok()
        }
        _ => None,
    };

    let ctx =
        NetContext { tls: TlsConfig::new(&identity)?, trust: Arc::new(Mutex::new(trust)), pin };
    let net = net::spawn(settings.net.clone(), node.hello(), ctx, tx.clone())
        .map_err(|e| format!("{:?}: {e}", settings.net))?;
    spawn_ticker(tx);

    let mut link: Option<Link> = None;
    let mut secure_input = false;
    let mut out = Out {
        link: &mut link,
        io: &mut io,
        settings: &settings,
        report,
        keys: KeyGuard::default(),
        clipboard: clipboard::Share::new(clipboard::System::default(), settings.clipboard),
    };
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
                if let Some(mac) = hello.mac {
                    log(format!("{} can be woken at {}", hello.name, wake::format_mac(mac)));
                }
                report(Report::Link(LinkState::Connected {
                    peer: hello.name.clone(),
                    addr,
                    os: hello.os,
                    mac: hello.mac,
                }));
                node.peer_connected(hello)
            }
            AppEvent::Net(NetEvent::Log(text)) => vec![Action::Log(text)],
            AppEvent::SendPower(power) => match out.link {
                Some(_) => vec![
                    Action::Log(format!("asking the other computer to {}", verb(power))),
                    Action::Send(Message::Power(power)),
                ],
                None => vec![Action::Log("not connected to the other computer".into())],
            },
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
                out.recheck_keys();
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
    keys: KeyGuard,
    clipboard: clipboard::Share<clipboard::System>,
}

impl Out<'_> {
    fn log(&self, text: String) {
        (self.report)(Report::Log(text));
    }

    /// Releases again any key we released that the OS still holds down.
    fn recheck_keys(&mut self) {
        if self.keys.is_idle() {
            return;
        }
        let down = sys::logical_keys();
        for usage in self.keys.check(&down, Instant::now()) {
            if let Some(injector) = self.io.injector.as_mut() {
                let _ = injector.inject(&InputEvent::Key { usage, down: false });
            }
            self.log(format!("key {usage:#04x} was still down; released it again"));
        }
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
                    if let InputEvent::Key { usage, down } = ev {
                        self.keys.injected(usage, down, Instant::now());
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
            Action::ShareClipboard => match self.clipboard.outgoing() {
                Ok(msg) => {
                    if let Message::Clipboard { text } = &msg {
                        self.log(format!("clipboard sent ({} characters)", text.chars().count()));
                    }
                    self.apply(Action::Send(msg));
                }
                Err(None) => {}
                Err(Some(why)) => self.log(why),
            },
            Action::SetClipboard(text) => {
                if let Some(line) = self.clipboard.incoming(text) {
                    self.log(line);
                }
            }
            Action::Power(power) => {
                self.log(format!("the other computer asked this one to {}", verb(power)));
                if self.io.injector.is_none() {
                    return; // dry run
                }
                // Shutting down can wait on apps; the loop keeps going meanwhile.
                let report = self.report.clone();
                thread::spawn(move || {
                    if let Err(e) = power::run(power) {
                        report(Report::Log(e));
                    }
                });
            }
            Action::Log(text) => self.log(text),
        }
    }
}

fn verb(power: Power) -> &'static str {
    match power {
        Power::Sleep => "sleep",
        Power::Shutdown => "shut down",
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

#[cfg(test)]
mod tests {
    use std::net::TcpListener;
    use std::path::Path;
    use std::sync::mpsc::Receiver;

    use super::*;
    use crate::config::{self, FileConfig, RunFlags};

    fn settings(name: &str, side: &str, net: NetMode, dir: &Path) -> Settings {
        let (listen, connect) = match net {
            NetMode::Listen(addr) => (Some(addr), None),
            NetMode::Connect(addr) => (None, Some(addr)),
        };
        config::resolve(
            FileConfig {
                name: Some(name.into()),
                side: Some(side.into()),
                listen,
                connect,
                announce: Some(false),
                data_dir: Some(dir.to_path_buf()),
                ..FileConfig::default()
            },
            RunFlags { dry_run: true, ..RunFlags::default() },
        )
        .unwrap()
    }

    fn wait_for<T>(rx: &Receiver<Report>, what: &str, pick: impl Fn(Report) -> Option<T>) -> T {
        loop {
            match rx.recv_timeout(Duration::from_secs(10)) {
                Ok(report) => {
                    if let Some(found) = pick(report) {
                        return found;
                    }
                }
                Err(_) => panic!("timed out waiting for {what}"),
            }
        }
    }

    /// One computer asks the other to sleep; the other carries it out (in a
    /// dry run, only logs it).
    #[test]
    fn power_request_reaches_the_other_computer() {
        let tmp = std::env::temp_dir().join(format!("sc-engine-power-{}", std::process::id()));
        let (mac_dir, pc_dir) = (tmp.join("mac"), tmp.join("pc"));
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let addr = format!("127.0.0.1:{port}");

        let (mac_tx, mac_rx) = mpsc::channel();
        let mac = Engine::start(
            settings("Mac", "left", NetMode::Listen(addr.clone()), &mac_dir),
            PinSource::Console,
            Arc::new(move |r| drop(mac_tx.send(r))),
        );
        // The PIN the Mac shows goes to the PC, as the user would type it.
        let (pin_tx, pin_rx) = mpsc::channel::<String>();
        let (pc_tx, pc_rx) = mpsc::channel();
        let pc = Engine::start(
            settings("PC", "right", NetMode::Connect(addr), &pc_dir),
            PinSource::Ask(Box::new(move |_| pin_rx.recv_timeout(Duration::from_secs(10)).ok())),
            Arc::new(move |r| drop(pc_tx.send(r))),
        );
        let pin = wait_for(&mac_rx, "the PIN", |r| match r {
            Report::ShowPin(pin) => Some(pin),
            _ => None,
        });
        pin_tx.send(pin).unwrap();
        let os = wait_for(&pc_rx, "the connection", |r| match r {
            Report::Link(LinkState::Connected { os, .. }) => Some(os),
            _ => None,
        });
        assert_eq!(os, Os::current());

        pc.send_power(Power::Sleep);
        wait_for(&mac_rx, "the request", |r| match r {
            Report::Log(line) if line == "the other computer asked this one to sleep" => Some(()),
            _ => None,
        });

        pc.stop();
        mac.stop();
        let _ = std::fs::remove_dir_all(tmp);
    }
}
