//! Phase 0 spike: prove that Windows input capture and injection work.
//!
//!   win-spike screens
//!   win-spike capture [--seconds N] [--no-moves] [--grab-after S --grab-for S]
//!   win-spike inject [--delay S] [--type]
//!   win-spike selftest
//!   win-spike edge-test     (drives a running switchcontrol across the right edge)

#[cfg(not(windows))]
fn main() {
    eprintln!("win-spike only runs on Windows");
    std::process::exit(1);
}

#[cfg(windows)]
fn main() {
    sc_platform::win::init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("screens") => {
            screens();
            Ok(())
        }
        Some("capture") => capture(&args[1..]),
        Some("inject") => inject(&args[1..]),
        Some("selftest") => selftest(),
        Some("edge-test") => edge_test(),
        _ => {
            eprintln!(
                "usage:\n  win-spike screens\n  win-spike capture [--seconds N] [--no-moves] [--grab-after S --grab-for S]\n  win-spike inject [--delay S] [--type]\n  win-spike selftest\n  win-spike edge-test"
            );
            std::process::exit(2);
        }
    };
    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

#[cfg(windows)]
use windows_impl::*;

#[cfg(windows)]
mod windows_impl {
    use std::sync::mpsc::{Receiver, RecvTimeoutError};
    use std::thread::sleep;
    use std::time::{Duration, Instant};

    use sc_platform::win::{
        Capture, Injector, cursor_image, cursor_pos, monitors, virtual_desktop,
    };
    use sc_platform::{CaptureEvent, InputEvent};

    type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

    /// HID usage of Left Shift: harmless to tap in any window.
    const LEFT_SHIFT: u16 = 0xE1;

    pub fn screens() {
        let desktop = virtual_desktop();
        println!(
            "virtual desktop: ({}, {}) .. ({}, {})  {}x{}",
            desktop.left,
            desktop.top,
            desktop.right,
            desktop.bottom,
            desktop.width(),
            desktop.height()
        );
        for m in monitors() {
            let b = m.bounds;
            println!(
                "{}{}: ({}, {}) .. ({}, {})  {}x{}",
                m.name,
                if m.primary { " [primary]" } else { "" },
                b.left,
                b.top,
                b.right,
                b.bottom,
                b.width(),
                b.height()
            );
        }
        if let Some((x, y)) = cursor_pos() {
            println!("cursor: ({x}, {y})");
        }
    }

