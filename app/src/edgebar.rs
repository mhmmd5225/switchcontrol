//! The glowing bar at the edge leading to the other computer.
//!
//! It lives in a window of its own: transparent, click-through, always on
//! top and never focused, laid along the whole shared edge. The window stays
//! put while the engine runs; only what it draws changes, and the engine
//! repaints it directly, so the bar stays current even while the main window
//! is minimized or covered (eframe then stops drawing the main window).

use std::sync::{Arc, Mutex};

use eframe::egui::{self, Color32, Mesh, Pos2, ViewportBuilder, ViewportId};
use sc_core::{EDGE_STEPS, EdgeHint, Side, edge_span, edge_x};

/// Width of the bar's window, in points.
const WIDTH: f32 = 28.0;
/// Width of the solid line right at the edge, in points.
const LINE: f32 = 3.0;
/// Length over which the bar fades out at its ends, in points.
const FADE: f32 = 48.0;
const GLOW: (u8, u8, u8) = (0x4d, 0xa3, 0xff);

fn viewport_id() -> ViewportId {
    ViewportId::from_hash_of("switchcontrol-edge-bar")
}

/// Where the bar's window goes, in screen units (Windows: physical pixels,
/// macOS: points).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Strip {
    side: Side,
    /// The outermost pixel column on `side`.
    x: i32,
    top: i32,
    bottom: i32,
    /// Screen units per point.
    scale: f32,
}

impl Strip {
    /// The strip along this machine's edge on `side`.
    pub fn new(screens: &[sc_proto::Rect], side: Side, scale: f32) -> Option<Strip> {
        let x = edge_x(screens, side)?;
        let (top, bottom) = edge_span(screens, side)?;
        Some(Strip { side, x, top, bottom, scale })
    }

    fn position(&self) -> Pos2 {
        let width = WIDTH * self.scale;
        let left = match self.side {
            Side::Left => self.x as f32,
            Side::Right => (self.x + 1) as f32 - width,
        };
        Pos2::new(left / self.scale, self.top as f32 / self.scale)
    }

    fn size(&self) -> egui::Vec2 {
        egui::vec2(WIDTH, (self.bottom - self.top) as f32 / self.scale)
    }
}

/// The current hint, shared between the engine's reports and the bar window.
#[derive(Clone, Default)]
pub struct EdgeBar {
    hint: Arc<Mutex<Option<EdgeHint>>>,
}

impl EdgeBar {
    /// Records a new hint and redraws the bar window alone.
    pub fn set(&self, ctx: &egui::Context, hint: Option<EdgeHint>) {
        let mut current = self.hint.lock().unwrap();
        if *current != hint {
            *current = hint;
            ctx.request_repaint_of(viewport_id());
        }
    }

    /// Keeps the bar window open; call on every frame of the main window
    /// while it should exist.
    pub fn show(&self, ctx: &egui::Context, strip: Strip) {
        // Without real windows (e.g. in tests) the bar would be an embedded panel.
        if ctx.embed_viewports() {
            return;
        }
        let builder = ViewportBuilder::default()
            .with_title("SwitchControl edge")
            .with_position(strip.position())
            .with_inner_size(strip.size())
            .with_decorations(false)
            .with_resizable(false)
            .with_transparent(true)
            .with_has_shadow(false)
            .with_mouse_passthrough(true)
            .with_window_level(egui::WindowLevel::AlwaysOnTop)
            .with_taskbar(false)
            .with_active(false);
        let hint = self.hint.clone();
        ctx.show_viewport_deferred(viewport_id(), builder, move |ui, _class| {
            let hint = *hint.lock().unwrap();
            if let Some(hint) = hint {
                paint(ui, strip, hint);
            }
        });
    }
}

/// Draws the glow for `hint` into the bar window.
fn paint(ui: &mut egui::Ui, strip: Strip, hint: EdgeHint) {
    let area = ui.max_rect();
    let to_y = |units: i32| area.top() + (units - strip.top) as f32 / strip.scale;
    let (top, bottom) = (to_y(hint.top).max(area.top()), to_y(hint.bottom).min(area.bottom()));
    if bottom <= top {
        return;
    }
    let near = f32::from(hint.near) / f32::from(EDGE_STEPS);
    let push = f32::from(hint.push) / f32::from(EDGE_STEPS);
    let strength = 0.25 + 0.75 * near;

    // Outer edge first: x0 at the screen edge, x1 towards the inside.
    let (x0, x1) = match strip.side {
        Side::Right => (area.right(), area.left()),
        Side::Left => (area.left(), area.right()),
    };
    let line_end = x0 + (x1 - x0) * (LINE / WIDTH);
    let mut mesh = Mesh::default();

    let glow = color(strength * 0.55 + push * 0.3);
    band(&mut mesh, (x0, x1), (top, bottom), glow, Color32::TRANSPARENT);
    band(&mut mesh, (x0, line_end), (top, bottom), color(strength), color(strength));

    // The push fills the line from the middle outwards, in white.
    if push > 0.0 {
        let middle = (top + bottom) / 2.0;
        let half = (bottom - top) / 2.0 * push;
        let white = Color32::from_white_alpha((255.0 * strength) as u8);
        band(&mut mesh, (x0, line_end), (middle - half, middle + half), white, white);
    }
    ui.painter().add(egui::Shape::mesh(mesh));
}

fn color(alpha: f32) -> Color32 {
    let a = (alpha.clamp(0.0, 1.0) * 255.0) as u8;
    Color32::from_rgba_unmultiplied(GLOW.0, GLOW.1, GLOW.2, a)
}

/// A rectangle from `x.0` (color `outer`) to `x.1` (color `inner`), fading
/// to nothing over [`FADE`] at its top and bottom.
fn band(mesh: &mut Mesh, x: (f32, f32), y: (f32, f32), outer: Color32, inner: Color32) {
    let fade = FADE.min((y.1 - y.0) / 2.0);
    let rows = [(y.0, false), (y.0 + fade, true), (y.1 - fade, true), (y.1, false)];
    let base = mesh.vertices.len() as u32;
    for (row_y, solid) in rows {
        let (a, b) =
            if solid { (outer, inner) } else { (Color32::TRANSPARENT, Color32::TRANSPARENT) };
        mesh.colored_vertex(Pos2::new(x.0, row_y), a);
        mesh.colored_vertex(Pos2::new(x.1, row_y), b);
    }
    for row in 0..3 {
        let i = base + row * 2;
        mesh.add_triangle(i, i + 1, i + 2);
        mesh.add_triangle(i + 1, i + 3, i + 2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_hugs_the_shared_edge() {
        let screens = [
            sc_proto::Rect { left: 0, top: 0, right: 2560, bottom: 1440 },
            sc_proto::Rect { left: 2560, top: 200, right: 4480, bottom: 1280 },
        ];
        // A 150% display on Windows: 28 points are 42 pixels.
        let right = Strip::new(&screens, Side::Right, 1.5).unwrap();
        assert_eq!(right.position(), Pos2::new((4480.0 - 42.0) / 1.5, 200.0 / 1.5));
        assert_eq!(right.size(), egui::vec2(WIDTH, 1080.0 / 1.5));

        let left = Strip::new(&screens, Side::Left, 1.0).unwrap();
        assert_eq!(left.position(), Pos2::new(0.0, 0.0));
        assert_eq!(left.size(), egui::vec2(WIDTH, 1440.0));
    }
}
