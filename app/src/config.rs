//! Settings from an optional TOML file, overridden by command-line flags.

use std::path::{Path, PathBuf};
use std::time::Duration;

use sc_core::Side;
use sc_proto::DEFAULT_PORT;
use serde::{Deserialize, Serialize};

use crate::identity;
use crate::pairing::normalize_pin;

pub const USAGE: &str = "\
usage: switchcontrol --side left|right [--listen ADDR | --connect ADDR] [options]

  --side left|right   where the other computer sits, seen from this one
  --listen ADDR       wait for the peer (default 0.0.0.0:24800)
  --connect ADDR      connect to the peer, e.g. 192.168.1.20 or 192.168.1.20:24800
  --name NAME         name shown to the peer (default: computer name)
  --speed F           pointer speed on the peer (default 1.0)
  --no-swap           keep Ctrl and Cmd/Win as they are
  --edge-delay MS     push against the edge this long before crossing (default 0)
  --no-clipboard      do not share clipboard text with the other computer
  --no-announce       do not make this computer visible on the local network
  --config FILE       read these settings from a TOML file (flags win)
  --pin PIN           PIN for pairing, instead of typing it when asked
  --data-dir DIR      where this computer's certificate and paired peers live
  --forget-peers      remove all paired computers, then continue
  --dry-run           no capture or injection; log what would be injected
  -v, --verbose       log every message sent and received

The first connection between two computers needs pairing: the listening side
shows a 6-digit PIN, which is entered on the connecting side.

Ctrl+Alt+Shift+Esc (Mac: Ctrl+Option+Shift+Esc) always returns the cursor.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetMode {
    Listen(String),
    Connect(String),
}

#[derive(Debug, Clone)]
pub struct Settings {
    pub name: String,
    pub side: Side,
    pub net: NetMode,
    pub swap_ctrl_cmd: bool,
    pub speed: f64,
    pub edge_delay: Duration,
    /// Share clipboard text when control passes between the computers.
    pub clipboard: bool,
    /// While listening, announce this computer on the local network.
    pub announce: bool,
    pub pin: Option<String>,
    pub data_dir: PathBuf,
    pub forget_peers: bool,
    pub dry_run: bool,
    pub verbose: bool,
}

/// The TOML file uses the long flag names, e.g. `side = "right"`.
/// The PIN is deliberately not accepted from the file.
#[derive(Debug, Default, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct FileConfig {
    pub name: Option<String>,
    pub side: Option<String>,
    pub listen: Option<String>,
    pub connect: Option<String>,
    pub speed: Option<f64>,
    pub swap_ctrl_cmd: Option<bool>,
    pub edge_delay_ms: Option<u64>,
    pub clipboard: Option<bool>,
    pub announce: Option<bool>,
    /// Draw a bar at the shared edge when the cursor comes near (window only).
    pub edge_bar: Option<bool>,
    pub data_dir: Option<PathBuf>,
}

/// Switches that only exist for one run, never in a file.
#[derive(Debug, Default, Clone)]
pub struct RunFlags {
    pub pin: Option<String>,
    pub forget_peers: bool,
    pub dry_run: bool,
    pub verbose: bool,
}

pub fn from_args(args: &[String]) -> Result<Settings, String> {
    let mut file = FileConfig::default();
    let mut cli = FileConfig::default();
    let mut flags = RunFlags::default();

    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = || it.next().cloned().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--side" => cli.side = Some(value()?),
            "--listen" => cli.listen = Some(value()?),
            "--connect" => cli.connect = Some(value()?),
            "--name" => cli.name = Some(value()?),
            "--speed" => {
                let raw = value()?;
                cli.speed = Some(raw.parse().map_err(|_| format!("--speed: not a number: {raw}"))?);
            }
            "--no-swap" => cli.swap_ctrl_cmd = Some(false),
            "--no-clipboard" => cli.clipboard = Some(false),
            "--no-announce" => cli.announce = Some(false),
            "--edge-delay" => {
                let raw = value()?;
                cli.edge_delay_ms = Some(
                    raw.parse().map_err(|_| format!("--edge-delay: not milliseconds: {raw}"))?,
                );
            }
            "--config" => file = load(Path::new(&value()?))?,
            "--pin" => {
                let raw = value()?;
                flags.pin =
                    Some(normalize_pin(&raw).ok_or_else(|| format!("--pin: not 6 digits: {raw}"))?);
            }
            "--data-dir" => cli.data_dir = Some(PathBuf::from(value()?)),
            "--forget-peers" => flags.forget_peers = true,
            "--dry-run" => flags.dry_run = true,
            "-v" | "--verbose" => flags.verbose = true,
            "-h" | "--help" => return Err(String::new()),
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    // A flag of either kind beats the file; listen and connect exclude each other.
    let (listen, connect) = if cli.listen.is_some() || cli.connect.is_some() {
        (cli.listen, cli.connect)
    } else {
        (file.listen, file.connect)
    };
    let merged = FileConfig {
        name: cli.name.or(file.name),
        side: cli.side.or(file.side),
        listen,
        connect,
        speed: cli.speed.or(file.speed),
        swap_ctrl_cmd: cli.swap_ctrl_cmd.or(file.swap_ctrl_cmd),
        edge_delay_ms: cli.edge_delay_ms.or(file.edge_delay_ms),
        clipboard: cli.clipboard.or(file.clipboard),
        announce: cli.announce.or(file.announce),
        edge_bar: file.edge_bar,
        data_dir: cli.data_dir.or(file.data_dir),
    };
    resolve(merged, flags)
}

