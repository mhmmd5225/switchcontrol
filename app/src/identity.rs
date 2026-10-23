//! This machine's TLS identity and the list of peers it has paired with.
//!
//! The certificate is self-signed and only identifies the machine: trust
//! comes from pairing, which records the peer certificate's fingerprint.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use ring::digest::{SHA256, digest};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

const CERT_FILE: &str = "cert.der";
const KEY_FILE: &str = "key.der";
const TRUST_FILE: &str = "trusted-peers.txt";

/// SHA-256 of a certificate's DER encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fingerprint([u8; 32]);

impl Fingerprint {
    pub fn of(cert: &CertificateDer<'_>) -> Self {
        let mut out = [0u8; 32];
        out.copy_from_slice(digest(&SHA256, cert.as_ref()).as_ref());
        Fingerprint(out)
    }

    pub fn bytes(self) -> [u8; 32] {
        self.0
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Fingerprint(bytes)
    }

    fn to_hex(self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn from_hex(text: &str) -> Option<Self> {
        if text.len() != 64 || !text.is_ascii() {
            return None;
        }
        let mut out = [0u8; 32];
        for (i, byte) in out.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&text[2 * i..2 * i + 2], 16).ok()?;
        }
        Some(Fingerprint(out))
    }
}

/// Short form for logs, e.g. `3f2a:91c0:7be4:0d5e`.
impl fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let hex = self.to_hex();
        write!(f, "{}:{}:{}:{}", &hex[0..4], &hex[4..8], &hex[8..12], &hex[12..16])
    }
}

pub struct Identity {
    pub cert: CertificateDer<'static>,
    pub key: PrivateKeyDer<'static>,
    pub fingerprint: Fingerprint,
}

impl Identity {
    /// Loads the identity from `dir`, creating it on first run.
    pub fn load_or_create(dir: &Path) -> io::Result<Identity> {
        let (cert_path, key_path) = (dir.join(CERT_FILE), dir.join(KEY_FILE));
        let (cert, key) = if cert_path.exists() && key_path.exists() {
            (fs::read(&cert_path)?, fs::read(&key_path)?)
        } else {
            let generated = rcgen::generate_simple_self_signed(vec!["switchcontrol".into()])
                .map_err(io::Error::other)?;
            let cert = generated.cert.der().to_vec();
            let key = generated.key_pair.serialize_der();
            fs::create_dir_all(dir)?;
            write_private(&key_path, &key)?;
            fs::write(&cert_path, &cert)?;
            (cert, key)
        };
        let cert = CertificateDer::from(cert);
        Ok(Identity {
            fingerprint: Fingerprint::of(&cert),
            cert,
            key: PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key)),
        })
    }
}

/// Fingerprints of paired peers, one `<hex> <name>` per line.
pub struct TrustStore {
    path: PathBuf,
    peers: Vec<(Fingerprint, String)>,
}

impl TrustStore {
    pub fn load(dir: &Path) -> io::Result<TrustStore> {
        let path = dir.join(TRUST_FILE);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(e),
        };
        let peers = text
            .lines()
            .filter_map(|line| {
                let (hex, name) = line.trim().split_once(' ').unwrap_or((line.trim(), ""));
                Some((Fingerprint::from_hex(hex)?, name.to_string()))
            })
            .collect();
        Ok(TrustStore { path, peers })
    }

    pub fn contains(&self, fingerprint: &Fingerprint) -> bool {
        self.peers.iter().any(|(fp, _)| fp == fingerprint)
    }

    /// Records a paired peer, replacing an older entry with the same fingerprint.
    pub fn add(&mut self, fingerprint: Fingerprint, name: &str) -> io::Result<()> {
        self.peers.retain(|(fp, _)| *fp != fingerprint);
        self.peers.push((fingerprint, name.to_string()));
        self.save()
    }

    pub fn clear(&mut self) -> io::Result<()> {
        self.peers.clear();
        self.save()
    }

    pub fn len(&self) -> usize {
        self.peers.len()
    }

    pub fn fingerprints(&self) -> Vec<Fingerprint> {
        self.peers.iter().map(|(fp, _)| *fp).collect()
    }

    fn save(&self) -> io::Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let text: String =
            self.peers.iter().map(|(fp, name)| format!("{} {name}\n", fp.to_hex())).collect();
        fs::write(&self.path, text)
    }
}

/// Where identity and trust live unless `--data-dir` says otherwise.
pub fn default_dir() -> PathBuf {
    let home = || std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(home).join("SwitchControl")
    } else if cfg!(target_os = "macos") {
        home().join("Library/Application Support/SwitchControl")
    } else {
        home().join(".config/switchcontrol")
    }
}

/// Writes the private key readable by the owner only (on Unix).
fn write_private(path: &Path, data: &[u8]) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?
            .write_all(data)
    }
    #[cfg(not(unix))]
    fs::write(path, data)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A fresh identity in its own temp directory.
    pub(crate) fn test_identity(name: &str) -> Identity {
        Identity::load_or_create(&temp_dir(name)).unwrap()
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sc-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn identity_is_created_once_and_reloaded() {
        let dir = temp_dir("identity");
        let first = Identity::load_or_create(&dir).unwrap();
        let second = Identity::load_or_create(&dir).unwrap();
        assert_eq!(first.fingerprint, second.fingerprint);
        assert_eq!(first.cert, second.cert);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn trust_store_persists() {
        let dir = temp_dir("trust");
        let fp = Fingerprint([7; 32]);
        let mut store = TrustStore::load(&dir).unwrap();
        assert!(!store.contains(&fp));
        store.add(fp, "mac").unwrap();
        store.add(fp, "mac renamed").unwrap();
        assert_eq!(store.len(), 1);

        let mut reloaded = TrustStore::load(&dir).unwrap();
        assert!(reloaded.contains(&fp));
        reloaded.clear().unwrap();
        assert!(!TrustStore::load(&dir).unwrap().contains(&fp));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn fingerprint_hex_round_trips() {
        let fp = Fingerprint([0xab; 32]);
        assert_eq!(Fingerprint::from_hex(&fp.to_hex()), Some(fp));
        assert_eq!(Fingerprint::from_hex("zz"), None);
        assert_eq!(fp.to_string(), "abab:abab:abab:abab");
    }
}
