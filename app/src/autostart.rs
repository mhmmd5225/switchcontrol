//! Starting SwitchControl when the user logs in.
//!
//! The entry runs the program with [`ARG`], which opens the window hidden in
//! the tray and starts sharing with the saved settings.
//!
//! - Windows: a value under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.
//! - macOS: a LaunchAgent in `~/Library/LaunchAgents` that opens the app.
//! - Linux: a `.desktop` file in `~/.config/autostart`.

use std::path::PathBuf;

/// Command-line flag of a launch at login.
pub const ARG: &str = "--autostart";

fn exe() -> Result<PathBuf, String> {
    std::env::current_exe().map_err(|e| format!("cannot find the program file: {e}"))
}

#[cfg(windows)]
mod imp {
    use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
    use windows::Win32::System::Registry::{
        HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
    };
    use windows::core::w;

    use super::{ARG, exe};

    const RUN_KEY: windows::core::PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
    const NAME: windows::core::PCWSTR = w!("SwitchControl");

    pub fn is_enabled() -> bool {
        let mut size = 0u32;
        unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                RUN_KEY,
                NAME,
                RRF_RT_REG_SZ,
                None,
                None,
                Some(&mut size),
            )
        }
        .is_ok()
    }

    pub fn set(enabled: bool) -> Result<(), String> {
        let result = if enabled {
            let command = format!("\"{}\" {ARG}", exe()?.display());
            let wide: Vec<u16> = command.encode_utf16().chain(Some(0)).collect();
            unsafe {
                RegSetKeyValueW(
                    HKEY_CURRENT_USER,
                    RUN_KEY,
                    NAME,
                    REG_SZ.0,
                    Some(wide.as_ptr().cast()),
                    (wide.len() * 2) as u32,
                )
            }
        } else {
            let result = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, NAME) };
            if result == ERROR_FILE_NOT_FOUND {
                return Ok(());
            }
            result
        };
        result.ok().map_err(|e| e.to_string())
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::path::{Path, PathBuf};

    use super::{ARG, exe};

    const LABEL: &str = "com.switchcontrol.app";

    fn plist() -> PathBuf {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        home.join("Library/LaunchAgents").join(format!("{LABEL}.plist"))
    }

    /// The `.app` the program runs from, if any.
    fn bundle(exe: &Path) -> Option<&Path> {
        let app = exe.parent()?.parent()?.parent()?;
        (app.extension()? == "app").then_some(app)
    }

    fn escape(text: &str) -> String {
        text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
    }

    pub fn is_enabled() -> bool {
        plist().exists()
    }

    pub fn set(enabled: bool) -> Result<(), String> {
        let path = plist();
        if !enabled {
            return match std::fs::remove_file(&path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
                _ => Ok(()),
            };
        }
        let exe = exe()?;
        // Through `open`, so macOS starts the app as an app (Dock, permissions).
        let args: Vec<String> = match bundle(&exe) {
            Some(app) => {
                vec!["/usr/bin/open".into(), app.display().to_string(), "--args".into(), ARG.into()]
            }
            None => vec![exe.display().to_string(), ARG.into()],
        };
        let args: String =
            args.iter().map(|a| format!("        <string>{}</string>\n", escape(a))).collect();
        let text = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
             \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\">\n<dict>\n\
             \x20   <key>Label</key>\n    <string>{LABEL}</string>\n\
             \x20   <key>ProgramArguments</key>\n    <array>\n{args}    </array>\n\
             \x20   <key>RunAtLoad</key>\n    <true/>\n\
             </dict>\n</plist>\n"
        );
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, text).map_err(|e| e.to_string())
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    use std::path::PathBuf;

    use super::{ARG, exe};

    fn desktop_file() -> PathBuf {
        let config = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| {
            std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join(".config")
        });
        config.join("autostart/switchcontrol.desktop")
    }

    pub fn is_enabled() -> bool {
        desktop_file().exists()
    }

    pub fn set(enabled: bool) -> Result<(), String> {
        let path = desktop_file();
        if !enabled {
            return match std::fs::remove_file(&path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
                _ => Ok(()),
            };
        }
        let text = format!(
            "[Desktop Entry]\nType=Application\nName=SwitchControl\nExec=\"{}\" {ARG}\n",
            exe()?.display()
        );
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, text).map_err(|e| e.to_string())
    }
}

pub use imp::{is_enabled, set};
