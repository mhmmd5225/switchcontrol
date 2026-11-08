//! The login-screen service (macOS).
//!
//! Before anyone has logged in, and while the screen is locked behind the
//! password, nothing of the window program runs. A launchd agent that loads
//! only into the login window's session starts the program as `--service`
//! there: it waits for the other computer, receives keyboard and mouse and
//! posts them, so the password can be typed from the other computer. Once a
//! user logs in, launchd stops it and the window program takes over.
//!
//! The agent lives in `/Library/LaunchAgents`, which takes an administrator:
//! the installer (or the "Kur" button, which asks for the password once) runs
//! the program as root with [`INSTALL_ARG`].

use std::path::Path;

#[cfg_attr(not(unix), allow(dead_code))] // only `install_as_root` uses it
pub const LABEL: &str = "com.switchcontrol.loginwindow";
pub const PLIST: &str = "/Library/LaunchAgents/com.switchcontrol.loginwindow.plist";
pub const LOG: &str = "/var/log/switchcontrol.log";
/// Starts the background service (what the agent runs).
pub const SERVICE_ARG: &str = "--service";
/// Run as root: install, or remove, the agent.
pub const INSTALL_ARG: &str = "--install-login-agent";
pub const UNINSTALL_ARG: &str = "--uninstall-login-agent";
/// Prints whether the agent is installed and where its log is.
pub const STATUS_ARG: &str = "--service-status";

/// Whether this system has a login screen the program can serve.
pub fn supported() -> bool {
    cfg!(target_os = "macos")
}

pub fn is_installed() -> bool {
    Path::new(PLIST).exists()
}

/// Whether `exe` is where the agent can rely on finding it: in the
/// Applications folder, not in Downloads or on a disk image.
pub fn in_applications(exe: &Path) -> bool {
    exe.starts_with("/Applications")
}

/// The launchd agent that runs the service in the login window's session.
#[cfg_attr(not(unix), allow(dead_code))] // only `install_as_root` uses it
pub fn plist_text(exe: &Path) -> String {
    let exe = escape(&exe.display().to_string());
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
         \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n<dict>\n\
         \x20   <key>Label</key>\n    <string>{LABEL}</string>\n\
         \x20   <key>ProgramArguments</key>\n    <array>\n\
         \x20       <string>{exe}</string>\n        <string>{SERVICE_ARG}</string>\n    </array>\n\
         \x20   <key>LimitLoadToSessionType</key>\n    <string>LoginWindow</string>\n\
         \x20   <key>RunAtLoad</key>\n    <true/>\n\
         \x20   <key>KeepAlive</key>\n    <true/>\n\
         \x20   <key>ThrottleInterval</key>\n    <integer>10</integer>\n\
         \x20   <key>StandardOutPath</key>\n    <string>{LOG}</string>\n\
         \x20   <key>StandardErrorPath</key>\n    <string>{LOG}</string>\n\
         </dict>\n</plist>\n"
    )
}

#[cfg_attr(not(unix), allow(dead_code))] // only `install_as_root` uses it
fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Writes the agent and the shared data folder. Must run as root.
#[cfg(unix)]
pub fn install_as_root(exe: &Path) -> Result<(), String> {
    use std::fs::{self, Permissions};
    use std::os::unix::fs::{PermissionsExt, chown};

    // Group "admin" on macOS: the people who set the computer up may pair.
    const ADMIN_GID: u32 = 80;
    let failed = |what: &str, e: std::io::Error| format!("{what}: {e}");

    let dir = Path::new(crate::identity::SHARED_DIR);
    fs::create_dir_all(dir).map_err(|e| failed("data folder", e))?;
    chown(dir, Some(0), Some(ADMIN_GID)).map_err(|e| failed("data folder owner", e))?;
    fs::set_permissions(dir, Permissions::from_mode(0o775))
        .map_err(|e| failed("data folder access", e))?;

    // launchd refuses an agent that is not owned by root.
    let plist = Path::new(PLIST);
    fs::write(plist, plist_text(exe)).map_err(|e| failed(PLIST, e))?;
    chown(plist, Some(0), Some(0)).map_err(|e| failed(PLIST, e))?;
    fs::set_permissions(plist, Permissions::from_mode(0o644)).map_err(|e| failed(PLIST, e))
}

#[cfg(not(unix))]
pub fn install_as_root(_exe: &Path) -> Result<(), String> {
    Err("the login-screen service exists on macOS only".into())
}

/// Removes the agent (the data folder and the pairings stay). Must run as root.
pub fn uninstall_as_root() -> Result<(), String> {
    match std::fs::remove_file(PLIST) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(format!("{PLIST}: {e}")),
        _ => Ok(()),
    }
}

/// Installs the agent, asking for the administrator password.
pub fn install() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot find the program file: {e}"))?;
    as_administrator(&exe, INSTALL_ARG)
}

/// Removes the agent, asking for the administrator password.
pub fn uninstall() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot find the program file: {e}"))?;
    as_administrator(&exe, UNINSTALL_ARG)
}

/// Runs `exe arg` as root: macOS shows its own password dialog.
#[cfg(target_os = "macos")]
fn as_administrator(exe: &Path, arg: &str) -> Result<(), String> {
    let command = format!("{} {arg}", shell_quote(&exe.display().to_string()));
    let script = format!(
        "do shell script \"{}\" with administrator privileges",
        command.replace('\\', "\\\\").replace('"', "\\\"")
    );
    let output = std::process::Command::new("/usr/bin/osascript")
        .args(["-e", &script])
        .output()
        .map_err(|e| format!("cannot ask for the administrator password: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

#[cfg(not(target_os = "macos"))]
fn as_administrator(_exe: &Path, _arg: &str) -> Result<(), String> {
    Err("the login-screen service exists on macOS only".into())
}

#[cfg(target_os = "macos")]
fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_agent_runs_the_service_in_the_login_window() {
        let text = plist_text(Path::new("/Applications/SwitchControl.app/Contents/MacOS/Sw&tch"));
        assert!(text.contains("<string>LoginWindow</string>"));
        assert!(text.contains("<string>--service</string>"));
        // The program path is escaped for XML.
        assert!(text.contains("/Applications/SwitchControl.app/Contents/MacOS/Sw&amp;tch"));
        assert!(text.contains(LOG) && text.contains(LABEL));
    }

    #[test]
    fn only_the_applications_folder_counts() {
        assert!(in_applications(Path::new("/Applications/SwitchControl.app/Contents/MacOS/x")));
        assert!(!in_applications(Path::new("/Users/me/Downloads/SwitchControl.app/x")));
    }
}
