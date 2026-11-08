//! Which of the two window sizes fits the screen.
//!
//! The window does not resize by hand: it is either the roomy standard one or
//! the tight compact one, chosen from the primary screen's height in logical
//! pixels (what it looks like after display scaling) unless the user picks.

use sc_platform::sys;

use crate::theme::Density;

/// Logical size of the standard window.
pub const STANDARD: (f32, f32) = (460.0, 860.0);
/// Logical size of the compact window.
pub const COMPACT: (f32, f32) = (420.0, 640.0);
/// Screens at least this tall get the standard window: it needs the screen
/// height minus the taskbar or menu bar and dock, and the title bar.
const STANDARD_MIN_SCREEN_H: f32 = 1000.0;
/// Room kept free on a small screen for the taskbar or dock and the title bar.
const SCREEN_MARGIN: f32 = 90.0;

/// What the user asked for in the settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pref {
    #[default]
    Auto,
    Compact,
    Standard,
}

impl Pref {
    pub fn parse(text: Option<&str>) -> Pref {
        match text {
            Some("compact") => Pref::Compact,
            Some("standard") => Pref::Standard,
            _ => Pref::Auto,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Pref::Auto => "auto",
            Pref::Compact => "compact",
            Pref::Standard => "standard",
        }
    }
}

/// The density for a screen `screen_h` logical pixels tall.
pub fn density(pref: Pref, screen_h: f32) -> Density {
    match pref {
        Pref::Compact => Density::Compact,
        Pref::Standard => Density::Standard,
        Pref::Auto if screen_h >= STANDARD_MIN_SCREEN_H => Density::Standard,
        Pref::Auto => Density::Compact,
    }
}

/// The window size for a density on a screen `screen_h` tall, never taller
/// than the screen leaves room for.
pub fn size(density: Density, screen_h: f32) -> (f32, f32) {
    let (w, h) = match density {
        Density::Standard => STANDARD,
        Density::Compact => COMPACT,
    };
    (w, h.min((screen_h - SCREEN_MARGIN).max(480.0)))
}

/// Logical size of the primary screen: what a window of this size has to fit
/// into. Physical pixels divided by the display scaling on Windows; macOS
/// already counts in points.
pub fn primary_screen() -> (f32, f32) {
    // First: on Windows this also makes the process DPI aware, so the monitors
    // below are measured in physical pixels.
    let scale = sys::screen_scale().max(1.0);
    let monitors = sys::monitors();
    let primary = monitors.iter().find(|m| m.primary).or(monitors.first());
    match primary {
        Some(m) => (m.bounds.width() as f32 / scale, m.bounds.height() as f32 / scale),
        None => (1920.0, 1080.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_follows_the_logical_screen_height() {
        // 1366x768 at 100 %, and 1080p at 150 % (720 logical).
        for h in [768.0, 720.0, 864.0] {
            assert_eq!(density(Pref::Auto, h), Density::Compact, "{h}");
        }
        // 1080p, 1440p at 125 %, 4K at 150 %.
        for h in [1080.0, 1152.0, 1440.0] {
            assert_eq!(density(Pref::Auto, h), Density::Standard, "{h}");
        }
    }

    #[test]
    fn the_choice_beats_the_screen() {
        assert_eq!(density(Pref::Compact, 2160.0), Density::Compact);
        assert_eq!(density(Pref::Standard, 600.0), Density::Standard);
    }

    #[test]
    fn the_window_fits_the_screen() {
        assert_eq!(size(Density::Standard, 1080.0), STANDARD);
        assert_eq!(size(Density::Compact, 768.0), COMPACT);
        // A very small screen shrinks the window instead of cutting it off.
        let (_, h) = size(Density::Compact, 600.0);
        assert!(h <= 600.0 - SCREEN_MARGIN);
        assert!(h >= 480.0);
    }

    #[test]
    fn preferences_round_trip() {
        for pref in [Pref::Auto, Pref::Compact, Pref::Standard] {
            assert_eq!(Pref::parse(Some(pref.as_str())), pref);
        }
        assert_eq!(Pref::parse(None), Pref::Auto);
        assert_eq!(Pref::parse(Some("huge")), Pref::Auto);
    }
}
