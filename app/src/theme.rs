//! Look and feel of the window: the system's own UI font, one dark "glass"
//! palette, and the widgets egui lacks (cards, buttons, segmented controls,
//! tabs, switches, the two-monitor picture).
//!
//! Every clickable widget here takes a fixed size and paints itself, so
//! hovering or pressing only changes colours: nothing moves, grows or gets a
//! new outline.

use std::sync::Arc;
use std::time::Duration;

use eframe::egui::{
    self, Color32, CornerRadius, CursorIcon, FontData, FontDefinitions, FontFamily, FontId, Id,
    Margin, Pos2, Rect, Response, RichText, Role, Sense, Shape, Stroke, StrokeKind, TextStyle,
    Theme, Vec2, WidgetInfo,
};

const SEMIBOLD_FAMILY: &str = "semibold";

/// Font files to try, regular then semibold. Variable fonts serve both, the
/// weight then comes from the `wght` axis.
fn font_candidates() -> (&'static [&'static str], &'static [&'static str]) {
    if cfg!(target_os = "macos") {
        (&["/System/Library/Fonts/SFNS.ttf"], &["/System/Library/Fonts/SFNS.ttf"])
    } else if cfg!(windows) {
        (
            &[r"C:\Windows\Fonts\SegUIVar.ttf", r"C:\Windows\Fonts\segoeui.ttf"],
            &[r"C:\Windows\Fonts\SegUIVar.ttf", r"C:\Windows\Fonts\seguisb.ttf"],
        )
    } else {
        (
            &["/usr/share/fonts/opentype/inter/Inter-Regular.otf"],
            &["/usr/share/fonts/opentype/inter/Inter-SemiBold.otf"],
        )
    }
}

fn first_font(paths: &[&str]) -> Option<Vec<u8>> {
    paths.iter().find_map(|p| std::fs::read(p).ok())
}

/// Sets fonts and the palette. Call once at startup.
pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let defaults = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let (regular, semibold) = font_candidates();
    let mut semibold_family = defaults.clone();
    if let Some(bytes) = first_font(regular) {
        fonts.font_data.insert("system-ui".into(), Arc::new(FontData::from_owned(bytes)));
        if let Some(family) = fonts.families.get_mut(&FontFamily::Proportional) {
            family.insert(0, "system-ui".into());
        }
        semibold_family.insert(0, "system-ui".into());
    }
    if let Some(bytes) = first_font(semibold) {
        fonts.font_data.insert("system-ui-semibold".into(), Arc::new(FontData::from_owned(bytes)));
        semibold_family.insert(0, "system-ui-semibold".into());
    }
    fonts.families.insert(FontFamily::Name(SEMIBOLD_FAMILY.into()), semibold_family);
    ctx.set_fonts(fonts);

    // One look, whatever the OS theme says.
    ctx.set_theme(egui::ThemePreference::Dark);
    let p = Palette::glass();
    ctx.style_mut_of(Theme::Light, |style| apply(style, &p));
    ctx.style_mut_of(Theme::Dark, |style| apply(style, &p));
}

/// Text in the semibold weight.
pub fn semibold(text: impl Into<String>) -> RichText {
    RichText::new(text).family(FontFamily::Name(SEMIBOLD_FAMILY.into())).variation("wght", 600.0)
}

#[derive(Debug, Clone, Copy)]
pub struct Palette {
    /// Top and bottom of the window's gradient.
    pub top: Color32,
    pub bottom: Color32,
    /// Glass: translucent white over the gradient.
    pub card: Color32,
    pub border: Color32,
    pub shine: Color32,
    pub field: Color32,
    pub field_hover: Color32,
    pub text: Color32,
    pub muted: Color32,
    /// The accent runs from `accent` to `accent_end`.
    pub accent: Color32,
    pub accent_end: Color32,
    pub green: Color32,
    pub amber: Color32,
    pub red: Color32,
}

