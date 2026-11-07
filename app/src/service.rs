//! `--service`: the copy that runs in the background at the macOS login
//! screen (see [`crate::loginwindow`]).
//!
//! It only receives: no keyboard or mouse capture, no pairing (nobody can
//! read or type a PIN there), no clipboard. Settings, identity and paired
//! computers come from the folder shared with the window program, so the
//! other computer sees the same certificate it already trusts. Logs go to
//! standard output, which launchd writes to [`crate::loginwindow::LOG`].

use std::path::Path;

use crate::config::{self, RunFlags, SETTINGS_FILE, Settings};
use crate::identity;

/// The service's settings: what the window saved in the shared folder.
pub fn settings() -> Result<Settings, String> {
    settings_in(Path::new(identity::SHARED_DIR))
}

fn settings_in(dir: &Path) -> Result<Settings, String> {
    let file = config::load(&dir.join(SETTINGS_FILE)).map_err(|e| {
        format!(
            "no saved settings ({e}); open the SwitchControl window once and start sharing, \
             after installing the login-screen service"
        )
    })?;
    let mut settings = config::resolve(file, RunFlags::default())?;
    settings.data_dir = dir.to_path_buf();
    settings.no_capture = true;
    settings.no_pairing = true;
    settings.clipboard = false;
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_service_only_receives() {
        let dir = std::env::temp_dir().join(format!("sc-service-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(settings_in(&dir).unwrap_err().contains("open the SwitchControl window"));

        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(SETTINGS_FILE),
            "name = \"Mac mini\"\nside = \"left\"\nclipboard = true\n",
        )
        .unwrap();
        let settings = settings_in(&dir).unwrap();
        assert_eq!(settings.name, "Mac mini");
        assert!(settings.no_capture && settings.no_pairing && !settings.clipboard);
        assert_eq!(settings.data_dir, dir);
        let _ = std::fs::remove_dir_all(dir);
    }
}
