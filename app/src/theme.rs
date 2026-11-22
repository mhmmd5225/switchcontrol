//! Look and feel of the window: the system's own UI font, a dark palette, and the widgets egui lacks (cards, buttons, segmented controls,
//! tabs, switches, the two-monitor picture).
//!
//! Every clickable widget here takes a fixed size and paints itself, so
//! hovering or pressing only changes colours: nothing moves, grows or gets a
//! new outline.

use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, CursorIcon, FontData, FontDefinitions, FontFamily, FontId, Id,
    Margin, Pos2, Rect, Response, RichText, Role, Sense, Stroke, StrokeKind, TextStyle, Theme,
    Vec2, WidgetInfo,
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

/// Sets fonts and the palette. Call once at startup.
pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let defaults = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let (regular, semibold) = font_candidates();
    let mut semibold_family = defaults.clone();
    let mut regular_path = None;
    if let Some((path, bytes)) =
        regular.iter().find_map(|path| std::fs::read(path).ok().map(|bytes| (*path, bytes)))
    {
        regular_path = Some(path);
        fonts.font_data.insert("system-ui".into(), Arc::new(FontData::from_owned(bytes)));
        if let Some(family) = fonts.families.get_mut(&FontFamily::Proportional) {
            family.insert(0, "system-ui".into());
        }
        semibold_family.insert(0, "system-ui".into());
    }
    for &path in semibold {
        if regular_path == Some(path) {
            break;
        }
        if let Ok(bytes) = std::fs::read(path) {
            fonts
                .font_data
                .insert("system-ui-semibold".into(), Arc::new(FontData::from_owned(bytes)));
            semibold_family.insert(0, "system-ui-semibold".into());
            break;
        }
    }
    fonts.families.insert(FontFamily::Name(SEMIBOLD_FAMILY.into()), semibold_family);
    ctx.set_fonts(fonts);

    // One look, whatever the OS theme says.
    ctx.set_theme(egui::ThemePreference::Dark);
    let p = Palette::dark();
    ctx.style_mut_of(Theme::Light, |style| apply(style, &p));
    ctx.style_mut_of(Theme::Dark, |style| apply(style, &p));
}

/// Text in the semibold weight.
pub fn semibold(text: impl Into<String>) -> RichText {
    RichText::new(text).family(FontFamily::Name(SEMIBOLD_FAMILY.into())).variation("wght", 600.0)
}

#[derive(Debug, Clone, Copy)]
pub struct Palette {
    /// Window background.
    pub top: Color32,
    /// Panel surface.
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
    pub fn dark() -> Palette {
        Palette {
            top: Color32::from_rgb(0x12, 0x16, 0x1e),
            card: Color32::from_white_alpha(13),
            border: Color32::from_white_alpha(23),
            shine: Color32::from_white_alpha(36),
            field: Color32::from_white_alpha(15),
            field_hover: Color32::from_white_alpha(26),
            text: Color32::from_rgb(0xee, 0xf0, 0xf6),
            muted: Color32::from_rgb(0xa7, 0xb2, 0xc5),
            accent: Color32::from_rgb(0x78, 0xb1, 0xff),
            accent_end: Color32::from_rgb(0x86, 0xc9, 0xff),
            green: Color32::from_rgb(0x3d, 0xdc, 0x97),
            amber: Color32::from_rgb(0xff, 0xb5, 0x47),
            red: Color32::from_rgb(0xff, 0x6b, 0x7a),
        }
    }