impl Palette {
    pub fn glass() -> Palette {
        Palette {
            top: Color32::from_rgb(0x0b, 0x0d, 0x17),
            bottom: Color32::from_rgb(0x16, 0x10, 0x2c),
            card: Color32::from_white_alpha(13),
            border: Color32::from_white_alpha(23),
            shine: Color32::from_white_alpha(36),
            field: Color32::from_white_alpha(15),
            field_hover: Color32::from_white_alpha(26),
            text: Color32::from_rgb(0xee, 0xf0, 0xf6),
            muted: Color32::from_rgb(0x8b, 0x90, 0xa5),
            accent: Color32::from_rgb(0x5b, 0x8c, 0xff),
            accent_end: Color32::from_rgb(0x9b, 0x6b, 0xff),
            green: Color32::from_rgb(0x3d, 0xdc, 0x97),
            amber: Color32::from_rgb(0xff, 0xb5, 0x47),
            red: Color32::from_rgb(0xff, 0x6b, 0x7a),
        }
    }

    pub fn of(_ui: &egui::Ui) -> Palette {
        Palette::glass()
    }
}

fn apply(style: &mut egui::Style, p: &Palette) {
    use egui::FontFamily::{Monospace, Proportional};
    style.text_styles = [
        (TextStyle::Heading, FontId::new(20.0, Proportional)),
        (TextStyle::Body, FontId::new(14.0, Proportional)),
        (TextStyle::Button, FontId::new(14.0, Proportional)),
        (TextStyle::Small, FontId::new(11.5, Proportional)),
        (TextStyle::Monospace, FontId::new(13.0, Monospace)),
    ]
    .into();
    style.spacing.item_spacing = Vec2::new(8.0, 8.0);
    style.spacing.button_padding = Vec2::new(12.0, 6.0);
    style.spacing.interact_size.y = 30.0;
    style.spacing.slider_width = 150.0;

    let v = &mut style.visuals;
    v.dark_mode = true;
    v.override_text_color = Some(p.text);
    v.weak_text_color = Some(p.muted);
    // The window paints its own gradient.
    v.panel_fill = Color32::TRANSPARENT;
    v.window_fill = Color32::from_rgb(0x1a, 0x1a, 0x2e);
    v.window_stroke = Stroke::new(1.0, p.border);
    v.extreme_bg_color = Color32::TRANSPARENT;
    v.faint_bg_color = p.field;
    v.error_fg_color = p.red;
    v.warn_fg_color = p.amber;
    v.hyperlink_color = p.accent;
    v.selection.bg_fill = p.accent;
    v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    v.slider_trailing_fill = true;
    v.collapsing_header_frame = false;
    v.indent_has_left_vline = false;

    // The widgets egui still draws (slider, collapsing header) change colour
    // on hover only: same outline width, no growing.
    let radius = CornerRadius::same(8);
    let widgets = &mut v.widgets;
    widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    for (w, fill) in [
        (&mut widgets.inactive, p.field),
        (&mut widgets.hovered, p.field_hover),
        (&mut widgets.active, p.field_hover),
        (&mut widgets.open, p.field),
    ] {
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.bg_stroke = Stroke::NONE;
        w.fg_stroke = Stroke::new(1.0, p.text);
        w.corner_radius = radius;
        w.expansion = 0.0;
    }
    widgets.noninteractive.corner_radius = radius;
}

// ---------------------------------------------------------------- helpers

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    a.lerp_to_gamma(b, t.clamp(0.0, 1.0))
}

/// Eases towards 1.0 while `on`, back to 0.0 otherwise.
fn anim(ui: &egui::Ui, id: Id, on: bool) -> f32 {
    ui.ctx().animate_bool_with_time(id, on, 0.14)
}

