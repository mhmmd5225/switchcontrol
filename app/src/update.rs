//! Download a stable release in the background, then replace the installation
//! with a helper after the application has shut down.

use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use eframe::egui;
use ring::digest::{Context, SHA256};
use semver::Version;
use serde::Deserialize;
use tempfile::{Builder, TempDir};

const RELEASES: &str = "https://github.com/mhmmd5225/switchcontrol/releases";
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
const MAX_PAYLOAD: u64 = 100 * 1024 * 1024;
const STAGE_PREFIX: &str = ".switchcontrol-update-";
const MAC_BINARY: &str = "Contents/MacOS/SwitchControl";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Platform {
    Windows,
    Mac,
}

impl Platform {
    fn current() -> Option<Self> {
        if cfg!(all(windows, target_arch = "x86_64")) {
            Some(Self::Windows)
        } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            Some(Self::Mac)
        } else {
            None
        }
    }

    fn target(self, exe: &Path) -> Option<PathBuf> {
        match self {
            Self::Windows => Some(exe.to_owned()),
            Self::Mac => {
                let bundle = exe.ancestors().nth(3)?;
                (bundle.extension() == Some(OsStr::new("app")) && bundle.join(MAC_BINARY) == exe)
                    .then(|| bundle.to_owned())
            }
        }
    }

    fn asset(self) -> &'static str {
        match self {
            Self::Windows => "switchcontrol.exe",
            Self::Mac => "SwitchControl-macOS.zip",
        }
    }

    fn staged(self, stage: &Path) -> PathBuf {
        match self {
            Self::Windows => stage.join(self.asset()),
            Self::Mac => stage.join("SwitchControl.app"),
        }
    }
}

#[derive(Deserialize)]
struct Manifest {
    version: String,
    windows: Asset,
    macos: Asset,
}

#[derive(Deserialize)]
struct Asset {
    sha256: String,
    bytes: u64,
}

fn candidate(current: &Version, published: &str) -> Option<Version> {
    let next = Version::parse(published.strip_prefix('v')?).ok()?;
    (next.pre.is_empty() && next.build.is_empty() && next > *current).then_some(next)
}

fn verify(path: &Path, asset: &Asset) -> io::Result<()> {
    let mut file = File::open(path)?;
    if asset.bytes == 0 || asset.bytes > MAX_PAYLOAD || file.metadata()?.len() != asset.bytes {
        return Err(io::Error::other("Güncelleme dosyasının boyutu geçersiz."));
    }
    if checksum(&mut file)? != asset.sha256 {
        return Err(io::Error::other("Güncelleme dosyasının SHA-256 özeti eşleşmiyor."));
    }
    Ok(())
}

fn checksum(mut file: impl Read) -> io::Result<String> {
    let mut sha = Context::new(&SHA256);
    let mut buffer = [0; 32 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        sha.update(&buffer[..count]);
    }
    Ok(sha.finish().as_ref().iter().map(|byte| format!("{byte:02x}")).collect())
}

fn quiet(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    command
}

fn download(url: &str, path: &Path, limit: u64, timeout: u32) -> io::Result<()> {
    let curl = if cfg!(windows) {
        std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .ok_or_else(|| io::Error::other("SystemRoot bulunamadı."))?
            .join("System32/curl.exe")
    } else {
        PathBuf::from("/usr/bin/curl")
    };
    let status = quiet(curl)
        .args([
            "--disable",
            "--fail",
            "--location",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--connect-timeout",
            "12",
            "--max-time",
            &timeout.to_string(),
            "--max-filesize",
            &limit.to_string(),
            "--output",
        ])
        .arg(path)
        .arg(url)
        .status()?;
    if !status.success() || fs::metadata(path)?.len() > limit {
        return Err(io::Error::other("GitHub güncelleme dosyası indirilemedi."));
    }
    Ok(())
}

struct Prepared {
    stage: TempDir,
    target: PathBuf,
    version: Version,
}

