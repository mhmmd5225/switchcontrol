//! Mutually authenticated TLS 1.3 over a blocking `TcpStream`.
//!
//! Both sides present their self-signed certificate. The verifiers accept any
//! certificate but still check the handshake signature, so each side knows
//! the peer holds the key for the certificate it showed; whether to trust
//! that certificate is decided afterwards by fingerprint (see `pairing`).
//!
//! After the handshake one thread reads while another writes. The rustls
//! connection sits behind a mutex that is held only while (de)crypting,
//! never while blocked on the socket.

use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::{Arc, Mutex};

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::{
    ClientConfig, ClientConnection, Connection, DigitallySignedStruct, DistinguishedName,
    ServerConfig, ServerConnection, SignatureScheme,
};

use crate::identity::{Fingerprint, Identity};

/// TLS configuration for both roles, built once from our identity.
pub struct TlsConfig {
    client: Arc<ClientConfig>,
    server: Arc<ServerConfig>,
}

impl TlsConfig {
    pub fn new(identity: &Identity) -> io::Result<TlsConfig> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let verifier = Arc::new(AnyCertificate(provider.clone()));
        let chain = vec![identity.cert.clone()];

        let client = ClientConfig::builder_with_provider(provider.clone())
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(io::Error::other)?
            .dangerous()
            .with_custom_certificate_verifier(verifier.clone())
            .with_client_auth_cert(chain.clone(), identity.key.clone_key())
            .map_err(io::Error::other)?;
        let server = ServerConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(io::Error::other)?
            .with_client_cert_verifier(verifier)
            .with_single_cert(chain, identity.key.clone_key())
            .map_err(io::Error::other)?;
        Ok(TlsConfig { client: Arc::new(client), server: Arc::new(server) })
    }

    pub fn connect(&self, sock: TcpStream) -> io::Result<Secure> {
        let name = ServerName::try_from("switchcontrol").expect("valid server name");
        let conn = ClientConnection::new(self.client.clone(), name).map_err(io::Error::other)?;
        Secure::handshake(sock, conn.into())
    }

    pub fn accept(&self, sock: TcpStream) -> io::Result<Secure> {
        let conn = ServerConnection::new(self.server.clone()).map_err(io::Error::other)?;
        Secure::handshake(sock, conn.into())
    }
}

/// An established TLS session.
pub struct Secure {
    conn: Mutex<Connection>,
    sock: TcpStream,
}

impl Secure {
    fn handshake(sock: TcpStream, mut conn: Connection) -> io::Result<Secure> {
        while conn.is_handshaking() {
            conn.complete_io(&mut &sock)?;
        }
        while conn.wants_write() {
            conn.write_tls(&mut &sock)?;
        }
        Ok(Secure { conn: Mutex::new(conn), sock })
    }

    pub fn socket(&self) -> &TcpStream {
        &self.sock
    }

    pub fn peer_fingerprint(&self) -> io::Result<Fingerprint> {
        let conn = self.conn.lock().unwrap();
        let cert: Option<&CertificateDer<'_>> = conn.peer_certificates().and_then(|c| c.first());
        cert.map(Fingerprint::of).ok_or_else(|| {
            io::Error::new(io::ErrorKind::PermissionDenied, "peer sent no certificate")
        })
    }

    /// A value unique to this TLS session, identical on both ends. Pairing
    /// binds to it so a PIN exchange cannot be relayed into another session.
    pub fn channel_binding(&self) -> io::Result<[u8; 32]> {
        let conn = self.conn.lock().unwrap();
        conn.export_keying_material([0u8; 32], b"EXPORTER-switchcontrol-pairing", None)
            .map_err(io::Error::other)
    }

    pub fn shutdown(&self) {
        let _ = self.sock.shutdown(Shutdown::Both);
    }

    fn send(&self, data: &[u8]) -> io::Result<()> {
        let mut conn = self.conn.lock().unwrap();
        conn.writer().write_all(data)?;
        while conn.wants_write() {
            conn.write_tls(&mut &self.sock)?;
        }
        Ok(())
    }