/// The rectangle `target`, reached gradually: what slides under the tabs.
fn glide(ui: &egui::Ui, id: Id, target: Rect) -> Rect {
    let ctx = ui.ctx();
    let x0 = ctx.animate_value_with_time(id.with("x0"), target.min.x, 0.18);
    let x1 = ctx.animate_value_with_time(id.with("x1"), target.max.x, 0.18);
    Rect::from_min_max(Pos2::new(x0, target.min.y), Pos2::new(x1, target.max.y))
}

fn text_galley(ui: &egui::Ui, text: RichText) -> Arc<egui::Galley> {
    egui::WidgetText::from(text).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        TextStyle::Button,
    )
}

fn with_alpha(c: Color32, alpha: f32) -> Color32 {
    let [r, g, b, _] = c.to_srgba_unmultiplied();
    Color32::from_rgba_unmultiplied(r, g, b, (alpha.clamp(0.0, 1.0) * 255.0) as u8)
}

/// A rounded rectangle filled with a left-to-right gradient, in thin slices.
fn gradient_rect(
    painter: &egui::Painter,
    rect: Rect,
    radius: CornerRadius,
    a: Color32,
    b: Color32,
) {
    const SLICES: usize = 24;
    let w = rect.width() / SLICES as f32;
    for i in 0..SLICES {
        let t = (i as f32 + 0.5) / SLICES as f32;
        let x0 = rect.left() + w * i as f32;
        let slice = Rect::from_min_max(
            Pos2::new(x0, rect.top()),
            Pos2::new((x0 + w + 1.0).min(rect.right()), rect.bottom()),
        );
        painter.with_clip_rect(slice.intersect(painter.clip_rect())).rect_filled(
            rect,
            radius,
            mix(a, b, t),
        );
    }
}

/// A soft light that fades to nothing at `radius`.
fn glow(painter: &egui::Painter, center: Pos2, radius: f32, color: Color32) {
    const N: u32 = 48;
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(center, color);
    for i in 0..N {
        let angle = i as f32 / N as f32 * std::f32::consts::TAU;
        let at = center + Vec2::new(angle.cos(), angle.sin()) * radius;
        mesh.colored_vertex(at, Color32::TRANSPARENT);
    }
    for i in 0..N {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % N);
    }
    painter.add(Shape::mesh(mesh));
}

/// A blurred shadow in `color` behind `rect`.
fn halo(painter: &egui::Painter, rect: Rect, radius: u8, color: Color32) {
    let shadow = egui::Shadow { offset: [0, 4], blur: 18, spread: 0, color };
    painter.add(shadow.as_shape(rect, CornerRadius::same(radius)));
}

/// The window's background: a dark gradient with two soft coloured lights.
/// Paint it first each frame, before the panels.
pub fn paint_backdrop(ui: &egui::Ui) {
    let p = Palette::glass();
    let rect = ui.ctx().content_rect();
    let painter = ui.painter();
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), p.top);
    mesh.colored_vertex(rect.right_top(), p.top);
    mesh.colored_vertex(rect.right_bottom(), p.bottom);
    mesh.colored_vertex(rect.left_bottom(), p.bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(Shape::mesh(mesh));
    glow(painter, rect.left_top() + Vec2::new(70.0, 110.0), 260.0, with_alpha(p.accent, 0.20));
    glow(
        painter,
        rect.right_bottom() - Vec2::new(40.0, 170.0),
        300.0,
        with_alpha(p.accent_end, 0.17),
    );
}

// ------------------------------------------------------------- containers

/// A translucent glass panel holding one group of controls.
pub fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let p = Palette::of(ui);
    let out = egui::Frame::new()
        .fill(p.card)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::symmetric(16, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        });
    // The light catching the top edge.
    let rect = out.response.rect;
    ui.painter().line_segment(
        [rect.left_top() + Vec2::new(16.0, 0.5), rect.right_top() + Vec2::new(-16.0, 0.5)],
        Stroke::new(1.0, p.shine),
    );
    out.inner
}

