//! SwitchControl: share one keyboard and mouse between two computers.
//!
//! Run it on both machines; each one says where the other sits. One side
//! listens and the other connects. Moving the cursor off the shared edge
//! hands keyboard and mouse to the other machine.
//!
//! Without arguments (e.g. started from Finder) it opens a window; with
//! arguments it runs in the terminal.

// On Windows a window program, so double-clicking it opens no console.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod autostart;
mod clipboard;
mod config;
mod discovery;
mod edgebar;
mod engine;
mod gui;
mod identity;
mod keyguard;
mod loginwindow;
mod net;
mod pairing;
mod power;
mod service;
mod theme;
mod tls;
mod tray;
mod wake;
mod window;

use std::process::ExitCode;
use std::sync::{Arc, mpsc};
use std::time::Instant;

use engine::{Engine, Report};
use net::PinSource;

fn main() -> ExitCode {
    // Old macOS versions pass a process serial number to apps started from Finder.
    let args: Vec<String> = std::env::args().skip(1).filter(|a| !a.starts_with("-psn_")).collect();
    let at_login = args == [autostart::ARG];
    if args.is_empty() || args == ["--gui"] || at_login {
        return match gui::run(at_login) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::FAILURE
            }
        };
    }

    attach_console();
    if let [flag, mac] = args.as_slice()
        && flag == "--wake"
    {
        return wake(mac);
    }
    match args.as_slice() {
        [arg] if arg == loginwindow::SERVICE_ARG => return run_service(),
        [arg] if arg == loginwindow::STATUS_ARG => return service_status(),
        [arg] if arg == loginwindow::INSTALL_ARG => {
            let exe = std::env::current_exe().unwrap_or_default();
            return finish(loginwindow::install_as_root(&exe));
        }
        [arg] if arg == loginwindow::UNINSTALL_ARG => {
            return finish(loginwindow::uninstall_as_root());
        }
        _ => {}
    }
    let settings = match config::from_args(&args) {
        Ok(settings) => settings,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("error: {e}\n");
            }
            eprintln!("{}", config::USAGE);
            return ExitCode::from(2);
        }
    };
    let pin = match settings.pin.clone() {
        Some(pin) => PinSource::Fixed(pin),
        None => PinSource::Console,
    };
    run_terminal(settings, pin)
}

/// Runs the engine with its log on standard output until it ends.
fn run_terminal(settings: config::Settings, pin: PinSource) -> ExitCode {
    let (tx, rx) = mpsc::channel();
    let engine = Engine::start(settings, pin, Arc::new(move |r| drop(tx.send(r))));
    let start = Instant::now();
    let line = |text: &str| println!("[{:>8.3}] {text}", start.elapsed().as_secs_f64());
    for report in rx {
        match report {
            Report::Log(text) => line(&text),
            Report::Stopped(Ok(())) => break,
            Report::Stopped(Err(e)) => {
                engine.wait();
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            }
            // The log already says all this in the terminal.
            Report::Link(_)
            | Report::Remote(_)
            | Report::Edge(_)
            | Report::ShowPin(_)
            | Report::AskPin(_) => {}
        }
    }
    engine.wait();
    ExitCode::SUCCESS
}

/// The background copy at the macOS login screen: launchd runs it there.
fn run_service() -> ExitCode {
    match service::settings() {
        // Nobody can type a PIN there: a computer must be paired already.
        Ok(settings) => run_terminal(settings, PinSource::Ask(Box::new(|_| None))),
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn service_status() -> ExitCode {
    let state = if loginwindow::is_installed() { "installed" } else { "not installed" };
    println!("login-screen service: {state} ({})", loginwindow::PLIST);
    println!("log: {}", loginwindow::LOG);
    println!("shared data folder: {}", identity::SHARED_DIR);
    ExitCode::SUCCESS
}

fn finish(result: Result<(), String>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn wake(text: &str) -> ExitCode {
    let Some(mac) = wake::parse_mac(text) else {
        eprintln!("error: --wake: not a network card address: {text}");
        return ExitCode::from(2);
    };
    match wake::send(mac) {
        Ok(()) => {
            println!("Wake-on-LAN packet sent to {}", wake::format_mac(mac));
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: cannot send the Wake-on-LAN packet: {e}");
            ExitCode::FAILURE
        }
    }
}

/// On Windows the program has no console of its own: in the terminal it
/// writes to the one it was started from, or opens one if there is none.
#[cfg(windows)]
fn attach_console() {
    use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AllocConsole, AttachConsole};
    unsafe {
        if AttachConsole(ATTACH_PARENT_PROCESS).is_err() {
            let _ = AllocConsole();
        }
    }
}

#[cfg(not(windows))]
fn attach_console() {}
