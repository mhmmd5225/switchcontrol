use sc_platform::{CaptureEvent, sys};
use sc_proto::InputEvent;
use std::thread::sleep;
use std::time::{Duration, Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    sys::init();
    let monitors = sys::monitors();
    let screen = monitors.iter().find(|monitor| monitor.primary).ok_or("No primary monitor")?;
    println!("Primary monitor {:?}, scale {}%", screen.bounds, screen.scale_percent);
    let (capture, input) = sys::Capture::start()?;
    let mut source = sys::Injector::untagged();
    capture.set_grab(true);
    sleep(Duration::from_millis(500));
    let park = sys::cursor_pos().ok_or("No cursor position")?;
    let center = (
        screen.bounds.left + screen.bounds.width() / 2,
        screen.bounds.top + screen.bounds.height() / 2,
    );
    if (park.0 - center.0).abs() > 2 || (park.1 - center.1).abs() > 2 {
        return Err(format!("Cursor parked at {park:?}, expected {center:?}").into());
    }
    let _ = input.try_iter().count();

    // Synthetic input exercises actual Win32 capture and injection in the VM.
    let shift_down = InputEvent::Key { usage: 0xE1, down: true };
    let shift_up = InputEvent::Key { usage: 0xE1, down: false };
    let vertical = InputEvent::Wheel { dx: 0, dy: 120 };
    let horizontal = InputEvent::Wheel { dx: -120, dy: 0 };
    for event in [
        shift_down,
        shift_up,
        vertical,
        horizontal,
        InputEvent::MouseMove { x: park.0 + 50, y: park.1 - 20 },
    ] {
        source.inject(&event)?;
    }

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut received = Vec::new();
    while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
        match input.recv_timeout(remaining) {
            Ok(CaptureEvent::Input(event)) => received.push(event),
            _ => break,
        }
    }
    let moved = received.iter().any(|event| {
        matches!(event, InputEvent::MouseDelta { dx, dy } if (dx - 50).abs() <= 1 && (dy + 20).abs() <= 1)
    });
    capture.set_grab(false);
    for expected in [shift_down, shift_up, vertical, horizontal] {
        if !received.contains(&expected) {
            return Err(format!("Missing {expected:?}; captured {received:?}").into());
        }
    }
    if !moved {
        return Err(format!("Missing cursor movement (50,-20); captured {received:?}").into());
    }
    println!("PASS: keyboard, signed horizontal/vertical wheel, and parked pointer deltas");
    Ok(())
}
