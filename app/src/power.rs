//! Putting this computer to sleep or shutting it down when the other one
//! asks. Shutting down goes through the system's normal path, so apps with
//! unsaved work can still stop it.

use std::process::Command;

use sc_proto::{Os, Power};

/// What a computer running this system can be asked to do.
pub fn supported(os: Os, power: Power) -> bool {
    matches!((os, power), (Os::MacOs, _) | (Os::Windows, Power::Shutdown))
}

pub fn run(power: Power) -> Result<(), String> {
    if !supported(Os::current(), power) {
        return Err(format!("{power:?} is not supported on this computer"));
    }
    let mut command = command(power);
    let output = command.output().map_err(|e| format!("cannot run {command:?}: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{command:?} failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

#[cfg(target_os = "macos")]
fn command(power: Power) -> Command {
    match power {
        Power::Sleep => {
            let mut c = Command::new("/usr/bin/pmset");
            c.arg("sleepnow");
            c
        }
        // The first time, macOS asks whether SwitchControl may control System Events.
        Power::Shutdown => {
            let mut c = Command::new("/usr/bin/osascript");
            c.args(["-e", "tell application \"System Events\" to shut down"]);
            c
        }
    }
}

#[cfg(windows)]
fn command(_: Power) -> Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    // Without /f, so apps with unsaved work can still hold it up.
    let mut c = Command::new("shutdown");
    c.args(["/s", "/t", "0"]).creation_flags(CREATE_NO_WINDOW);
    c
}

#[cfg(not(any(windows, target_os = "macos")))]
fn command(_: Power) -> Command {
    unreachable!("nothing is supported here")
}