    pub fn capture(args: &[String]) -> Result<()> {
        let seconds = flag_value(args, "--seconds")?;
        let grab_after = flag_value(args, "--grab-after")?;
        let grab_for = flag_value(args, "--grab-for")?.unwrap_or(5.0);
        let show_moves = !args.iter().any(|a| a == "--no-moves");

        let (capture, rx) = Capture::start()?;
        let start = Instant::now();
        let stop_at = seconds.map(Duration::from_secs_f64);
        let grab_on_at = grab_after.map(Duration::from_secs_f64);
        let grab_off_at = grab_on_at.map(|t| t + Duration::from_secs_f64(grab_for));
        let mut grab_done = false;
        let mut counts = Counts::default();

        println!(
            "capturing{}; Ctrl+C to stop",
            match seconds {
                Some(s) => format!(" for {s}s"),
                None => String::new(),
            }
        );

        loop {
            let now = start.elapsed();
            if stop_at.is_some_and(|t| now >= t) {
                break;
            }
            if !grab_done && !capture.is_grabbing() && grab_on_at.is_some_and(|t| now >= t) {
                capture.set_grab(true);
                println!(
                    "[{:>7.3}] GRAB ON for {grab_for}s: local input is swallowed (Ctrl+Alt+Shift+Esc releases)",
                    now.as_secs_f64()
                );
                grab_done = true;
            }
            if capture.is_grabbing() && grab_off_at.is_some_and(|t| now >= t) {
                capture.set_grab(false);
                println!("[{:>7.3}] GRAB OFF (timer)", now.as_secs_f64());
            }

            match rx.recv_timeout(Duration::from_millis(20)) {
                Ok(ev) => {
                    counts.add(&ev);
                    let moving = matches!(
                        ev,
                        CaptureEvent::Input(
                            InputEvent::MouseMove { .. } | InputEvent::MouseDelta { .. }
                        )
                    );
                    if show_moves || !moving {
                        println!("[{:>7.3}] {}", start.elapsed().as_secs_f64(), describe(&ev));
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return Err("capture thread stopped".into()),
            }
        }
        drop(capture);
        println!("{counts}");
        Ok(())
    }

    pub fn inject(args: &[String]) -> Result<()> {
        let delay = flag_value(args, "--delay")?.unwrap_or(3.0);
        let type_text = args.iter().any(|a| a == "--type");

        if type_text {
            println!("focus a text editor (e.g. Notepad); typing starts in {delay}s");
        } else {
            println!("mouse square starts in {delay}s");
        }
        sleep(Duration::from_secs_f64(delay));

        let mut injector = Injector::new();
        let origin = mouse_square(&mut injector)?;
        println!("mouse square done, cursor back at {origin:?}");

        if type_text {
            // Physical keys (HID usages): "hello " then the four keys right of P / L.
            // US layout prints "hello [];'", Turkish Q prints "hello ğüşi".
            let keys: &[u16] = &[0x0B, 0x08, 0x0F, 0x0F, 0x12, 0x2C, 0x2F, 0x30, 0x33, 0x34];
            for &usage in keys {
                tap(&mut injector, usage)?;
                sleep(Duration::from_millis(30));
            }
            tap(&mut injector, 0x28)?; // Enter
            println!("typed test keys (US: \"hello [];'\", TR-Q: \"hello ğüşi\")");
        }
        Ok(())
    }

    /// Injects events and checks them against what the hooks see: untagged
    /// injections must be captured, tagged ones (our marker) must be ignored.
    pub fn selftest() -> Result<()> {
        let (capture, rx) = Capture::start()?;
        let mut failures = 0;

        println!("1) untagged injection must be captured");
        let mut untagged = Injector::untagged();
        drain(&rx);
        let targets = square_targets();
        let origin = mouse_square(&mut untagged)?;
        tap(&mut untagged, LEFT_SHIFT)?;
        let seen = collect(&rx, Duration::from_millis(300));
        for &(x, y) in &targets {
            let hit = seen.iter().any(|ev| {
                matches!(ev, CaptureEvent::Input(InputEvent::MouseMove { x: cx, y: cy })
                    if (cx - x).abs() <= 2 && (cy - y).abs() <= 2)
            });
            failures += check(hit, &format!("MouseMove near ({x}, {y}) captured"));
        }
        failures += check(has_key(&seen, LEFT_SHIFT, true), "Shift down captured");
        failures += check(has_key(&seen, LEFT_SHIFT, false), "Shift up captured");

        println!("2) tagged injection must be ignored");
        let mut tagged = Injector::new();
        drain(&rx);
        mouse_square(&mut tagged)?;
        tap(&mut tagged, LEFT_SHIFT)?;
        let seen = collect(&rx, Duration::from_millis(300));
        let leaked = seen
            .iter()
            .filter(|ev| {
                matches!(ev, CaptureEvent::Input(InputEvent::MouseMove { x, y })
                    if targets.iter().any(|&(tx, ty)| (x - tx).abs() <= 2 && (y - ty).abs() <= 2))
            })
            .count();
        failures += check(leaked == 0, &format!("no tagged MouseMove captured ({leaked} leaked)"));
        failures += check(!has_key(&seen, LEFT_SHIFT, true), "tagged Shift not captured");

        println!("3) grab parks and hides the cursor and reports deltas");
        let image_before = cursor_image();
        capture.set_grab(true);
        sleep(Duration::from_millis(150));
        drain(&rx);
        let image_grabbed = cursor_image();
        let park = cursor_pos().ok_or("GetCursorPos failed")?;
        untagged.inject(&InputEvent::MouseMove { x: park.0 + 50, y: park.1 - 20 })?;
        sleep(Duration::from_millis(150));
        let after = cursor_pos().ok_or("GetCursorPos failed")?;
        capture.set_grab(false);
        let seen = collect(&rx, Duration::from_millis(100));
        let delta = seen.iter().any(|ev| {
            matches!(ev, CaptureEvent::Input(InputEvent::MouseDelta { dx, dy })
                if (dx - 50).abs() <= 1 && (dy + 20).abs() <= 1)
        });
        failures += check(delta, "MouseDelta (50, -20) captured");
        failures +=
            check(after == park, &format!("cursor stayed parked at {park:?} (now {after:?})"));
        failures += check(
            image_grabbed.is_some() && image_grabbed != image_before,
            &format!("cursor image swapped while grabbing ({image_before:?} -> {image_grabbed:?})"),
        );

        Injector::new().inject(&InputEvent::MouseMove { x: origin.0, y: origin.1 })?;
        sleep(Duration::from_millis(100));
        let image_after = cursor_image();
        failures += check(
            image_after != image_grabbed,
            &format!("cursor image restored after grab ({image_after:?})"),
        );
        drop(capture);

        if failures == 0 {
            println!("selftest PASSED");
            Ok(())
        } else {
            Err(format!("selftest FAILED ({failures} checks)").into())
        }
    }

    /// Drives a running `switchcontrol` (peer on the right) with untagged
    /// input: across the right edge, a little way into the peer, and back.
    pub fn edge_test() -> Result<()> {
        let desktop = virtual_desktop();
        let origin = cursor_pos().ok_or("GetCursorPos failed")?;
        let y = desktop.top + desktop.height() / 2;
        let mut input = Injector::untagged();

        println!("moving to the right edge at y={y}");
        input.inject(&InputEvent::MouseMove { x: desktop.right - 1, y })?;
        sleep(Duration::from_millis(400));

        let park = cursor_pos().ok_or("GetCursorPos failed")?;
        println!("cursor parked at {park:?}; moving 5 x 100px into the peer");
        for _ in 0..5 {
            input.inject(&InputEvent::MouseMove { x: park.0 + 100, y: park.1 })?;
            sleep(Duration::from_millis(60));
        }
        println!("moving back in 300px steps until the cursor is released");
        for _ in 0..10 {
            input.inject(&InputEvent::MouseMove { x: park.0 - 300, y: park.1 })?;
            sleep(Duration::from_millis(60));
            // Once control is back, further moves would land on the local screen.
            if cursor_pos() != Some(park) {
                break;
            }
        }
        sleep(Duration::from_millis(200));

        let back = cursor_pos().ok_or("GetCursorPos failed")?;
        let expected_x = desktop.right - 2;
        Injector::new().inject(&InputEvent::MouseMove { x: origin.0, y: origin.1 })?;
        if back.0 == expected_x {
            println!("edge-test PASSED: control came back, cursor at {back:?}");
            Ok(())
        } else {
            Err(format!("edge-test FAILED: cursor at {back:?}, expected x={expected_x}").into())
        }
    }

    /// Corners of a 200px square starting at the cursor, kept on-screen.
    fn square_targets() -> Vec<(i32, i32)> {
        let desktop = virtual_desktop();
        let (x, y) = cursor_pos().unwrap_or((desktop.left + 300, desktop.top + 300));
        let x = x.clamp(desktop.left, desktop.right - 201);
        let y = y.clamp(desktop.top, desktop.bottom - 201);
        vec![(x + 200, y), (x + 200, y + 200), (x, y + 200), (x, y)]
    }

    /// Moves the cursor around a square and back; returns the starting point.
    fn mouse_square(injector: &mut Injector) -> Result<(i32, i32)> {
        let origin = cursor_pos().ok_or("GetCursorPos failed")?;
        for (x, y) in square_targets() {
            injector.inject(&InputEvent::MouseMove { x, y })?;
            sleep(Duration::from_millis(80));
        }
        injector.inject(&InputEvent::MouseMove { x: origin.0, y: origin.1 })?;
        Ok(origin)
    }

    fn tap(injector: &mut Injector, usage: u16) -> Result<()> {
        injector.inject(&InputEvent::Key { usage, down: true })?;
        injector.inject(&InputEvent::Key { usage, down: false })?;
        Ok(())
    }

    fn has_key(events: &[CaptureEvent], usage: u16, down: bool) -> bool {
        events.contains(&CaptureEvent::Input(InputEvent::Key { usage, down }))
    }

    fn drain(rx: &Receiver<CaptureEvent>) {
        sleep(Duration::from_millis(50));
        while rx.try_recv().is_ok() {}
    }

    fn collect(rx: &Receiver<CaptureEvent>, settle: Duration) -> Vec<CaptureEvent> {
        sleep(settle);
        rx.try_iter().collect()
    }

    fn check(ok: bool, what: &str) -> usize {
        println!("   [{}] {what}", if ok { "ok" } else { "FAIL" });
        usize::from(!ok)
    }

    fn describe(ev: &CaptureEvent) -> String {
        match ev {
            CaptureEvent::EmergencyRelease => "EMERGENCY RELEASE (grab ended)".into(),
            CaptureEvent::Input(InputEvent::Key { usage, down }) => {
                format!("key   {usage:#04x} {}", if *down { "down" } else { "up" })
            }
            CaptureEvent::Input(other) => format!("{other:?}"),
        }
    }

    fn flag_value(args: &[String], name: &str) -> Result<Option<f64>> {
        match args.iter().position(|a| a == name) {
            None => Ok(None),
            Some(i) => {
                let raw = args.get(i + 1).ok_or_else(|| format!("{name} needs a value"))?;
                Ok(Some(raw.parse().map_err(|_| format!("{name}: not a number: {raw}"))?))
            }
        }
    }

    #[derive(Default)]
    struct Counts {
        moves: usize,
        deltas: usize,
        buttons: usize,
        wheels: usize,
        keys: usize,
    }

    impl Counts {
        fn add(&mut self, ev: &CaptureEvent) {
            match ev {
                CaptureEvent::Input(InputEvent::MouseMove { .. }) => self.moves += 1,
                CaptureEvent::Input(InputEvent::MouseDelta { .. }) => self.deltas += 1,
                CaptureEvent::Input(InputEvent::Button { .. }) => self.buttons += 1,
                CaptureEvent::Input(InputEvent::Wheel { .. }) => self.wheels += 1,
                CaptureEvent::Input(InputEvent::Key { .. }) => self.keys += 1,
                CaptureEvent::EmergencyRelease => {}
            }
        }
    }

    impl std::fmt::Display for Counts {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(
                f,
                "captured: {} moves, {} grab deltas, {} buttons, {} wheel, {} keys",
                self.moves, self.deltas, self.buttons, self.wheels, self.keys
            )
        }
    }
}