/// A card with a coloured edge and strip, for things that need attention.
pub fn notice<R>(ui: &mut egui::Ui, accent: Color32, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let p = Palette::of(ui);
    let out = egui::Frame::new()
        .fill(mix(Color32::from_rgb(0x14, 0x14, 0x24), accent, 0.10))
        .stroke(Stroke::new(1.0, with_alpha(accent, 0.55)))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin { left: 20, right: 16, top: 12, bottom: 12 })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        });
    let rect = out.response.rect;
    ui.painter().line_segment(
        [rect.left_top() + Vec2::new(8.0, 14.0), rect.left_bottom() + Vec2::new(8.0, -14.0)],
        Stroke::new(3.0, accent),
    );
    ui.painter().line_segment(
        [rect.left_top() + Vec2::new(16.0, 0.5), rect.right_top() + Vec2::new(-16.0, 0.5)],
        Stroke::new(1.0, p.shine),
    );
    out.inner
}

/// The small muted title above a card.
pub fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        ui.label(semibold(title.to_uppercase()).size(11.0).color(Palette::of(ui).muted));
    });
    ui.add_space(2.0);
}

/// One line of a card: a label on the left, its control on the right.
pub fn row<R>(
    ui: &mut egui::Ui,
    label: &str,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> (Response, R) {
    ui.horizontal(|ui| {
        ui.set_min_height(32.0);
        let label = ui.label(label);
        let inner = ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), add).inner;
        (label, inner)
    })
    .inner
}

/// A thin line between rows of a card.
pub fn divider(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, Color32::from_white_alpha(14));
}

// ---------------------------------------------------------------- buttons

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The one thing to press: gradient.
    Primary,
    /// An ordinary action: glass.
    Secondary,
    /// Quiet, for small or optional actions.
    Ghost,
    /// Undoes something: red.
    Danger,
}

/// A button as wide as its text.
pub fn button(ui: &mut egui::Ui, label: &str, kind: Kind) -> Response {
    let width = text_galley(ui, semibold(label).size(13.5)).size().x + 28.0;
    draw_button(ui, Vec2::new(width.max(64.0), 30.0), label, kind, 13.5, 9)
}

/// The big button at the bottom of the window.
pub fn big_button(ui: &mut egui::Ui, label: &str, kind: Kind) -> Response {
    draw_button(ui, Vec2::new(ui.available_width(), 44.0), label, kind, 15.0, 12)
}

fn draw_button(
    ui: &mut egui::Ui,
    size: Vec2,
    label: &str,
    kind: Kind,
    font_size: f32,
    radius: u8,
) -> Response {
    let p = Palette::of(ui);
    let enabled = ui.is_enabled();
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| WidgetInfo::labeled(Role::Button, enabled, label));
    if enabled && response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let hover = anim(ui, response.id.with("hover"), enabled && response.hovered());
    let press =
        anim(ui, response.id.with("press"), enabled && response.is_pointer_button_down_on());
    let dim = if enabled { 1.0 } else { 0.4 };
    let radius = CornerRadius::same(radius);
    let painter = ui.painter().clone();
    // Brighter on hover, darker while pressed.
    let shade =
        |c: Color32| mix(mix(c, Color32::WHITE, 0.12 * hover), Color32::BLACK, 0.12 * press);

    let (text_color, edge) = match kind {
        Kind::Primary | Kind::Danger => {
            let (a, b) = if kind == Kind::Primary {
                (p.accent, p.accent_end)
            } else {
                (mix(p.red, Color32::BLACK, 0.25), mix(p.red, Color32::BLACK, 0.45))
            };
            if hover > 0.0 {
                halo(&painter, rect, 12, with_alpha(a, 0.38 * hover * dim));
            }
            gradient_rect(
                &painter,
                rect,
                radius,
                shade(a).gamma_multiply(dim),
                shade(b).gamma_multiply(dim),
            );
            (Color32::WHITE.gamma_multiply(dim), Color32::from_white_alpha((40.0 * dim) as u8))
        }
        Kind::Secondary => {
            let fill = mix(p.field, p.field_hover, hover);
            painter.rect_filled(rect, radius, mix(fill, Color32::BLACK, 0.15 * press));
            (p.text.gamma_multiply(dim), mix(p.border, p.shine, hover))
        }
        Kind::Ghost => {
            let fill = with_alpha(Color32::WHITE, 0.10 * hover + 0.05 * press);
            painter.rect_filled(rect, radius, fill);
            (mix(p.muted, p.text, hover).gamma_multiply(dim), Color32::TRANSPARENT)
        }
    };
    painter.rect_stroke(rect, radius, Stroke::new(1.0, edge), StrokeKind::Inside);
    if response.has_focus() {
        painter.rect_stroke(rect, radius, Stroke::new(2.0, p.accent), StrokeKind::Outside);
    }
    let galley = text_galley(ui, semibold(label).size(font_size).color(text_color));
    painter.galley(rect.center() - galley.size() / 2.0, galley, text_color);
    response
}