fn prepare(platform: Platform, exe: &Path, current: &Version) -> io::Result<Option<Prepared>> {
    let metadata = tempfile::NamedTempFile::new()?;
    download(&format!("{RELEASES}/latest/download/update.toml"), metadata.path(), 4096, 20)?;
    let manifest: Manifest = toml::from_str(&fs::read_to_string(metadata.path())?)
        .map_err(|_| io::Error::other("Güncelleme bilgileri okunamadı."))?;
    let Some(version) = candidate(current, &manifest.version) else {
        return Ok(None);
    };
    let target =
        platform.target(exe).ok_or_else(|| io::Error::other("Uygulama paketi bulunamadı."))?;
    let parent = target.parent().ok_or_else(|| io::Error::other("Kurulum klasörü bulunamadı."))?;
    let stage = Builder::new().prefix(STAGE_PREFIX).tempdir_in(parent).map_err(|e| {
        io::Error::other(format!("Kurulum klasöründe güncelleme hazırlanamadı: {e}"))
    })?;
    fs::write(stage.path().join("current-sha256"), checksum(File::open(exe)?)?)?;
    let payload = stage.path().join(platform.asset());
    let asset = match platform {
        Platform::Windows => manifest.windows,
        Platform::Mac => manifest.macos,
    };
    if asset.bytes == 0 || asset.bytes > MAX_PAYLOAD {
        return Err(io::Error::other("Güncelleme dosyasının boyutu geçersiz."));
    }
    download(
        &format!("{RELEASES}/download/v{version}/{}", platform.asset()),
        &payload,
        asset.bytes,
        240,
    )?;
    verify(&payload, &asset)?;
    if platform == Platform::Mac {
        let unpacked =
            quiet("/usr/bin/ditto").args(["-x", "-k"]).arg(&payload).arg(stage.path()).status()?;
        let bundle = platform.staged(stage.path());
        if !unpacked.success()
            || !bundle.join(MAC_BINARY).is_file()
            || !quiet("/usr/bin/codesign")
                .args([
                    "--verify",
                    "--deep",
                    "--strict",
                    "-R",
                    "identifier \"com.switchcontrol.app\"",
                ])
                .arg(&bundle)
                .status()?
                .success()
        {
            return Err(io::Error::other("macOS uygulama paketi doğrulanamadı."));
        }
    }
    Ok(Some(Prepared { stage, target, version }))
}

fn gate(stage: &Path) -> io::Result<File> {
    open_lock(&stage.join("lock"))
}

fn open_lock(path: &Path) -> io::Result<File> {
    OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path)
}

fn wait_for_gate(stage: &Path) -> io::Result<File> {
    wait_for_lock(gate(stage)?)
}

fn wait_for_lock(lock: File) -> io::Result<File> {
    let end = Instant::now() + Duration::from_secs(30);
    loop {
        match lock.try_lock() {
            Ok(()) => return Ok(lock),
            Err(TryLockError::WouldBlock) if Instant::now() < end => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(e.into()),
        }
    }
}

impl Prepared {
    fn start_helper(&self, exe: &Path) -> io::Result<File> {
        let stage = self.stage.path();
        let helper = stage.join(if cfg!(windows) { "installer.exe" } else { "installer" });
        fs::copy(exe, &helper)?;
        let lock = gate(stage)?;
        lock.lock()?;
        quiet(helper).arg("--apply-update").arg(&self.target).spawn()?;
        Ok(lock)
    }
}

#[cfg(target_os = "macos")]
fn swap_bundle(old: &Path, new: &Path) -> io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let old = CString::new(old.as_os_str().as_bytes())?;
    let new = CString::new(new.as_os_str().as_bytes())?;
    if unsafe { libc::renamex_np(old.as_ptr(), new.as_ptr(), libc::RENAME_SWAP) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(target_os = "macos"))]
fn swap_bundle(_old: &Path, _new: &Path) -> io::Result<()> {
    Err(io::Error::other("macOS gerekli."))
}

fn retry_rename(from: &Path, to: &Path) -> io::Result<()> {
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        match fs::rename(from, to) {
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied && Instant::now() < end => {
                std::thread::sleep(Duration::from_millis(100));
            }
            result => return result,
        }
    }
}

fn reopen(platform: Platform, target: &Path, stage: &Path) -> io::Result<()> {
    let mut command = match platform {
        Platform::Windows => quiet(target),
        Platform::Mac => {
            let mut open = quiet("/usr/bin/open");
            open.arg("-n").arg(target).arg("--args");
            open
        }
    };
    let mut launched = command.arg("--finish-update").arg(stage).spawn()?;
    if platform == Platform::Mac && !launched.wait()?.success() {
        return Err(io::Error::other("SwitchControl yeniden açılamadı."));
    }
    Ok(())
}

