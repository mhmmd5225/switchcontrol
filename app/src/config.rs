//! Settings from an optional TOML file, overridden by command-line flags.

use std::path::PathBuf;

use sc_core::Side;
use sc_proto::DEFAULT_PORT;
use serde::Deserialize;

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
    pub pin: Option<String>,
    pub data_dir: PathBuf,
    pub forget_peers: bool,
    pub dry_run: bool,
    pub verbose: bool,
}

/// The TOML file uses the long flag names, e.g. `side = "right"`.
/// The PIN is deliberately not accepted from the file.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct FileConfig {
    name: Option<String>,
    side: Option<String>,
    listen: Option<String>,
    connect: Option<String>,
    speed: Option<f64>,
    swap_ctrl_cmd: Option<bool>,
    data_dir: Option<PathBuf>,
}

pub fn from_args(args: &[String]) -> Result<Settings, String> {
    let mut file = FileConfig::default();
    let mut cli = FileConfig::default();
    let mut pin = None;
    let mut forget_peers = false;
    let mut dry_run = false;
    let mut verbose = false;

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
            "--config" => file = read_file(PathBuf::from(value()?))?,
            "--pin" => {
                let raw = value()?;
                pin =
                    Some(normalize_pin(&raw).ok_or_else(|| format!("--pin: not 6 digits: {raw}"))?);
            }
            "--data-dir" => cli.data_dir = Some(PathBuf::from(value()?)),
            "--forget-peers" => forget_peers = true,
            "--dry-run" => dry_run = true,
            "-v" | "--verbose" => verbose = true,
            "-h" | "--help" => return Err(String::new()),
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    let side = match cli.side.or(file.side).as_deref() {
        Some("left") => Side::Left,
        Some("right") => Side::Right,
        Some(other) => return Err(format!("side must be left or right, not {other}")),
        None => return Err("--side is required".into()),
    };
    // A flag of either kind beats the file; listen and connect exclude each other.
    let (listen, connect) = if cli.listen.is_some() || cli.connect.is_some() {
        (cli.listen, cli.connect)
    } else {
        (file.listen, file.connect)
    };
    let net = match (listen, connect) {
        (Some(_), Some(_)) => return Err("use either --listen or --connect, not both".into()),
        (_, Some(addr)) => NetMode::Connect(with_port(addr)),
        (Some(addr), None) => NetMode::Listen(with_port(addr)),
        (None, None) => NetMode::Listen(format!("0.0.0.0:{DEFAULT_PORT}")),
    };
    let speed = cli.speed.or(file.speed).unwrap_or(1.0);
    if !(speed > 0.0 && speed <= 10.0) {
        return Err(format!("speed must be in (0, 10], not {speed}"));
    }

    Ok(Settings {
        name: cli.name.or(file.name).unwrap_or_else(default_name),
        side,
        net,
        swap_ctrl_cmd: cli.swap_ctrl_cmd.or(file.swap_ctrl_cmd).unwrap_or(true),
        speed,
        pin,
        data_dir: cli.data_dir.or(file.data_dir).unwrap_or_else(identity::default_dir),
        forget_peers,
        dry_run,
        verbose,
    })
}

fn read_file(path: PathBuf) -> Result<FileConfig, String> {
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

fn with_port(addr: String) -> String {
    if addr.contains(':') { addr } else { format!("{addr}:{DEFAULT_PORT}") }
}

fn default_name() -> String {
    ["COMPUTERNAME", "HOSTNAME"]
        .iter()
        .find_map(|var| std::env::var(var).ok())
        .unwrap_or_else(|| "switchcontrol".into())
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
        assert_eq!(s.speed, 1.0);
    }

    #[test]
    fn connect_adds_default_port() {
        let s = parse("--side left --connect 192.168.1.20 --no-swap --speed 1.5").unwrap();
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
}
