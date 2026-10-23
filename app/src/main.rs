//! SwitchControl: share one keyboard and mouse between two computers.
//!
//! Run it on both machines; each one says where the other sits. One side
//! listens and the other connects. Moving the cursor off the shared edge
//! hands keyboard and mouse to the other machine.
//!
//! Without arguments (e.g. started from Finder) it opens a window; with
//! arguments it runs in the terminal.

mod config;
mod edgebar;
mod engine;
mod gui;
mod identity;
mod keyguard;
mod net;
mod pairing;
mod theme;
mod tls;

use std::process::ExitCode;
use std::sync::{Arc, mpsc};
use std::time::Instant;

use engine::{Engine, Report};
use net::PinSource;

fn main() -> ExitCode {
    // Old macOS versions pass a process serial number to apps started from Finder.
    let args: Vec<String> = std::env::args().skip(1).filter(|a| !a.starts_with("-psn_")).collect();
    if args.is_empty() || args == ["--gui"] {
        return match gui::run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::FAILURE
            }
        };
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