/// Buttons side by side in a rounded track, one of them selected. A coloured
/// pill slides to the one picked.
pub fn segmented<T: PartialEq + Copy>(
    ui: &mut egui::Ui,
    value: &mut T,
    options: &[(T, &str)],
) -> Response {
    let p = Palette::of(ui);
    let enabled = ui.is_enabled();
    let (inset, item_h, gap) = (3.0, 26.0, 2.0);
    // Measured in the bold weight, so picking another option never resizes the track.
    let widths: Vec<f32> = options
        .iter()
        .map(|&(_, label)| text_galley(ui, semibold(label).size(13.0)).size().x + 22.0)
        .collect();
    let total =
        widths.iter().sum::<f32>() + gap * options.len().saturating_sub(1) as f32 + inset * 2.0;
    let (track, mut response) =
        ui.allocate_exact_size(Vec2::new(total, item_h + inset * 2.0), Sense::hover());
    let id = response.id;
    let painter = ui.painter().clone();
    painter.rect_filled(track, CornerRadius::same(10), p.field);
    painter.rect_stroke(
        track,
        CornerRadius::same(10),
        Stroke::new(1.0, Color32::from_white_alpha(14)),
        StrokeKind::Inside,
    );

    let mut x = track.left() + inset;
    let rects: Vec<Rect> = widths
        .iter()
        .map(|&w| {
            let r = Rect::from_min_size(Pos2::new(x, track.top() + inset), Vec2::new(w, item_h));
            x += w + gap;
            r
        })
        .collect();
    if let Some(i) = options.iter().position(|&(o, _)| o == *value) {
        let pill = glide(ui, id, rects[i]);
        gradient_rect(&painter, pill, CornerRadius::same(7), p.accent, p.accent_end);
    }

    let mut changed = None;
    for (i, &(option, label)) in options.iter().enumerate() {
        let selected = *value == option;
        let item = ui.interact(rects[i], id.with(i), Sense::click());
        item.widget_info(|| WidgetInfo::selected(Role::Button, enabled, selected, label));
        if enabled && item.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        let hover = anim(ui, item.id.with("hover"), enabled && item.hovered());
        let color = if selected { Color32::WHITE } else { mix(p.muted, p.text, hover) };
        let color = if enabled { color } else { color.gamma_multiply(0.5) };
        let galley = text_galley(ui, semibold(label).size(13.0).color(color));
        painter.galley(rects[i].center() - galley.size() / 2.0, galley, color);
        if item.clicked() && !selected {
            changed = Some(option);
        }
    }
    if let Some(option) = changed {
        *value = option;
        response.mark_changed();
    }
    response
}

