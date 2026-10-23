//! Look and feel of the window: the system's own UI font, a light and a dark
//! palette that follow the OS, and the few widgets egui lacks (cards,
//! segmented controls, switches, the two-monitor picture).

use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Pos2, Rect,
    Response, RichText, Role, Sense, Stroke, StrokeKind, TextStyle, Theme, Vec2, WidgetInfo,
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

/// Sets fonts and both palettes. Call once at startup.
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

    ctx.style_mut_of(Theme::Light, |style| apply(style, &Palette::LIGHT));
    ctx.style_mut_of(Theme::Dark, |style| apply(style, &Palette::DARK));
}

/// Text in the semibold weight.
pub fn semibold(text: impl Into<String>) -> RichText {
    RichText::new(text).family(FontFamily::Name(SEMIBOLD_FAMILY.into())).variation("wght", 600.0)
}

#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub background: Color32,
    pub card: Color32,
    pub border: Color32,
    pub field: Color32,
    pub field_hover: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub accent: Color32,
    pub green: Color32,
    pub amber: Color32,
    pub red: Color32,
}

impl Palette {
    pub const LIGHT: Palette = Palette {
        background: Color32::from_rgb(0xf4, 0xf5, 0xf7),
        card: Color32::from_rgb(0xff, 0xff, 0xff),
        border: Color32::from_rgb(0xe2, 0xe5, 0xea),
        field: Color32::from_rgb(0xf0, 0xf2, 0xf5),
        field_hover: Color32::from_rgb(0xe6, 0xe9, 0xee),
        text: Color32::from_rgb(0x1d, 0x21, 0x27),
        muted: Color32::from_rgb(0x6b, 0x72, 0x80),
        accent: Color32::from_rgb(0x25, 0x63, 0xeb),
        green: Color32::from_rgb(0x16, 0xa3, 0x4a),
        amber: Color32::from_rgb(0xd9, 0x77, 0x06),
        red: Color32::from_rgb(0xdc, 0x26, 0x26),
    };

    pub const DARK: Palette = Palette {
        background: Color32::from_rgb(0x15, 0x17, 0x1c),
        card: Color32::from_rgb(0x1e, 0x21, 0x27),
        border: Color32::from_rgb(0x2d, 0x31, 0x39),
        field: Color32::from_rgb(0x27, 0x2b, 0x32),
        field_hover: Color32::from_rgb(0x30, 0x35, 0x3d),
        text: Color32::from_rgb(0xe7, 0xe9, 0xec),
        muted: Color32::from_rgb(0x99, 0xa0, 0xab),
        accent: Color32::from_rgb(0x4c, 0x8d, 0xff),
        green: Color32::from_rgb(0x34, 0xc7, 0x59),
        amber: Color32::from_rgb(0xf5, 0x9e, 0x0b),
        red: Color32::from_rgb(0xf8, 0x71, 0x71),
    };

    pub fn of(ui: &egui::Ui) -> Palette {
        if ui.visuals().dark_mode { Palette::DARK } else { Palette::LIGHT }
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
    v.override_text_color = Some(p.text);
    v.weak_text_color = Some(p.muted);
    v.panel_fill = p.background;
    v.window_fill = p.card;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.extreme_bg_color = p.field;
    v.faint_bg_color = p.field;
    v.error_fg_color = p.red;
    v.warn_fg_color = p.amber;
    v.hyperlink_color = p.accent;
    v.selection.bg_fill = p.accent;
    v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    v.slider_trailing_fill = true;
    v.collapsing_header_frame = false;
    v.indent_has_left_vline = false;

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
    widgets.hovered.bg_stroke = Stroke::new(1.0, p.border);
    widgets.active.bg_stroke = Stroke::new(1.0, p.accent);
    widgets.noninteractive.corner_radius = radius;
}

/// A white (or dark) rounded panel holding one group of controls.
pub fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let p = Palette::of(ui);
    egui::Frame::new()
        .fill(p.card)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::symmetric(16, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// A card with a coloured edge, for things that need attention.
pub fn notice<R>(ui: &mut egui::Ui, accent: Color32, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let p = Palette::of(ui);
    egui::Frame::new()
        .fill(p.card)
        .stroke(Stroke::new(1.5, accent))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::symmetric(16, 14))
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
    let p = Palette::of(ui);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, p.border);
}

