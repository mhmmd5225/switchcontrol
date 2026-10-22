//! SwitchControl: share one keyboard and mouse between two computers.
//!
//! Run it on both machines; each one says where the other sits. One side
//! listens and the other connects. Moving the cursor off the shared edge
//! hands keyboard and mouse to the other machine.

mod config;
mod identity;
mod net;
mod pairing;
mod tls;

use std::error::Error;
use std::process::ExitCode;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use sc_core::{Action, Config, Node};
use sc_platform::sys::{self, Capture, Injector};
use sc_platform::{CaptureEvent, InputEvent};
use sc_proto::{Message, Os};

use config::Settings;
use identity::{Identity, TrustStore};
use net::{Link, NetContext, NetEvent};
use tls::TlsConfig;

pub enum AppEvent {
    Capture(CaptureEvent),
    Net(NetEvent),
    Tick,
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let settings = match config::from_args(&args) {
        Ok(settings) => settings,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("error: {e}\n");
            }
            eprintln!("{}", config::USAGE);
            return ExitCode::from(2);
        }
    };
    match run(settings) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(settings: Settings) -> Result<(), Box<dyn Error>> {
    sys::init();
    let log = Logger { start: Instant::now() };
    let screens: Vec<_> = sys::monitors().into_iter().map(|m| m.bounds).collect();
    if screens.is_empty() {
        return Err("no monitors found".into());
    }

    let mut node = Node::new(
        Config {
            peer_side: settings.side,
            swap_ctrl_cmd: settings.swap_ctrl_cmd,
            speed: settings.speed,
        },
        Os::current(),
        settings.name.clone(),
        screens.clone(),
    );
    let (tx, rx) = mpsc::channel();

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
    log.line(format!(
        "{} ({:?}) screens {:?}; peer on the {:?}; {:?}{}",
        settings.name,
        Os::current(),
        screens,
        settings.side,
        settings.net,
        if settings.dry_run { "; dry run" } else { "" }
    ));
    log.line(format!(
        "certificate {}; {} paired computer(s); data in {}",
        identity.fingerprint,
        trust.len(),
        data_dir.display()
    ));

    let ctx = NetContext {
        tls: TlsConfig::new(&identity)?,
        trust: Arc::new(Mutex::new(trust)),
        pin: settings.pin.clone(),
    };
    net::spawn(settings.net.clone(), node.hello(), ctx, tx.clone())?;
    spawn_ticker(tx);

    let mut link: Option<Link> = None;
    let mut secure_input = false;
    for event in rx {
        let actions = match event {
            AppEvent::Capture(CaptureEvent::Input(ev)) => node.local_input(ev),
            AppEvent::Capture(CaptureEvent::EmergencyRelease) => node.emergency_release(),
            AppEvent::Net(NetEvent::Connected { hello, link: new_link, addr, fingerprint }) => {
                if let Some(old) = link.replace(new_link) {
                    old.close();
                }
                log.line(format!("encrypted connection to {addr} (certificate {fingerprint})"));
                node.peer_connected(hello)
            }
            AppEvent::Net(NetEvent::Log(text)) => vec![Action::Log(text)],
            AppEvent::Net(NetEvent::Fatal(reason)) => {
                for action in node.peer_disconnected() {
                    apply(action, &mut link, &mut io, &settings, &log);
                }
                return Err(reason.into());
            }
            AppEvent::Net(NetEvent::Message(msg)) => {
                if settings.verbose && msg != Message::Ping {
                    log.line(format!("recv {msg:?}"));
                }
                node.peer_message(msg)
            }
            AppEvent::Net(NetEvent::Disconnected(reason)) => {
                link = None;
                log.line(format!("connection lost: {reason}"));
                node.peer_disconnected()
            }
            AppEvent::Tick => {
                let mut actions = Vec::new();
                if link.is_some() {
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
            apply(action, &mut link, &mut io, &settings, &log);
        }
    }
    Ok(())
}

struct Io {
    capture: Option<Capture>,
    injector: Option<Injector>,
}

fn apply(action: Action, link: &mut Option<Link>, io: &mut Io, settings: &Settings, log: &Logger) {
    match action {
        Action::Send(msg) => {
            if settings.verbose && msg != Message::Ping {
                log.line(format!("send {msg:?}"));
            }
            if let Some(l) = link.as_ref()
                && let Err(e) = l.send(&msg)
            {
                // The reader thread notices too and reports the disconnect.
                log.line(format!("send failed: {e}"));
                l.close();
            }
        }
        Action::Inject(ev) => match io.injector.as_mut() {
            Some(injector) => {
                if let Err(e) = injector.inject(&ev) {
                    log.line(format!("inject {ev:?} failed: {e}"));
                }
            }
            None => {
                if settings.verbose || !matches!(ev, InputEvent::MouseMove { .. }) {
                    log.line(format!("inject {ev:?}"));
                }
            }
        },
        Action::Grab(on) => match &io.capture {
            Some(capture) => capture.set_grab(on),
            None => log.line(format!("grab {on}")),
        },
        Action::Log(text) => log.line(text),
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

struct Logger {
    start: Instant,
}

impl Logger {
    fn line(&self, text: impl AsRef<str>) {
        println!("[{:>8.3}] {}", self.start.elapsed().as_secs_f64(), text.as_ref());
    }
}