/// Page tabs across the width: a coloured line slides under the open one.
pub fn tabs<T: PartialEq + Copy>(ui: &mut egui::Ui, value: &mut T, options: &[(T, &str)]) {
    let p = Palette::of(ui);
    let height = 36.0;
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    let id = response.id;
    let painter = ui.painter().clone();
    painter.line_segment(
        [rect.left_bottom() - Vec2::new(0.0, 0.5), rect.right_bottom() - Vec2::new(0.0, 0.5)],
        Stroke::new(1.0, Color32::from_white_alpha(18)),
    );
    let width = rect.width() / options.len().max(1) as f32;
    let mut changed = None;
    for (i, &(option, label)) in options.iter().enumerate() {
        let selected = *value == option;
        let item_rect = Rect::from_min_size(
            rect.min + Vec2::new(width * i as f32, 0.0),
            Vec2::new(width, height),
        );
        let item = ui.interact(item_rect, id.with(i), Sense::click());
        item.widget_info(|| WidgetInfo::selected(Role::Button, true, selected, label));
        if item.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        let hover = anim(ui, item.id.with("hover"), item.hovered());
        let color = if selected { p.text } else { mix(p.muted, p.text, hover) };
        let text = if selected { semibold(label) } else { RichText::new(label) };
        let galley = text_galley(ui, text.size(14.0).color(color));
        painter.galley(item_rect.center() - galley.size() / 2.0, galley, color);
        if selected {
            let line = Rect::from_min_max(
                Pos2::new(item_rect.left() + 18.0, rect.bottom() - 2.0),
                Pos2::new(item_rect.right() - 18.0, rect.bottom()),
            );
            gradient_rect(
                &painter,
                glide(ui, id, line),
                CornerRadius::same(1),
                p.accent,
                p.accent_end,
            );
        }
        if item.clicked() && !selected {
            changed = Some(option);
        }
    }
    if let Some(option) = changed {
        *value = option;
    }
}

/// An on/off switch.
pub fn switch(ui: &mut egui::Ui, on: &mut bool, label: &str) -> Response {
    let p = Palette::of(ui);
    let size = Vec2::new(40.0, 24.0);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let enabled = ui.is_enabled();
    response.widget_info(|| WidgetInfo::selected(Role::Switch, enabled, *on, label));
    if enabled && response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    if ui.is_rect_visible(rect) {
        let t = ui.ctx().animate_bool_responsive(response.id, *on);
        let hover = anim(ui, response.id.with("hover"), enabled && response.hovered());
        let dim = if enabled { 1.0 } else { 0.45 };
        let painter = ui.painter().clone();
        let radius = CornerRadius::same(12);
        let off = mix(p.field_hover, Color32::WHITE, 0.06 * hover);
        painter.rect_filled(rect, radius, off.gamma_multiply(dim));
        if t > 0.0 {
            // The gradient track fades in as the knob moves over.
            let on_a = mix(off, p.accent, t);
            let on_b = mix(off, p.accent_end, t);
            gradient_rect(
                &painter,
                rect,
                radius,
                on_a.gamma_multiply(dim),
                on_b.gamma_multiply(dim),
            );
        }
        let x = egui::lerp((rect.left() + 12.0)..=(rect.right() - 12.0), t);
        let center = Pos2::new(x, rect.center().y);
        if hover > 0.0 {
            painter.circle_filled(
                center,
                9.5 + 3.5 * hover,
                Color32::from_white_alpha((40.0 * hover) as u8),
            );
        }
        painter.circle_filled(center, 9.5, Color32::WHITE.gamma_multiply(dim));
    }
    response
}