fn apply(platform: Platform, helper: &Path, target: &Path) -> io::Result<()> {
    let stage =
        helper.parent().ok_or_else(|| io::Error::other("Güncelleme klasörü bulunamadı."))?;
    if !valid_stage(stage, target) {
        return Err(io::Error::other("Geçersiz güncelleme klasörü."));
    }
    let _lock = wait_for_gate(stage)?;
    let parent = target.parent().ok_or_else(|| io::Error::other("Kurulum klasörü bulunamadı."))?;
    let _installation = wait_for_lock(open_lock(&parent.join(".switchcontrol-install.lock"))?)?;
    let executable = match platform {
        Platform::Mac => target.join(MAC_BINARY),
        Platform::Windows => target.to_owned(),
    };
    let expected = fs::read_to_string(stage.join("current-sha256"))?;
    if expected != checksum(File::open(executable)?)? {
        // Another instance already replaced this installation while the
        // package was being downloaded. Keep the version it installed.
        return reopen(platform, target, stage);
    }
    let new = platform.staged(stage);
    let backup = stage.join("previous.exe");
    let install = match platform {
        Platform::Mac => swap_bundle(target, &new),
        Platform::Windows => fs::copy(target, &backup).and_then(|_| retry_rename(&new, target)),
    };
    if install.is_ok() && reopen(platform, target, stage).is_ok() {
        return Ok(());
    }
    if install.is_ok() {
        // Preserve the prior version if the new application cannot be launched.
        match platform {
            Platform::Mac => swap_bundle(target, &new)?,
            Platform::Windows => retry_rename(&backup, target)?,
        }
    }
    fs::write(stage.join("failed"), b"")?;
    reopen(platform, target, stage)?;
    Ok(())
}

fn valid_stage(stage: &Path, target: &Path) -> bool {
    let parent = stage.parent().and_then(|path| fs::canonicalize(path).ok());
    parent.is_some()
        && parent == target.parent().and_then(|path| fs::canonicalize(path).ok())
        && stage
            .file_name()
            .and_then(OsStr::to_str)
            .is_some_and(|name| name.starts_with(STAGE_PREFIX))
        && fs::symlink_metadata(stage).is_ok_and(|metadata| metadata.file_type().is_dir())
}

pub fn helper_command(args: &[OsString]) -> Option<io::Result<()>> {
    let [flag, target] = args else { return None };
    if flag != "--apply-update" {
        return None;
    }
    Some((|| {
        let platform =
            Platform::current().ok_or_else(|| io::Error::other("Sistem desteklenmiyor."))?;
        apply(platform, &std::env::current_exe()?, Path::new(target))
    })())
}

