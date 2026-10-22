//! PIN pairing.
//!
//! The listening side shows a fresh 6-digit PIN and the user types it on the
//! connecting side. Both run SPAKE2 with the PIN as password, so neither an
//! eavesdropper nor a machine in the middle learns anything that would let it
//! test PIN guesses offline: each connection attempt is exactly one guess.
//! Key confirmation is bound to the TLS session, and on success each side
//! records the other's certificate fingerprint.

use std::io::{self, Read, Write};

use ring::hmac;
use ring::rand::{SecureRandom, SystemRandom};
use sc_proto::{Auth, read_frame, write_frame};
use spake2::{Ed25519Group, Identity, Password, Spake2};

pub const PIN_DIGITS: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Accepted the connection; shows the PIN.
    Server,
    /// Opened the connection; the user enters the PIN.
    Client,
}

pub fn generate_pin() -> String {
    let mut bytes = [0u8; 4];
    SystemRandom::new().fill(&mut bytes).expect("system random number generator");
    format!("{:06}", u32::from_le_bytes(bytes) % 1_000_000)
}

/// Accepts "123456" or "123 456"; anything else is `None`.
pub fn normalize_pin(text: &str) -> Option<String> {
    let digits: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    (digits.len() == PIN_DIGITS && digits.chars().all(|c| c.is_ascii_digit())).then_some(digits)
}

/// Runs the PIN exchange. Fails with `PermissionDenied` if the PINs differ.
pub fn pair(
    reader: &mut impl Read,
    writer: &mut impl Write,
    role: Role,
    pin: &str,
    channel_binding: &[u8],
) -> io::Result<()> {
    let (spake, outbound) = Spake2::<Ed25519Group>::start_symmetric(
        &Password::new(pin.as_bytes()),
        &Identity::new(b"switchcontrol pairing v1"),
    );
    write_frame(writer, &Auth::Spake(outbound))?;
    let inbound = match read_frame(reader)? {
        Auth::Spake(msg) => msg,
        other => return Err(unexpected(other)),
    };
    let shared = spake.finish(&inbound).map_err(|_| denied("invalid pairing message"))?;

    // Distinct labels per role, so a side cannot be fooled by its own tag
    // reflected back at it.
    let key = hmac::Key::new(hmac::HMAC_SHA256, &shared);
    let (mine, theirs): (&[u8], &[u8]) = match role {
        Role::Server => (b"server", b"client"),
        Role::Client => (b"client", b"server"),
    };
    let tag = hmac::sign(&key, &[mine, channel_binding].concat());
    write_frame(writer, &Auth::Confirm(tag.as_ref().to_vec()))?;
    let their_tag = match read_frame(reader)? {
        Auth::Confirm(tag) => tag,
        other => return Err(unexpected(other)),
    };
    hmac::verify(&key, &[theirs, channel_binding].concat(), &their_tag)
        .map_err(|_| denied("wrong PIN"))
}

fn denied(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, msg)
}

fn unexpected(msg: Auth) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("unexpected pairing message {msg:?}"))
}

#[cfg(test)]
mod tests {
    use std::net::{TcpListener, TcpStream};
    use std::thread;

    use super::*;

    /// Runs both sides over loopback TCP and returns (server, client) results.
    fn run(server_pin: &str, client_pin: &str, bindings: (&[u8], &[u8])) -> (bool, bool) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (server_pin, server_binding) = (server_pin.to_string(), bindings.0.to_vec());
        let server = thread::spawn(move || {
            let (sock, _) = listener.accept().unwrap();
            pair(&mut &sock, &mut &sock, Role::Server, &server_pin, &server_binding).is_ok()
        });
        let sock = TcpStream::connect(addr).unwrap();
        let client = pair(&mut &sock, &mut &sock, Role::Client, client_pin, bindings.1).is_ok();
        (server.join().unwrap(), client)
    }

    #[test]
    fn matching_pins_pair() {
        assert_eq!(run("123456", "123456", (b"tls", b"tls")), (true, true));
    }

    #[test]
    fn different_pins_fail_on_both_sides() {
        assert_eq!(run("123456", "123457", (b"tls", b"tls")), (false, false));
    }

    #[test]
    fn different_tls_sessions_fail() {
        // What a relay between two separate TLS sessions would look like.
        assert_eq!(run("123456", "123456", (b"session A", b"session B")), (false, false));
    }

    #[test]
    fn pins_are_six_digits() {
        for _ in 0..50 {
            assert!(normalize_pin(&generate_pin()).is_some());
        }
        assert_eq!(normalize_pin(" 123 456 "), Some("123456".into()));
        assert_eq!(normalize_pin("12345"), None);
        assert_eq!(normalize_pin("12345a"), None);
    }
}