/// A text field in a glass frame. The frame is the same size whether or not
/// the pointer is over it or it has focus; only its colours change.
pub fn text_field(
    ui: &mut egui::Ui,
    text: &mut String,
    hint: &str,
    width: f32,
    style: TextStyle,
) -> Response {
    let p = Palette::of(ui);
    let height = if style == TextStyle::Heading { 42.0 } else { 32.0 };
    let (rect, frame) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let radius = CornerRadius::same(9);
    let hover = anim(ui, frame.id.with("hover"), ui.rect_contains_pointer(rect));
    ui.painter().rect_filled(rect, radius, mix(p.field, p.field_hover, hover * 0.6));

    let inner = rect.shrink2(Vec2::new(10.0, 0.0));
    let response = ui
        .scope_builder(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| {
                ui.add(
                    egui::TextEdit::singleline(text)
                        .hint_text(hint)
                        .font(style)
                        .frame(egui::Frame::NONE)
                        .vertical_align(egui::Align::Center)
                        .min_size(Vec2::new(inner.width(), height))
                        .desired_width(inner.width()),
                )
            },
        )
        .inner;
    let focus = anim(ui, frame.id.with("focus"), response.has_focus());
    let edge = mix(Color32::from_white_alpha(20), p.accent, focus);
    ui.painter().rect_stroke(rect, radius, Stroke::new(1.0, edge), StrokeKind::Inside);
    response
}

// ------------------------------------------------------------------ status

/// How the picture shows the link between the two computers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link {
    Off,
    Waiting,
    Connected,
    /// Connected, and keyboard and mouse drive the other computer.
    Remote,
}