/// Returns true if installation failed and the previous build was reopened.
pub fn finish(stage: &Path) -> bool {
    let Some(target) = std::env::current_exe()
        .ok()
        .and_then(|exe| Platform::current()?.target(&exe))
        .filter(|target| valid_stage(stage, target))
    else {
        return false;
    };
    let Ok(lock) = wait_for_gate(stage) else { return false };
    let failed = stage.join("failed").is_file();
    drop(lock);
    // Windows may still be unmapping the helper after its lock closes.
    if valid_stage(stage, &target) {
        for _ in 0..10 {
            if fs::remove_dir_all(stage).is_ok() || !stage.exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    failed
}

pub struct Updater {
    current: Version,
    platform: Platform,
    exe: PathBuf,
    off_file: PathBuf,
    next_check: Instant,
    download: Option<Receiver<io::Result<Option<Prepared>>>>,
    ready: Option<Prepared>,
    pub enabled: bool,
    pub status: String,
}

impl Updater {
    pub fn new(data_dir: &Path, failed: bool) -> Option<Self> {
        let current =
            Version::parse(option_env!("SWITCHCONTROL_VERSION")?.strip_prefix('v')?).ok()?;
        let platform = Platform::current()?;
        let exe = std::env::current_exe().ok()?;
        platform.target(&exe)?;
        let off_file = data_dir.join("automatic-updates-disabled");
        Some(Self {
            platform,
            exe,
            current,
            enabled: !off_file.exists(),
            off_file,
            next_check: Instant::now()
                + if failed { CHECK_INTERVAL } else { Duration::from_secs(10) },
            download: None,
            ready: None,
            status: if failed {
                "Kurulum tamamlanamadı. Önceki sürüm yeniden açıldı.".into()
            } else {
                String::new()
            },
        })
    }

    pub fn set_enabled(&mut self, enabled: bool) -> io::Result<()> {
        if enabled {
            match fs::remove_file(&self.off_file) {
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                result => result?,
            }
        } else {
            fs::create_dir_all(self.off_file.parent().expect("setting has a directory"))?;
            fs::write(&self.off_file, b"")?;
        }
        self.enabled = enabled;
        self.status.clear();
        self.ready = None;
        self.download = None;
        self.next_check = Instant::now();
        Ok(())
    }

    /// Returns a lock to hold until process exit so the helper can install.
    pub fn poll(&mut self, ctx: &egui::Context, idle: bool) -> Option<File> {
        if !self.enabled {
            return None;
        }
        if let Some(download) = &self.download {
            match download.try_recv() {
                Ok(result) => {
                    self.download = None;
                    match result {
                        Ok(Some(ready)) => {
                            self.status =
                                format!("{} hazır. Paylaşım durunca kurulacak.", ready.version);
                            self.ready = Some(ready);
                        }
                        Ok(None) => self.status = format!("Sürüm {} güncel.", self.current),
                        Err(e) => {
                            self.status = format!("Güncelleme denetlenemedi: {e}");
                            self.next_check = Instant::now() + Duration::from_secs(15 * 60);
                        }
                    }
                }
                Err(mpsc::TryRecvError::Empty) => return None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.download = None;
                    self.status = "Güncelleme denetimi kesildi.".into();
                }
            }
        }

        if idle && let Some(ready) = self.ready.take() {
            match ready.start_helper(&self.exe) {
                Ok(lock) => {
                    let _ = ready.stage.keep();
                    return Some(lock);
                }
                Err(e) => self.status = format!("Güncelleme kurulamadı: {e}"),
            }
        }
        if self.ready.is_none() {
            let remaining = self.next_check.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                self.next_check = Instant::now() + CHECK_INTERVAL;
                let (send, receive) = mpsc::channel();
                let (ctx, exe, current, platform) =
                    (ctx.clone(), self.exe.clone(), self.current.clone(), self.platform);
                self.status = "Güncellemeler denetleniyor…".into();
                self.download = Some(receive);
                std::thread::spawn(move || {
                    let _ = send.send(prepare(platform, &exe, &current));
                    ctx.request_repaint_of(egui::ViewportId::ROOT);
                });
            } else {
                ctx.request_repaint_after(remaining);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn updates_only_to_newer_stable_versions() {
        let current = Version::parse("0.3.9").unwrap();
        for invalid in ["v0.3.8", "v0.3.9", "v0.4.0-rc.1", "v0.4.0+test", "0.4.0", "v../latest"] {
            assert_eq!(candidate(&current, invalid), None);
        }
        assert_eq!(candidate(&current, "v0.3.10"), Version::parse("0.3.10").ok());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn installer_restores_previous_executable_when_launch_fails() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("SwitchControl.exe");
        let old = b"#!/bin/sh\nexit 0\n";
        fs::write(&target, old).unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).unwrap();
        let stage = Builder::new().prefix(STAGE_PREFIX).tempdir_in(root.path()).unwrap();
        let expected = checksum(old.as_slice()).unwrap();
        fs::write(stage.path().join("current-sha256"), expected).unwrap();
        fs::write(Platform::Windows.staged(stage.path()), b"invalid executable").unwrap();

        apply(Platform::Windows, &stage.path().join("installer.exe"), &target).unwrap();

        assert_eq!(fs::read(target).unwrap(), old);
        assert!(stage.path().join("failed").is_file());
    }

    #[test]
    fn rejects_truncated_or_changed_payloads() {
        let file = tempfile::NamedTempFile::new().unwrap();
        fs::write(file.path(), b"abc").unwrap();
        let asset = Asset {
            bytes: 3,
            sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".into(),
        };
        verify(file.path(), &asset).unwrap();
        for invalid in [b"ab".as_slice(), b"xyz"] {
            fs::write(file.path(), invalid).unwrap();
            assert!(verify(file.path(), &asset).is_err());
        }
    }
}