    /// Blocks until some plaintext arrives and appends it to `out`.
    fn receive(&self, out: &mut Vec<u8>) -> io::Result<()> {
        let mut raw = [0u8; 16 * 1024];
        loop {
            // Plaintext may already be decrypted, e.g. when the peer's first
            // message arrived together with the end of the handshake.
            if take_plaintext(&mut self.conn.lock().unwrap(), out)? {
                return Ok(());
            }
            let n = (&self.sock).read(&mut raw)?;
            if n == 0 {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            let mut conn = self.conn.lock().unwrap();
            let mut incoming = &raw[..n];
            while !incoming.is_empty() {
                conn.read_tls(&mut incoming)?;
                conn.process_new_packets()
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            }
            while conn.wants_write() {
                conn.write_tls(&mut &self.sock)?;
            }
        }
    }
}

/// Moves decrypted bytes into `out`. Returns whether there were any; fails
/// once the peer has closed and nothing is left.
fn take_plaintext(conn: &mut Connection, out: &mut Vec<u8>) -> io::Result<bool> {
    let closed = match conn.reader().read_to_end(out) {
        Ok(_) => true,
        Err(e) if e.kind() == io::ErrorKind::WouldBlock => false,
        Err(e) => return Err(e),
    };
    if !out.is_empty() {
        Ok(true)
    } else if closed {
        Err(io::ErrorKind::UnexpectedEof.into())
    } else {
        Ok(false)
    }
}

impl Write for &Secure {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.send(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Plaintext reader over a session; only one thread should read.
pub struct SecureReader {
    secure: Arc<Secure>,
    buf: Vec<u8>,
    pos: usize,
}

impl SecureReader {
    pub fn new(secure: Arc<Secure>) -> Self {
        SecureReader { secure, buf: Vec::new(), pos: 0 }
    }
}

impl Read for SecureReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.pos == self.buf.len() {
            self.buf.clear();
            self.pos = 0;
            self.secure.receive(&mut self.buf)?;
        }
        let n = out.len().min(self.buf.len() - self.pos);
        out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

/// Accepts any certificate but verifies handshake signatures with it.
#[derive(Debug)]
struct AnyCertificate(Arc<CryptoProvider>);

impl AnyCertificate {
    fn schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

impl ServerCertVerifier for AnyCertificate {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.schemes()
    }
}

impl ClientCertVerifier for AnyCertificate {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }

    fn verify_client_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> Result<ClientCertVerified, rustls::Error> {
        Ok(ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.schemes()
    }
}

/// Two TLS sessions joined over loopback, for tests.
#[cfg(test)]
pub fn loopback_pair(server: &Identity, client: &Identity) -> (Arc<Secure>, Arc<Secure>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server_tls = TlsConfig::new(server).unwrap();
    let client_tls = TlsConfig::new(client).unwrap();
    let accept = std::thread::spawn(move || {
        let (sock, _) = listener.accept().unwrap();
        server_tls.accept(sock).unwrap()
    });
    let client_side = client_tls.connect(TcpStream::connect(addr).unwrap()).unwrap();
    (Arc::new(accept.join().unwrap()), Arc::new(client_side))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::tests::test_identity;
    use sc_proto::{Message, read_frame, write_frame};

    /// Both sides write straight after the handshake and then read, as a
    /// session does; the peer's frame can arrive with the last handshake bytes.
    #[test]
    fn first_frames_right_after_handshake() {
        let (server_id, client_id) = (test_identity("tls-fs"), test_identity("tls-fc"));
        for _ in 0..20 {
            let (server, client) = loopback_pair(&server_id, &client_id);
            let other = server.clone();
            let server_side = std::thread::spawn(move || {
                write_frame(&mut &*other, &Message::Ping).unwrap();
                read_frame::<Message>(&mut SecureReader::new(other)).unwrap()
            });
            write_frame(&mut &*client, &Message::Leave).unwrap();
            let got = read_frame::<Message>(&mut SecureReader::new(client)).unwrap();
            assert_eq!(got, Message::Ping);
            assert_eq!(server_side.join().unwrap(), Message::Leave);
        }
    }

    #[test]
    fn frames_flow_both_ways_with_matching_identities() {
        let (server_id, client_id) = (test_identity("tls-s"), test_identity("tls-c"));
        let (server, client) = loopback_pair(&server_id, &client_id);

        assert_eq!(server.peer_fingerprint().unwrap(), client_id.fingerprint);
        assert_eq!(client.peer_fingerprint().unwrap(), server_id.fingerprint);
        assert_eq!(server.channel_binding().unwrap(), client.channel_binding().unwrap());

        // Reader on one thread while the other side writes concurrently.
        let reader_side = server.clone();
        let reader = std::thread::spawn(move || {
            let mut r = SecureReader::new(reader_side);
            (0..100).map(|_| read_frame::<Message>(&mut r).unwrap()).collect::<Vec<_>>()
        });
        for i in 0..100 {
            write_frame(&mut &*client, &Message::MouseMove { x: i, y: -i }).unwrap();
        }
        let got = reader.join().unwrap();
        assert_eq!(got[99], Message::MouseMove { x: 99, y: -99 });

        write_frame(&mut &*server, &Message::Ping).unwrap();
        assert_eq!(
            read_frame::<Message>(&mut SecureReader::new(client.clone())).unwrap(),
            Message::Ping
        );
    }
}
