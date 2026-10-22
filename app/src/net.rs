//! One encrypted connection to the peer, re-established whenever it drops.
//!
//! A background thread accepts or connects, runs TLS, checks that both sides
//! trust each other (pairing with a PIN if not), exchanges `Hello`s and then
//! forwards every incoming message. Both sides send `Ping` every second; a
//! peer that stays silent for [`PEER_TIMEOUT`] is considered gone.

use std::io::{self, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use sc_proto::{Auth, Hello, Message, PROTOCOL_VERSION, read_frame, write_frame};

use crate::config::NetMode;
use crate::engine::AppEvent;
use crate::identity::{Fingerprint, TrustStore};
use crate::pairing::{self, Role};
use crate::tls::{Secure, SecureReader, TlsConfig};

const PEER_TIMEOUT: Duration = Duration::from_secs(5);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
/// Time the user has to read the PIN on one screen and type it on the other.
pub const PAIRING_TIMEOUT: Duration = Duration::from_secs(120);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const RETRY_DELAY: Duration = Duration::from_secs(2);
/// Wrong PINs accepted before the listener stops offering pairing.
const MAX_PAIRING_FAILURES: u32 = 5;
/// How often an idle listener checks whether it should stop.
const POLL: Duration = Duration::from_millis(100);

/// What the connection thread is doing between sessions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Waiting {
    Listening(String),
    Connecting(String),
}

pub enum NetEvent {
    Waiting(Waiting),
    /// Pairing request: the user must type this PIN on the other computer.
    ShowPin(String),
    Connected {
        hello: Hello,
        link: Link,
        addr: SocketAddr,
        fingerprint: Fingerprint,
    },
    Message(Message),
    Disconnected(String),
    Log(String),
    /// Unrecoverable (e.g. wrong PIN typed on this side); the app should exit.
    Fatal(String),
}

pub struct NetContext {
    pub tls: TlsConfig,
    pub trust: Arc<Mutex<TrustStore>>,
    pub pin: PinSource,
}

/// Where the connecting side gets the PIN when pairing.
pub enum PinSource {
    /// Given up front, e.g. with `--pin`.
    Fixed(String),
    /// Typed into the terminal.
    Console,
    /// Asked through a callback (the window); `None` means cancelled.
    Ask(Box<dyn Fn(SocketAddr) -> Option<String> + Send>),
}

/// Write half of the current connection.
pub struct Link {
    secure: Arc<Secure>,
}

impl Link {
    pub fn send(&self, msg: &Message) -> io::Result<()> {
        write_frame(&mut &*self.secure, msg)
    }

    /// Ends the connection; the reader thread then reports `Disconnected`.
    pub fn close(&self) {
        self.secure.shutdown();
    }
}

