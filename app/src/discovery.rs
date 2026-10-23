//! Finding SwitchControl on the local network without typing addresses.
//!
//! A listening computer broadcasts a small UDP beacon every
//! [`ANNOUNCE_EVERY`] (name, OS, TCP port, certificate fingerprint). The
//! connecting side listens for beacons: the window lists what it finds, and
//! the connection thread uses them to reach a paired computer whose address
//! changed. A beacon is only a hint: who is on the other end is still proven
//! by TLS and the list of paired computers.

use std::collections::HashMap;
use std::io;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use sc_proto::{DEFAULT_PORT, Os, PROTOCOL_VERSION};
use serde::{Deserialize, Serialize};

use crate::identity::Fingerprint;

const MAGIC: [u8; 4] = *b"SWCD";
pub const ANNOUNCE_EVERY: Duration = Duration::from_secs(2);
/// A computer not heard from for this long is dropped from the list.
const FORGET_AFTER: Duration = Duration::from_secs(7);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Beacon {
    magic: [u8; 4],
    version: u16,
    pub name: String,
    pub os: Os,
    /// TCP port the sender listens on.
    pub port: u16,
    pub fingerprint: [u8; 32],
}

impl Beacon {
    pub fn new(name: &str, port: u16, fingerprint: Fingerprint) -> Beacon {
        Beacon {
            magic: MAGIC,
            version: PROTOCOL_VERSION,
            name: name.chars().take(64).collect(),
            os: Os::current(),
            port,
            fingerprint: fingerprint.bytes(),
        }
    }

    fn encode(&self) -> Vec<u8> {
        postcard::to_stdvec(self).expect("a beacon always encodes")
    }

    /// `None` for anything that is not a beacon of this protocol version.
    fn decode(bytes: &[u8]) -> Option<Beacon> {
        let beacon: Beacon = postcard::from_bytes(bytes).ok()?;
        (beacon.magic == MAGIC && beacon.version == PROTOCOL_VERSION).then_some(beacon)
    }
}

/// A computer heard on the network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub name: String,
    pub os: Os,
    pub addr: SocketAddr,
    pub fingerprint: Fingerprint,
    seen: Instant,
}

type Waker = Box<dyn Fn() + Send>;

/// Collects beacons. One per process, see [`Listener::global`].
pub struct Listener {
    found: Mutex<HashMap<[u8; 32], Found>>,
    waker: Mutex<Option<Waker>>,
}

impl Listener {
    /// The process-wide listener on [`DEFAULT_PORT`], started on first use.
    /// If the port is taken the list just stays empty.
    pub fn global() -> &'static Arc<Listener> {
        static GLOBAL: OnceLock<Arc<Listener>> = OnceLock::new();
        GLOBAL.get_or_init(|| {
            let bind = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, DEFAULT_PORT).into();
            Listener::start(bind).unwrap_or_else(|_| Arc::new(Listener::empty()))
        })
    }

    fn empty() -> Listener {
        Listener { found: Mutex::new(HashMap::new()), waker: Mutex::new(None) }
    }

    fn start(bind: SocketAddr) -> io::Result<Arc<Listener>> {
        let socket = UdpSocket::bind(bind)?;
        let listener = Arc::new(Listener::empty());
        let shared = listener.clone();
        thread::Builder::new().name("sc-discovery".into()).spawn(move || {
            let mut buf = [0u8; 512];
            loop {
                let Ok((len, from)) = socket.recv_from(&mut buf) else { continue };
                if let Some(beacon) = Beacon::decode(&buf[..len]) {
                    shared.heard(beacon, from);
                }
            }
        })?;
        Ok(listener)
    }

    fn heard(&self, beacon: Beacon, from: SocketAddr) {
        let found = Found {
            name: beacon.name,
            os: beacon.os,
            addr: SocketAddr::new(from.ip(), beacon.port),
            fingerprint: Fingerprint::from_bytes(beacon.fingerprint),
            seen: Instant::now(),
        };
        let changed = {
            let mut map = self.found.lock().unwrap();
            let changed = map.get(&beacon.fingerprint).is_none_or(|old| {
                old.addr != found.addr
                    || old.name != found.name
                    || old.seen.elapsed() > FORGET_AFTER
            });
            map.insert(beacon.fingerprint, found);
            changed
        };
        if changed && let Some(wake) = self.waker.lock().unwrap().as_ref() {
            wake();
        }
    }

    /// Computers heard recently, by name.
    pub fn peers(&self) -> Vec<Found> {
        let mut peers: Vec<Found> = self
            .found
            .lock()
            .unwrap()
            .values()
            .filter(|f| f.seen.elapsed() < FORGET_AFTER)
            .cloned()
            .collect();
        peers.sort_by(|a, b| a.name.cmp(&b.name).then(a.addr.cmp(&b.addr)));
        peers
    }

    /// Called whenever a computer appears or changes address.
    pub fn set_waker(&self, wake: impl Fn() + Send + 'static) {
        *self.waker.lock().unwrap() = Some(Box::new(wake));
    }
}

/// Broadcasts this computer's beacon until dropped.
pub struct Announcer {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Announcer {
    /// Announces on the local network.
    pub fn start(beacon: Beacon) -> io::Result<Announcer> {
        Announcer::start_to(beacon, SocketAddrV4::new(Ipv4Addr::BROADCAST, DEFAULT_PORT).into())
    }

    fn start_to(beacon: Beacon, target: SocketAddr) -> io::Result<Announcer> {
        let socket = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))?;
        socket.set_broadcast(true)?;
        let packet = beacon.encode();
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let thread = thread::Builder::new().name("sc-announce".into()).spawn(move || {
            while !thread_stop.load(Ordering::SeqCst) {
                // No network yet (e.g. right after login) is not an error worth reporting.
                let _ = socket.send_to(&packet, target);
                let end = Instant::now() + ANNOUNCE_EVERY;
                while Instant::now() < end && !thread_stop.load(Ordering::SeqCst) {
                    thread::sleep(Duration::from_millis(100));
                }
            }
        })?;
        Ok(Announcer { stop, thread: Some(thread) })
    }
}

impl Drop for Announcer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fingerprint(byte: u8) -> Fingerprint {
        Fingerprint::from_bytes([byte; 32])
    }

    #[test]
    fn beacons_round_trip_and_reject_noise() {
        let beacon = Beacon::new("Masaüstü", 24800, fingerprint(7));
        assert_eq!(Beacon::decode(&beacon.encode()), Some(beacon.clone()));
        assert_eq!(Beacon::decode(b"hello"), None);
        let mut other = beacon;
        other.version = PROTOCOL_VERSION + 1;
        assert_eq!(Beacon::decode(&other.encode()), None);
    }

    #[test]
    fn announced_computers_are_found() {
        let port = UdpSocket::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let listener = Listener::start(SocketAddr::from(([127, 0, 0, 1], port))).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        listener.set_waker(move || {
            let _ = tx.send(());
        });

        let _announcer = Announcer::start_to(
            Beacon::new("PC", 24811, fingerprint(9)),
            SocketAddr::from(([127, 0, 0, 1], port)),
        )
        .unwrap();
        rx.recv_timeout(Duration::from_secs(5)).expect("beacon heard");
        let peers = listener.peers();
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].name, "PC");
        assert_eq!(peers[0].addr, SocketAddr::from(([127, 0, 0, 1], 24811)));
        assert_eq!(peers[0].fingerprint, fingerprint(9));
    }
}