/// Buttons side by side in a rounded track, one of them selected.
pub fn segmented<T: PartialEq + Copy>(
    ui: &mut egui::Ui,
    value: &mut T,
    options: &[(T, &str)],
) -> Response {
    let p = Palette::of(ui);
    let mut changed = None;
    let frame = egui::Frame::new()
        .fill(p.field)
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(3))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            // In a right-aligned row the buttons are laid out from the right.
            let rtl = ui.layout().prefer_right_to_left();
            ui.horizontal(|ui| {
                let ordered: Vec<_> =
                    if rtl { options.iter().rev().collect() } else { options.iter().collect() };
                for &(option, label) in ordered {
                    let selected = *value == option;
                    let text = if selected {
                        semibold(label).color(p.text)
                    } else {
                        RichText::new(label).color(p.muted)
                    };
                    let button = egui::Button::new(text)
                        .fill(if selected { p.card } else { Color32::TRANSPARENT })
                        .stroke(if selected { Stroke::new(1.0, p.border) } else { Stroke::NONE })
                        .corner_radius(CornerRadius::same(6))
                        .min_size(Vec2::new(0.0, 26.0))
                        .selected(selected);
                    if ui.add(button).clicked() && !selected {
                        changed = Some(option);
                    }
                }
            });
        });
    let mut response = frame.response;
    if let Some(option) = changed {
        *value = option;
        response.mark_changed();
    }
    response
}

/// An on/off switch.
pub fn switch(ui: &mut egui::Ui, on: &mut bool, label: &str) -> Response {
    let p = Palette::of(ui);
    let size = Vec2::new(38.0, 22.0);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let enabled = ui.is_enabled();
    response.widget_info(|| WidgetInfo::selected(Role::Switch, enabled, *on, label));
    if ui.is_rect_visible(rect) {
        let t = ui.ctx().animate_bool_responsive(response.id, *on);
        let track = if *on { p.accent } else { p.field_hover };
        let track = if enabled { track } else { track.gamma_multiply(0.5) };
        let painter = ui.painter();
        painter.rect_filled(rect, CornerRadius::same(11), track);
        let x = egui::lerp((rect.left() + 11.0)..=(rect.right() - 11.0), t);
        painter.circle_filled(Pos2::new(x, rect.center().y), 8.5, Color32::WHITE);
    }
    response
}

/// The big button at the bottom of the window.
pub fn primary_button(ui: &mut egui::Ui, text: &str, fill: Color32) -> Response {
    let button = egui::Button::new(semibold(text).size(15.0).color(Color32::WHITE))
        .fill(fill)
        .corner_radius(CornerRadius::same(10));
    ui.add_sized([ui.available_width(), 40.0], button)
}

/// How the picture shows the link between the two computers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link {
    Off,
    Waiting,
    Connected,
    /// Connected, and keyboard and mouse drive the other computer.
    Remote,
}

/// Two monitors side by side, the other computer on the side it sits, with
/// the link between them.
pub fn monitors(ui: &mut egui::Ui, peer_left: bool, link: Link, this: &str, peer: &str) {
    let p = Palette::of(ui);
    let height = 132.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    let painter = ui.painter_at(rect);

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
        Link::Off => p.border,
        Link::Waiting => p.amber,
        Link::Connected => p.green,
        Link::Remote => p.accent,
    };
    // The link, drawn first so the monitors sit on top of its ends.
    let y = left_rect.center().y;
    let (x0, x1) = (left_rect.right() + 8.0, right_rect.left() - 8.0);
    if link == Link::Waiting || link == Link::Off {
        let mut x = x0;
        while x < x1 {
            let end = (x + 6.0).min(x1);
            painter
                .line_segment([Pos2::new(x, y), Pos2::new(end, y)], Stroke::new(2.0, line_color));
            x += 11.0;
        }
    } else {
        painter.line_segment([Pos2::new(x0, y), Pos2::new(x1, y)], Stroke::new(2.5, line_color));
        let dot = |x: f32| painter.circle_filled(Pos2::new(x, y), 4.0, line_color);
        dot(x0);
        dot(x1);
    }

    // Where keyboard and mouse are: lit up in the accent colour.
    let active_peer = link == Link::Remote;
    let active_this = link == Link::Connected;
    for (r, active, name, caption) in [
        (this_rect, active_this, this, "Bu bilgisayar"),
        (peer_rect, active_peer, peer, "Diğer bilgisayar"),
    ] {
        let fill = if active { p.accent.gamma_multiply(0.18) } else { p.field };
        let stroke =
            Stroke::new(if active { 2.0 } else { 1.5 }, if active { p.accent } else { p.border });
        painter.rect(r, CornerRadius::same(7), fill, stroke, StrokeKind::Inside);
        // Stand.
        let neck =
            Rect::from_center_size(Pos2::new(r.center().x, r.bottom() + 5.0), Vec2::new(10.0, 8.0));
        painter.rect_filled(neck, 0.0, p.border);
        let foot = Rect::from_center_size(
            Pos2::new(r.center().x, r.bottom() + 10.0),
            Vec2::new(40.0, 3.0),
        );
        painter.rect_filled(foot, CornerRadius::same(2), p.border);

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