/// Stops the connection thread and frees the port.
pub struct NetHandle {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

struct Shared {
    stop: AtomicBool,
    /// Socket of the session in progress, so `stop` can interrupt it.
    active: Mutex<Option<TcpStream>>,
}

impl Shared {
    fn stopping(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    /// Sleeps up to `time`; false if asked to stop meanwhile.
    fn pause(&self, time: Duration) -> bool {
        let end = Instant::now() + time;
        while Instant::now() < end {
            if self.stopping() {
                return false;
            }
            thread::sleep(POLL.min(end - Instant::now()));
        }
        !self.stopping()
    }
}

impl NetHandle {
    /// Ends the current session and the thread. A pending PIN prompt must be
    /// answered or cancelled first.
    pub fn stop(mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        if let Some(stream) = self.shared.active.lock().unwrap().as_ref() {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Starts the connection thread. Binding happens here so a busy port is
/// reported right away.
pub fn spawn(
    mode: NetMode,
    hello: Hello,
    ctx: NetContext,
    tx: Sender<AppEvent>,
) -> io::Result<NetHandle> {
    let listener = match &mode {
        NetMode::Listen(addr) => {
            let listener = TcpListener::bind(addr)?;
            listener.set_nonblocking(true)?;
            Some(listener)
        }
        NetMode::Connect(_) => None,
    };
    let shared = Arc::new(Shared { stop: AtomicBool::new(false), active: Mutex::new(None) });
    let thread_shared = shared.clone();
    let thread = thread::Builder::new().name("sc-net".into()).spawn(move || {
        let shared = thread_shared;
        let mut pairing_failures = 0;
        let announce = || {
            let waiting = match &mode {
                NetMode::Listen(addr) => Waiting::Listening(addr.clone()),
                NetMode::Connect(addr) => Waiting::Connecting(addr.clone()),
            };
            let _ = tx.send(AppEvent::Net(NetEvent::Waiting(waiting)));
        };
        announce();
        loop {
            if shared.stopping() {
                return;
            }
            let (stream, role) = match (&mode, &listener) {
                (NetMode::Listen(_), Some(listener)) => match listener.accept() {
                    Ok((stream, _)) => (stream, Role::Server),
                    Err(e) => {
                        let delay =
                            if e.kind() == io::ErrorKind::WouldBlock { POLL } else { RETRY_DELAY };
                        if !shared.pause(delay) {
                            return;
                        }
                        continue;
                    }
                },
                (NetMode::Connect(addr), _) => match connect(addr) {
                    Ok(stream) => (stream, Role::Client),
                    Err(_) => {
                        if !shared.pause(RETRY_DELAY) {
                            return;
                        }
                        continue;
                    }
                },
                _ => unreachable!("listen mode always has a listener"),
            };
            // macOS hands out accepted sockets in the listener's non-blocking mode.
            if stream.set_nonblocking(false).is_err() {
                continue;
            }
            *shared.active.lock().unwrap() = stream.try_clone().ok();
            if shared.stopping() {
                return;
            }

            let pairing_allowed = pairing_failures < MAX_PAIRING_FAILURES;
            let result = session(stream, role, &hello, &ctx, &tx, pairing_allowed);
            *shared.active.lock().unwrap() = None;
            if shared.stopping() {
                return;
            }
            let event = match result {
                Ok(()) => return, // the app has quit
                Err(e) if e.kind() == io::ErrorKind::PermissionDenied => match role {
                    // The PIN typed here was wrong; retrying would just ask again.
                    Role::Client => NetEvent::Fatal(format!("pairing failed: {e}")),
                    Role::Server => {
                        pairing_failures += 1;
                        // Slows down anyone guessing PINs.
                        thread::sleep(RETRY_DELAY);
                        NetEvent::Log(format!(
                            "pairing failed: {e} ({pairing_failures}/{MAX_PAIRING_FAILURES})"
                        ))
                    }
                },
                Err(e) => NetEvent::Disconnected(describe(&e)),
            };
            let fatal = matches!(event, NetEvent::Fatal(_));
            if tx.send(AppEvent::Net(event)).is_err() || fatal {
                return;
            }
            announce();
            if role == Role::Client && !shared.pause(RETRY_DELAY) {
                return;
            }
        }
    })?;
    Ok(NetHandle { shared, thread: Some(thread) })
}

fn connect(addr: &str) -> io::Result<TcpStream> {
    let mut last = io::Error::new(io::ErrorKind::NotFound, format!("cannot resolve {addr}"));
    for resolved in addr.to_socket_addrs()? {
        match TcpStream::connect_timeout(&resolved, CONNECT_TIMEOUT) {
            Ok(stream) => return Ok(stream),
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// Runs one connection until it fails. `Ok` means the app stopped listening.
fn session(
    stream: TcpStream,
    role: Role,
    hello: &Hello,
    ctx: &NetContext,
    tx: &Sender<AppEvent>,
    pairing_allowed: bool,
) -> io::Result<()> {
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    let addr = stream.peer_addr()?;
    let secure = Arc::new(match role {
        Role::Server => ctx.tls.accept(stream)?,
        Role::Client => ctx.tls.connect(stream)?,
    });
    let mut reader = SecureReader::new(secure.clone());
    let mut writer = &*secure;
    let fingerprint = secure.peer_fingerprint()?;

    let trusted = ctx.trust.lock().unwrap().contains(&fingerprint);
    write_frame(&mut writer, &Auth::Status { trusted })?;
    let peer_trusts = match read_frame(&mut reader)? {
        Auth::Status { trusted } => trusted,
        other => return Err(invalid(format!("expected trust status, got {other:?}"))),
    };
    let pairing = !(trusted && peer_trusts);
    if pairing {
        if !pairing_allowed {
            return Err(io::Error::other(
                "too many wrong PINs; restart switchcontrol to pair again",
            ));
        }
        secure.socket().set_read_timeout(Some(PAIRING_TIMEOUT))?;
        let pin = match role {
            Role::Server => {
                let pin = pairing::generate_pin();
                let _ = tx.send(AppEvent::Net(NetEvent::Log(format!(
                    "pairing request from {addr} (certificate {fingerprint}). \
                     Enter this PIN on the other computer: {} {}",
                    &pin[..3],
                    &pin[3..]
                ))));
                let _ = tx.send(AppEvent::Net(NetEvent::ShowPin(pin.clone())));
                pin
            }
            Role::Client => match &ctx.pin {
                PinSource::Fixed(pin) => pin.clone(),
                PinSource::Console => prompt_pin(addr)?,
                PinSource::Ask(ask) => ask(addr).ok_or_else(|| {
                    io::Error::new(io::ErrorKind::PermissionDenied, "pairing cancelled")
                })?,
            },
        };
        pairing::pair(&mut reader, &mut writer, role, &pin, &secure.channel_binding()?)?;
        secure.socket().set_read_timeout(Some(HANDSHAKE_TIMEOUT))?;
    }

    write_frame(&mut writer, &Message::Hello(hello.clone()))?;
    let peer = match read_frame(&mut reader)? {
        Message::Hello(peer) => peer,
        other => return Err(invalid(format!("expected Hello, got {other:?}"))),
    };
    if peer.version != PROTOCOL_VERSION {
        return Err(invalid(format!(
            "peer speaks protocol {}, we speak {PROTOCOL_VERSION}",
            peer.version
        )));
    }
    if pairing {
        ctx.trust.lock().unwrap().add(fingerprint, &peer.name)?;
        let _ = tx.send(AppEvent::Net(NetEvent::Log(format!("paired with {}", peer.name))));
    }

    secure.socket().set_read_timeout(Some(PEER_TIMEOUT))?;
    let link = Link { secure: secure.clone() };
    let connected = NetEvent::Connected { hello: peer, link, addr, fingerprint };
    if tx.send(AppEvent::Net(connected)).is_err() {
        return Ok(());
    }
    loop {
        let msg = read_frame(&mut reader)?;
        if tx.send(AppEvent::Net(NetEvent::Message(msg))).is_err() {
            return Ok(());
        }
    }
}

fn prompt_pin(addr: SocketAddr) -> io::Result<String> {
    let denied = |msg: &str| io::Error::new(io::ErrorKind::PermissionDenied, msg.to_string());
    print!("Pairing with {addr}: enter the PIN shown on the other computer: ");
    io::stdout().flush()?;
    let mut line = String::new();
    if io::stdin().read_line(&mut line)? == 0 {
        return Err(denied("no PIN entered (use --pin when there is no console)"));
    }
    pairing::normalize_pin(&line).ok_or_else(|| denied("a PIN is 6 digits"))
}

fn invalid(msg: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

fn describe(e: &io::Error) -> String {
    match e.kind() {
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => "peer stopped responding".into(),
        io::ErrorKind::UnexpectedEof => "peer closed the connection".into(),
        _ => e.to_string(),
    }
}
