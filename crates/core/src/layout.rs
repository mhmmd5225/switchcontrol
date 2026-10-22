//! Screen geometry: which edge leads to the peer and how positions map across.

use sc_proto::Rect;

/// Where the peer's screens sit relative to ours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

impl Side {
    pub fn opposite(self) -> Side {
        match self {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
        }
    }
}

/// The outermost pixel column on `side` (inclusive).
pub fn edge_x(screens: &[Rect], side: Side) -> Option<i32> {
    match side {
        Side::Left => screens.iter().map(|r| r.left).min(),
        Side::Right => screens.iter().map(|r| r.right - 1).max(),
    }
}

/// Monitors that reach the outer edge on `side`.
fn edge_screens(screens: &[Rect], side: Side) -> impl Iterator<Item = &Rect> {
    let edge = edge_x(screens, side);
    screens.iter().filter(move |r| {
        Some(match side {
            Side::Left => r.left,
            Side::Right => r.right - 1,
        }) == edge
    })
}

/// Vertical extent `(top, bottom)` covered by the monitors on the outer edge.
pub fn edge_span(screens: &[Rect], side: Side) -> Option<(i32, i32)> {
    let top = edge_screens(screens, side).map(|r| r.top).min()?;
    let bottom = edge_screens(screens, side).map(|r| r.bottom).max()?;
    Some((top, bottom))
}

/// Whether (x, y) is at or past the outer edge on `side`, at a height where a
/// monitor actually reaches that edge.
pub fn on_edge(screens: &[Rect], side: Side, x: i32, y: i32) -> bool {
    let Some(edge) = edge_x(screens, side) else { return false };
    let at_edge = match side {
        Side::Left => x <= edge,
        Side::Right => x >= edge,
    };
    at_edge && edge_screens(screens, side).any(|r| r.top <= y && y < r.bottom)
}

/// Maps `y` from one vertical span onto another, keeping its relative height.
pub fn map_span(y: i32, from: (i32, i32), to: (i32, i32)) -> i32 {
    let frac = (y - from.0) as f64 / (from.1 - from.0).max(1) as f64;
    let mapped = to.0 + (frac * (to.1 - to.0) as f64) as i32;
    mapped.clamp(to.0, to.1 - 1)
}

/// The closest point to (x, y) that lies on one of the screens.
pub fn clamp_to_screens(screens: &[Rect], x: f64, y: f64) -> (f64, f64) {
    let mut best = (x, y);
    let mut best_dist = f64::INFINITY;
    for r in screens {
        let cx = x.clamp(r.left as f64, (r.right - 1) as f64);
        let cy = y.clamp(r.top as f64, (r.bottom - 1) as f64);
        let dist = (cx - x).powi(2) + (cy - y).powi(2);
        if dist < best_dist {
            best = (cx, cy);
            best_dist = dist;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn rect(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
        Rect { left, top, right, bottom }
    }

    #[test]
    fn single_monitor_edges() {
        let screens = [rect(0, 0, 2560, 1440)];
        assert_eq!(edge_x(&screens, Side::Right), Some(2559));
        assert_eq!(edge_x(&screens, Side::Left), Some(0));
        assert!(on_edge(&screens, Side::Right, 2559, 700));
        assert!(!on_edge(&screens, Side::Right, 2558, 700));
        assert!(on_edge(&screens, Side::Left, 0, 0));
    }

    #[test]
    fn edge_ignores_heights_without_a_monitor() {
        // A short monitor to the right of a tall one.
        let screens = [rect(0, 0, 1920, 1080), rect(1920, 0, 3840, 600)];
        assert_eq!(edge_x(&screens, Side::Right), Some(3839));
        assert!(on_edge(&screens, Side::Right, 3839, 599));
        assert!(!on_edge(&screens, Side::Right, 3839, 600));
        assert_eq!(edge_span(&screens, Side::Right), Some((0, 600)));
        assert_eq!(edge_span(&screens, Side::Left), Some((0, 1080)));
    }

    #[test]
    fn span_mapping_keeps_relative_height() {
        assert_eq!(map_span(720, (0, 1440), (0, 982)), 491);
        assert_eq!(map_span(0, (0, 1440), (0, 982)), 0);
        assert_eq!(map_span(1439, (0, 1440), (0, 982)), 981);
        assert_eq!(map_span(50, (0, 100), (-200, 0)), -100);
    }

    #[test]
    fn clamping_finds_the_nearest_screen() {
        let screens = [rect(0, 0, 1920, 1080), rect(1920, 0, 3840, 600)];
        assert_eq!(clamp_to_screens(&screens, 100.0, 100.0), (100.0, 100.0));
        assert_eq!(clamp_to_screens(&screens, -50.0, 500.0), (0.0, 500.0));
        // Below the short monitor: pulled up onto it, not across to the tall one.
        assert_eq!(clamp_to_screens(&screens, 3000.0, 650.0), (3000.0, 599.0));
        assert_eq!(clamp_to_screens(&screens, 5000.0, 5000.0), (3839.0, 599.0));
    }
}