/// A small rounded badge with the link's state, a pulsing dot while waiting.
pub fn status_pill(ui: &mut egui::Ui, link: Link) {
    let p = Palette::of(ui);
    let (text, color) = match link {
        Link::Off => ("Kapalı", p.muted),
        Link::Waiting => ("Bekleniyor", p.amber),
        Link::Connected => ("Bağlı", p.green),
        Link::Remote => ("Kontrol karşıda", p.accent),
    };
    let galley = text_galley(ui, semibold(text).size(12.0).color(color));
    let size = Vec2::new(galley.size().x + 36.0, 26.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(13), with_alpha(color, 0.14));
    painter.rect_stroke(
        rect,
        CornerRadius::same(13),
        Stroke::new(1.0, with_alpha(color, 0.35)),
        StrokeKind::Inside,
    );
    let dot = Pos2::new(rect.left() + 14.0, rect.center().y);
    painter.circle_filled(dot, 3.5, color);
    if link == Link::Waiting {
        let phase = (ui.input(|i| i.time) as f32 * 1.1).fract();
        painter.circle_stroke(
            dot,
            3.5 + 7.0 * phase,
            Stroke::new(1.5, with_alpha(color, 0.6 * (1.0 - phase))),
        );
        ui.ctx().request_repaint_after(Duration::from_millis(33));
    }
    painter.galley(
        Pos2::new(rect.left() + 25.0, rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
}

/// Two monitors side by side, the other computer on the side it sits, with
/// the link between them.
pub fn monitors(ui: &mut egui::Ui, peer_left: bool, link: Link, this: &str, peer: &str) {
    let p = Palette::of(ui);
    let height = 132.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    let painter = ui.painter_at(rect.expand(24.0).intersect(ui.clip_rect()));
    let time = ui.input(|i| i.time) as f32;

    let screen = Vec2::new(108.0, 66.0);
    let gap = 92.0;
    let total = screen.x * 2.0 + gap;
    let left = rect.center().x - total / 2.0;
    let top = rect.top() + 6.0;
    let left_rect = Rect::from_min_size(Pos2::new(left, top), screen);
    let right_rect = Rect::from_min_size(Pos2::new(left + screen.x + gap, top), screen);
    let (peer_rect, this_rect) =
        if peer_left { (left_rect, right_rect) } else { (right_rect, left_rect) };

    let line_color = match link {
        Link::Off => Color32::from_white_alpha(40),
        Link::Waiting => p.amber,
        Link::Connected => p.green,
        Link::Remote => p.accent,
    };
    // The link, drawn first so the monitors sit on top of its ends.
    let y = left_rect.center().y;
    let (x0, x1) = (left_rect.right() + 8.0, right_rect.left() - 8.0);
    match link {
        Link::Off => dashes(&painter, x0, x1, y, 0.0, line_color),
        Link::Waiting => {
            // Dashes marching towards the other computer.
            let direction = if peer_left { -1.0 } else { 1.0 };
            dashes(&painter, x0, x1, y, time * 14.0 * direction, line_color);
            ui.ctx().request_repaint_after(Duration::from_millis(33));
        }
        Link::Connected | Link::Remote => {
            painter.line_segment(
                [Pos2::new(x0, y), Pos2::new(x1, y)],
                Stroke::new(7.0, with_alpha(line_color, 0.16)),
            );
            painter
                .line_segment([Pos2::new(x0, y), Pos2::new(x1, y)], Stroke::new(2.5, line_color));
            painter.circle_filled(Pos2::new(x0, y), 4.0, line_color);
            painter.circle_filled(Pos2::new(x1, y), 4.0, line_color);
            if link == Link::Remote {
                // A light travelling from this computer to the other.
                let t = (time * 0.9).fract();
                let (from, to) = if peer_left { (x1, x0) } else { (x0, x1) };
                painter.circle_filled(Pos2::new(egui::lerp(from..=to, t), y), 3.0, Color32::WHITE);
                ui.ctx().request_repaint_after(Duration::from_millis(33));
            }
        }
    }

    // Where keyboard and mouse are: lit up in the accent colour.
    let active_peer = link == Link::Remote;
    let active_this = link == Link::Connected;
    for (r, active, name, caption) in [
        (this_rect, active_this, this, "Bu bilgisayar"),
        (peer_rect, active_peer, peer, "Diğer bilgisayar"),
    ] {
        let lit = anim(ui, ui.scope_id().with(("monitor", caption)), active);
        if lit > 0.0 {
            halo(&painter, r, 8, with_alpha(p.accent, 0.45 * lit));
        }
        let fill = mix(Color32::from_white_alpha(16), with_alpha(p.accent, 0.22), lit);
        painter.rect_filled(r, CornerRadius::same(8), fill);
        // A faint glare across the glass.
        painter.line_segment(
            [r.left_top() + Vec2::new(10.0, 1.0), r.right_top() + Vec2::new(-10.0, 1.0)],
            Stroke::new(1.0, p.shine),
        );
        painter.rect_stroke(
            r,
            CornerRadius::same(8),
            Stroke::new(1.5, mix(Color32::from_white_alpha(46), p.accent, lit)),
            StrokeKind::Inside,
        );
        // Stand.
        let neck =
            Rect::from_center_size(Pos2::new(r.center().x, r.bottom() + 5.0), Vec2::new(10.0, 8.0));
        painter.rect_filled(neck, 0.0, Color32::from_white_alpha(34));
        let foot = Rect::from_center_size(
            Pos2::new(r.center().x, r.bottom() + 10.0),
            Vec2::new(40.0, 3.0),
        );
        painter.rect_filled(foot, CornerRadius::same(2), Color32::from_white_alpha(34));

        painter.text(
            Pos2::new(r.center().x, r.bottom() + 20.0),
            egui::Align2::CENTER_TOP,
            name,
            FontId::new(13.0, FontFamily::Name(SEMIBOLD_FAMILY.into())),
            p.text,
        );
        painter.text(
            Pos2::new(r.center().x, r.bottom() + 37.0),
            egui::Align2::CENTER_TOP,
            caption,
            FontId::proportional(11.0),
            p.muted,
        );
    }
}

/// A dashed line from `x0` to `x1`, the dashes shifted by `offset`.
fn dashes(painter: &egui::Painter, x0: f32, x1: f32, y: f32, offset: f32, color: Color32) {
    let period = 11.0;
    let mut x = x0 - offset.rem_euclid(period);
    while x < x1 {
        let (a, b) = (x.max(x0), (x + 6.0).min(x1));
        if b > a {
            painter.line_segment([Pos2::new(a, y), Pos2::new(b, y)], Stroke::new(2.0, color));
        }
        x += period;
    }
}