    pub fn of(_ui: &egui::Ui) -> Palette {
        Palette::dark()
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
    // The window paints its own background.
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
/// Paint the window surface behind the panels.
pub fn paint_backdrop(ui: &egui::Ui) {
    ui.painter().rect_filled(ui.ctx().content_rect(), 0.0, Palette::of(ui).top);
}

// ------------------------------------------------------------- containers

/// A panel holding one group of controls.
pub fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let p = Palette::of(ui);
    egui::Frame::new()
        .fill(p.card)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// A panel for pairing, permissions, or other feedback.
pub fn notice<R>(ui: &mut egui::Ui, accent: Color32, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(mix(Palette::of(ui).top, accent, 0.10))
        .stroke(Stroke::new(1.0, with_alpha(accent, 0.55)))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
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
    if ui.available_width() < 385.0 {
        let label = ui.label(label);
        let inner = ui.horizontal_wrapped(add).inner;
        return (label, inner);
    }
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
            gradient_rect(
                &painter,
                rect,
                radius,
                shade(a).gamma_multiply(dim),
                shade(b).gamma_multiply(dim),
            );
            let text = if kind == Kind::Primary { p.top } else { Color32::WHITE };
            (text.gamma_multiply(dim), Color32::from_white_alpha((40.0 * dim) as u8))
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
        let color = if selected { p.top } else { mix(p.muted, p.text, hover) };
        let color = if enabled { color } else { color.gamma_multiply(0.5) };
        let galley = text_galley(ui, semibold(label).size(13.0).color(color));
        painter.galley(rects[i].center() - galley.size() / 2.0, galley, color);
        if item.has_focus() {
            painter.rect_stroke(rects[i], 7, Stroke::new(2.0, p.text), StrokeKind::Inside);
        }
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
        if item.has_focus() {
            painter.rect_stroke(item_rect, 5, Stroke::new(2.0, p.accent), StrokeKind::Inside);
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
        if response.has_focus() {
            painter.rect_stroke(rect, radius, Stroke::new(2.0, p.accent), StrokeKind::Outside);
        }
    }
    response
}

/// A text field with a focus outline. The frame is the same size whether or not
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
    let (rect, frame) = ui.allocate_exact_size(
        Vec2::new(width.min(ui.available_width()).max(24.0), height),
        Sense::hover(),
    );
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
                        .align(egui::Align2::LEFT_CENTER)
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

/// A static indicator beside the connection status.
pub fn status_indicator(ui: &mut egui::Ui, link: Link) {
    let p = Palette::of(ui);
    let color = match link {
        Link::Off => p.muted,
        Link::Waiting => p.amber,
        Link::Connected => p.green,
        Link::Remote => p.accent,
    };
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.0, color);
}

/// Two monitors side by side, the other computer on the side it sits, with
/// the link between them.
pub fn monitors(ui: &mut egui::Ui, peer_left: bool, link: Link) {
    let p = Palette::of(ui);
    let width = ui.available_width().min(360.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 108.0), Sense::hover());
    let painter = ui.painter_at(rect);
    let gap = (width * 0.21).min(72.0);
    let screen = Vec2::new((width - gap) / 2.0, 62.0);
    let left = rect.center().x - width / 2.0;
    let top = rect.top() + 4.0;
    let left_rect = Rect::from_min_size(Pos2::new(left, top), screen);
    let right_rect = Rect::from_min_size(Pos2::new(left + screen.x + gap, top), screen);
    let line = match link {
        Link::Off => p.border,
        Link::Waiting => p.amber,
        Link::Connected => p.green,
        Link::Remote => p.accent,
    };
    let y = left_rect.center().y;
    painter.line_segment(
        [Pos2::new(left_rect.right(), y), Pos2::new(right_rect.left(), y)],
        Stroke::new(2.0, line),
    );
    for (screen, peer) in [(left_rect, peer_left), (right_rect, !peer_left)] {
        let active = (peer && link == Link::Remote) || (!peer && link == Link::Connected);
        let (fill, outline) =
            if active { (with_alpha(line, 0.22), line) } else { (p.field, p.border) };
        painter.rect_filled(screen, 8, fill);
        painter.rect_stroke(screen, 8, Stroke::new(1.5, outline), StrokeKind::Inside);
        painter.line_segment(
            [
                screen.center_bottom() + Vec2::new(-12.0, 8.0),
                screen.center_bottom() + Vec2::new(12.0, 8.0),
            ],
            Stroke::new(2.0, outline),
        );
        painter.text(
            screen.center_bottom() + Vec2::new(0.0, 14.0),
            egui::Align2::CENTER_TOP,
            if peer { "Diğer bilgisayar" } else { "Bu bilgisayar" },
            FontId::proportional(12.0),
            p.muted,
        );
    }
}