/// Checks a configuration and fills in the defaults.
pub fn resolve(config: FileConfig, flags: RunFlags) -> Result<Settings, String> {
    let side = match config.side.as_deref() {
        Some("left") => Side::Left,
        Some("right") => Side::Right,
        Some(other) => return Err(format!("side must be left or right, not {other}")),
        None => return Err("--side is required".into()),
    };
    let net = match (config.listen, config.connect) {
        (Some(_), Some(_)) => return Err("use either --listen or --connect, not both".into()),
        (_, Some(addr)) => NetMode::Connect(with_port(addr)),
        (Some(addr), None) => NetMode::Listen(with_port(addr)),
        (None, None) => NetMode::Listen(format!("0.0.0.0:{DEFAULT_PORT}")),
    };
    let speed = config.speed.unwrap_or(1.0);
    if !(speed > 0.0 && speed <= 10.0) {
        return Err(format!("speed must be in (0, 10], not {speed}"));
    }
    let edge_delay_ms = config.edge_delay_ms.unwrap_or(0);
    if edge_delay_ms > 5000 {
        return Err(format!("edge delay must be at most 5000 ms, not {edge_delay_ms}"));
    }

    Ok(Settings {
        name: config.name.filter(|n| !n.trim().is_empty()).unwrap_or_else(default_name),
        side,
        net,
        swap_ctrl_cmd: config.swap_ctrl_cmd.unwrap_or(true),
        speed,
        edge_delay: Duration::from_millis(edge_delay_ms),
        clipboard: config.clipboard.unwrap_or(true),
        announce: config.announce.unwrap_or(true),
        pin: flags.pin,
        data_dir: config.data_dir.unwrap_or_else(identity::default_dir),
        forget_peers: flags.forget_peers,
        dry_run: flags.dry_run,
        verbose: flags.verbose,
    })
}

pub fn load(path: &Path) -> Result<FileConfig, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn save(path: &Path, config: &FileConfig) -> Result<(), String> {
    let text = toml::to_string(config).map_err(|e| e.to_string())?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Adds the default port. An empty address stays empty: it means "find a
/// paired computer on the network".
fn with_port(addr: String) -> String {
    let addr = addr.trim().to_string();
    if addr.is_empty() || addr.contains(':') { addr } else { format!("{addr}:{DEFAULT_PORT}") }
}

pub fn default_name() -> String {
    ["COMPUTERNAME", "HOSTNAME"]
        .iter()
        .find_map(|var| std::env::var(var).ok())
        .or_else(host_name)
        .unwrap_or_else(|| "switchcontrol".into())
}

/// macOS does not export `HOSTNAME` to apps started from Finder.
#[cfg(unix)]
fn host_name() -> Option<String> {
    let out = std::process::Command::new("hostname").arg("-s").output().ok()?;
    let name = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (!name.is_empty()).then_some(name)
}

#[cfg(not(unix))]
fn host_name() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &str) -> Result<Settings, String> {
        let args: Vec<String> = args.split_whitespace().map(String::from).collect();
        from_args(&args)
    }

    #[test]
    fn defaults_to_listening() {
        let s = parse("--side right").unwrap();
        assert_eq!(s.side, Side::Right);
        assert_eq!(s.net, NetMode::Listen("0.0.0.0:24800".into()));
        assert!(s.swap_ctrl_cmd);
        assert!(s.clipboard);
        assert_eq!(s.speed, 1.0);
        assert!(!parse("--side right --no-clipboard").unwrap().clipboard);
    }

    #[test]
    fn connect_adds_default_port() {
        let s = parse("--side left --connect 192.168.1.20 --no-swap --speed 1.5 --edge-delay 300")
            .unwrap();
        assert_eq!(s.edge_delay, Duration::from_millis(300));
        assert_eq!(s.net, NetMode::Connect("192.168.1.20:24800".into()));
        assert!(!s.swap_ctrl_cmd);
        assert_eq!(s.speed, 1.5);
    }

    #[test]
    fn pin_and_data_dir() {
        let s = parse("--side left --pin 123456 --data-dir D:/sc --forget-peers").unwrap();
        assert_eq!(s.pin.as_deref(), Some("123456"));
        assert_eq!(s.data_dir, PathBuf::from("D:/sc"));
        assert!(s.forget_peers);
        assert!(parse("--side left --pin 12345").is_err());
    }

    #[test]
    fn rejects_bad_input() {
        assert!(parse("").is_err());
        assert!(parse("--side up").is_err());
        assert!(parse("--side left --listen :1 --connect x").is_err());
        assert!(parse("--side left --speed 0").is_err());
        assert!(parse("--side left --edge-delay 9000").is_err());
        assert!(parse("--side left --edge-delay -1").is_err());
        assert!(parse("--side left --bogus").is_err());
    }

    #[test]
    fn file_settings_are_overridden_by_flags() {
        let path = std::env::temp_dir().join(format!("sc-config-{}.toml", std::process::id()));
        std::fs::write(&path, "side = \"left\"\nconnect = \"10.0.0.5\"\nspeed = 2.0\n").unwrap();
        let s = parse(&format!("--config {} --speed 0.5", path.display())).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(s.side, Side::Left);
        assert_eq!(s.net, NetMode::Connect("10.0.0.5:24800".into()));
        assert_eq!(s.speed, 0.5);
    }

    #[test]
    fn saved_file_round_trips() {
        let path = std::env::temp_dir().join(format!("sc-save-{}.toml", std::process::id()));
        let config = FileConfig {
            side: Some("left".into()),
            connect: Some("10.0.0.5".into()),
            speed: Some(1.5),
            swap_ctrl_cmd: Some(false),
            ..FileConfig::default()
        };
        save(&path, &config).unwrap();
        let loaded = load(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(loaded, config);
    }
}
