//! Platform appearance for the desktop window and its shared controls.

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

    ctx.set_theme(egui::ThemePreference::System);
    ctx.options_mut(|options| options.fallback_theme = Theme::Light);
    let platform = Platform::current();
    ctx.style_mut_of(Theme::Light, |style| apply(style, Palette::new(platform, Theme::Light)));
    ctx.style_mut_of(Theme::Dark, |style| apply(style, Palette::new(platform, Theme::Dark)));
}

/// Text in the semibold weight.
pub fn semibold(text: impl Into<String>) -> RichText {
    RichText::new(text).family(FontFamily::Name(SEMIBOLD_FAMILY.into())).variation("wght", 600.0)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Platform {
    Windows,
    Mac,
}

impl Platform {
    fn current() -> Self {
        if cfg!(target_os = "macos") { Self::Mac } else { Self::Windows }
    }

    fn control_height(self) -> f32 {
        match self {
            Self::Windows => 32.0,
            Self::Mac => 28.0,
        }
    }

    fn control_radius(self) -> u8 {
        match self {
            Self::Windows => 4,
            Self::Mac => 6,
        }
    }
}

#[derive(Clone, Copy)]
pub struct Palette {
    platform: Platform,
    dark: bool,
    pub window: Color32,
    pub panel: Color32,
    border: Color32,
    surface: Color32,
    hover: Color32,
    text: Color32,
    pub muted: Color32,
    pub accent: Color32,
    on_accent: Color32,
    pub green: Color32,
    pub amber: Color32,
    pub red: Color32,
}

impl Palette {
    fn new(platform: Platform, theme: Theme) -> Self {
        let (window, panel, surface, hover, border, accent) = match (platform, theme) {
            (Platform::Windows, Theme::Light) => {
                (0xf3f3f3, 0xfcfcfc, 0xffffff, 0xe9e9e9, 0xd1d1d1, 0x0067c0)
            }
            (Platform::Windows, Theme::Dark) => {
                (0x202020, 0x292929, 0x333333, 0x414141, 0x505050, 0x7ab9fa)
            }
            (Platform::Mac, Theme::Light) => {
                (0xf1f1f3, 0xffffff, 0xe7e7eb, 0xdcdce0, 0xd4d4d9, 0x005fcb)
            }
            (Platform::Mac, Theme::Dark) => {
                (0x28282b, 0x333337, 0x45454b, 0x505056, 0x5b5b62, 0x91bfff)
            }
        };
        let dark = theme == Theme::Dark;
        let (text, muted, on_accent, green, amber, red) = if dark {
            (0xf5f5f5, 0xc8c8d0, 0x13243b, 0x82d8a1, 0xffcd83, 0xffa0a7)
        } else {
            (0x202124, 0x56575e, 0xffffff, 0x1d693d, 0x864707, 0xb32132)
        };
        fn color(rgb: u32) -> Color32 {
            Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
        }
        Self {
            platform,
            dark,
            window: color(window),
            panel: color(panel),
            border: color(border),
            surface: color(surface),
            hover: color(hover),
            text: color(text),
            muted: color(muted),
            accent: color(accent),
            on_accent: color(on_accent),
            green: color(green),
            amber: color(amber),
            red: color(red),
        }
    }

    pub fn of(ui: &egui::Ui) -> Self {
        let theme = if ui.visuals().dark_mode { Theme::Dark } else { Theme::Light };
        Self::new(Platform::current(), theme)
    }
}

fn apply(style: &mut egui::Style, p: Palette) {
    use egui::FontFamily::{Monospace, Proportional};
    let body_size = if p.platform == Platform::Mac { 13.0 } else { 14.0 };
    style.text_styles = [
        (TextStyle::Heading, FontId::new(20.0, Proportional)),
        (TextStyle::Body, FontId::new(body_size, Proportional)),
        (TextStyle::Button, FontId::new(body_size, Proportional)),
        (TextStyle::Small, FontId::new(11.5, Proportional)),
        (TextStyle::Monospace, FontId::new(13.0, Monospace)),
    ]
    .into();
    style.spacing.item_spacing = Vec2::new(8.0, 8.0);
    style.spacing.button_padding = Vec2::new(12.0, 6.0);
    style.spacing.interact_size.y = p.platform.control_height();
    style.spacing.slider_width = 150.0;
    if p.platform == Platform::Windows {
        style.spacing.scroll = egui::style::ScrollStyle::thin();
    }

    let v = &mut style.visuals;
    v.dark_mode = p.dark;
    v.override_text_color = Some(p.text);
    v.weak_text_color = Some(p.muted);
    // The window paints its own background.
    v.panel_fill = Color32::TRANSPARENT;
    v.window_fill = p.panel;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.extreme_bg_color = p.surface;
    v.faint_bg_color = p.surface;
    v.error_fg_color = p.red;
    v.warn_fg_color = p.amber;
    v.hyperlink_color = p.accent;
    v.selection.bg_fill = p.accent;
    v.selection.stroke = Stroke::new(1.0, p.on_accent);
    v.slider_trailing_fill = true;
    v.collapsing_header_frame = false;
    v.indent_has_left_vline = false;

    // The widgets egui still draws (slider, collapsing header) change colour
    // on hover only: same outline width, no growing.
    let radius = CornerRadius::same(p.platform.control_radius());
    let widgets = &mut v.widgets;
    widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    for (w, fill) in [
        (&mut widgets.inactive, p.surface),
        (&mut widgets.hovered, p.hover),
        (&mut widgets.active, p.hover),
        (&mut widgets.open, p.surface),
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

/// Paint the window surface behind the panels.
pub fn paint_backdrop(ui: &egui::Ui) {
    ui.painter().rect_filled(ui.ctx().content_rect(), 0.0, Palette::of(ui).window);
}

// ------------------------------------------------------------- containers

/// A panel holding one group of controls.
pub fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let p = Palette::of(ui);
    egui::Frame::new()
        .fill(p.panel)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(12))
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
        .fill(mix(Palette::of(ui).panel, accent, 0.08))
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
    ui.label(semibold(title).size(14.0));
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
    ui.painter().rect_filled(rect, 0.0, Palette::of(ui).border);
}

// ---------------------------------------------------------------- buttons

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Primary,
    Secondary,
    Ghost,
    Danger,
}

pub fn button(ui: &mut egui::Ui, label: &str, kind: Kind) -> Response {
    action_button(ui, label, kind, 0.0)
}

pub fn action_button(ui: &mut egui::Ui, label: &str, kind: Kind, min_width: f32) -> Response {
    let p = Palette::of(ui);
    let enabled = ui.is_enabled();
    let color = match kind {
        Kind::Primary => p.on_accent,
        Kind::Danger => p.red,
        _ => p.text,
    };
    let galley = text_galley(ui, semibold(label).size(13.0).color(color));
    let width = (galley.size().x + 24.0).max(56.0).max(min_width);
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, p.platform.control_height()), Sense::click());
    response.widget_info(|| WidgetInfo::labeled(Role::Button, enabled, label));
    if !ui.is_rect_visible(rect) {
        return response;
    }

    let (fill, border) = match kind {
        Kind::Primary => (p.accent, Color32::TRANSPARENT),
        Kind::Ghost => (Color32::TRANSPARENT, Color32::TRANSPARENT),
        _ => (p.surface, p.border),
    };
    let fill = if response.is_pointer_button_down_on() {
        mix(fill, p.text, 0.16)
    } else if response.hovered() {
        mix(fill, p.text, 0.07)
    } else {
        fill
    };
    let radius = p.platform.control_radius();
    let painter = ui.painter();
    painter.rect_filled(rect, radius, if enabled { fill } else { p.panel });
    painter.rect_stroke(rect, radius, Stroke::new(1.0, border), StrokeKind::Inside);
    if response.has_focus() {
        painter.rect_stroke(rect, radius, Stroke::new(2.0, p.accent), StrokeKind::Outside);
    }
    let text = if enabled { color } else { p.muted };
    painter.galley_with_override_text_color(rect.center() - galley.size() / 2.0, galley, text);
    response
}

/// Buttons side by side with a fixed selection indicator.
pub fn segmented<T: PartialEq + Copy>(
    ui: &mut egui::Ui,
    value: &mut T,
    options: &[(T, &str)],
) -> Response {
    let p = Palette::of(ui);
    let height = p.platform.control_height();
    let inset = 2.0;
    let widths: Vec<_> = options
        .iter()
        .map(|&(_, label)| text_galley(ui, semibold(label).size(13.0)).size().x + 18.0)
        .collect();
    let width = widths.iter().sum::<f32>() + 2.0 * inset;
    let (track, mut response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let radius = p.platform.control_radius();
    let painter = ui.painter();
    painter.rect_filled(track, radius, p.surface);
    painter.rect_stroke(track, radius, Stroke::new(1.0, p.border), StrokeKind::Inside);
    let mut x = track.left() + inset;
    for (index, ((option, label), width)) in options.iter().zip(widths).enumerate() {
        let rect = Rect::from_min_size(
            Pos2::new(x, track.top() + inset),
            Vec2::new(width, height - 2.0 * inset),
        );
        x += width;
        let selected = *value == *option;
        let item = ui.interact(rect, response.id.with(index), Sense::click());
        item.widget_info(|| WidgetInfo::selected(Role::Button, ui.is_enabled(), selected, *label));
        if selected {
            let fill =
                if p.platform == Platform::Mac { p.panel } else { mix(p.panel, p.accent, 0.12) };
            painter.rect_filled(rect, radius.saturating_sub(1), fill);
            painter.rect_stroke(
                rect,
                radius.saturating_sub(1),
                Stroke::new(1.0, p.border),
                StrokeKind::Inside,
            );
        } else if item.hovered() {
            painter.rect_filled(rect, radius.saturating_sub(1), p.hover);
        }
        let color = if selected && ui.is_enabled() { p.text } else { p.muted };
        let galley = text_galley(ui, semibold(*label).size(13.0).color(color));
        painter.galley(rect.center() - galley.size() / 2.0, galley, color);
        if item.has_focus() {
            painter.rect_stroke(rect, radius, Stroke::new(2.0, p.accent), StrokeKind::Inside);
        }
        if item.clicked() && !selected {
            *value = *option;
            response.mark_changed();
        }
    }
    response
}

/// Windows uses a left-aligned tab row and macOS uses a compact toolbar group.
pub fn tabs<T: PartialEq + Copy>(ui: &mut egui::Ui, value: &mut T, options: &[(T, &str)]) {
    let p = Palette::of(ui);
    if p.platform == Platform::Mac {
        ui.vertical_centered(|ui| segmented(ui, value, options));
        divider(ui);
        return;
    }

    let height = 36.0;
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    let painter = ui.painter();
    painter.hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, p.border));
    let mut x = rect.left();
    for (i, &(option, label)) in options.iter().enumerate() {
        let selected = *value == option;
        let color = if selected { p.text } else { p.muted };
        let galley = text_galley(ui, semibold(label).size(14.0).color(color));
        let width = (galley.size().x + 30.0).max(80.0);
        let item_rect = Rect::from_min_size(Pos2::new(x, rect.top()), Vec2::new(width, height));
        x += width + 4.0;
        let item = ui.interact(item_rect, response.id.with(i), Sense::click());
        item.widget_info(|| WidgetInfo::selected(Role::Button, true, selected, label));
        let radius = p.platform.control_radius();
        if item.hovered() || item.is_pointer_button_down_on() {
            painter.rect_filled(item_rect.shrink(2.0), radius, p.hover);
        }
        painter.galley(item_rect.center() - galley.size() / 2.0, galley, color);
        if selected {
            let y = item_rect.bottom() - 2.0;
            painter.hline(
                (item_rect.left() + 18.0)..=(item_rect.right() - 18.0),
                y,
                Stroke::new(3.0, p.accent),
            );
        }
        if item.has_focus() {
            painter.rect_stroke(
                item_rect.shrink(2.0),
                radius,
                Stroke::new(2.0, p.accent),
                StrokeKind::Inside,
            );
        }
        if item.clicked() {
            *value = option;
        }
    }
}

/// An on/off switch with the platform's thumb and track proportions.
pub fn switch(ui: &mut egui::Ui, on: &mut bool, label: &str) -> Response {
    let p = Palette::of(ui);
    let track_size = match p.platform {
        Platform::Windows => Vec2::new(40.0, 20.0),
        Platform::Mac => Vec2::new(34.0, 20.0),
    };
    let (hit, mut response) = ui.allocate_exact_size(
        Vec2::new(track_size.x + 4.0, p.platform.control_height()),
        Sense::click(),
    );
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let enabled = ui.is_enabled();
    response.widget_info(|| WidgetInfo::selected(Role::Switch, enabled, *on, label));

    if ui.is_rect_visible(hit) {
        let rect = Rect::from_center_size(hit.center(), track_size);
        let radius = CornerRadius::same(10);
        let (mut fill, thumb, border) = if *on && p.platform == Platform::Mac {
            let blue = if p.dark { Color32::from_rgb(27, 104, 194) } else { p.accent };
            (blue, Color32::WHITE, blue)
        } else if *on {
            (p.accent, p.on_accent, p.accent)
        } else if p.platform == Platform::Mac {
            (p.border, Color32::WHITE, p.muted)
        } else {
            (p.surface, p.muted, p.muted)
        };
        if response.hovered() {
            fill = mix(fill, p.text, 0.07);
        }
        let painter = ui.painter();
        painter.rect_filled(rect, radius, if enabled { fill } else { mix(fill, p.panel, 0.5) });
        painter.rect_stroke(rect, radius, Stroke::new(1.0, border), StrokeKind::Inside);
        let x = if *on { rect.right() - 10.0 } else { rect.left() + 10.0 };
        let size = if p.platform == Platform::Mac { 8.0 } else { 6.0 };
        painter.circle_filled(Pos2::new(x, rect.center().y), size, thumb);
        if response.has_focus() {
            painter.rect_stroke(hit, radius, Stroke::new(2.0, p.accent), StrokeKind::Outside);
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
    let height = if style == TextStyle::Heading { 40.0 } else { p.platform.control_height() };
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(width.min(ui.available_width()).max(24.0), height),
        Sense::hover(),
    );
    let radius = p.platform.control_radius();
    let fill =
        if ui.rect_contains_pointer(rect) { mix(p.surface, p.hover, 0.5) } else { p.surface };
    ui.painter().rect_filled(rect, radius, fill);

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
    let edge =
        if response.has_focus() { Stroke::new(2.0, p.accent) } else { Stroke::new(1.0, p.border) };
    ui.painter().rect_stroke(rect, radius, edge, StrokeKind::Inside);
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
            if active { (with_alpha(line, 0.22), line) } else { (p.surface, p.border) };
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

#[cfg(test)]
mod tests {
    use super::*;

    fn contrast(left: Color32, right: Color32) -> f32 {
        fn luminance(color: Color32) -> f32 {
            let linear: egui::Rgba = color.into();
            0.2126 * linear.r() + 0.7152 * linear.g() + 0.0722 * linear.b()
        }

        let (left, right) = (luminance(left), luminance(right));
        (left.max(right) + 0.05) / (left.min(right) + 0.05)
    }

    #[test]
    fn platform_palettes_keep_text_and_controls_readable() {
        for platform in [Platform::Windows, Platform::Mac] {
            for theme in [Theme::Light, Theme::Dark] {
                let p = Palette::new(platform, theme);
                for surface in [p.window, p.panel, p.surface, p.hover] {
                    assert!(contrast(p.text, surface) >= 4.5);
                    assert!(contrast(p.muted, surface) >= 4.5);
                }
                assert!(contrast(p.on_accent, p.accent) >= 4.5);
                for feedback in [p.green, p.amber, p.red] {
                    assert!(contrast(feedback, p.panel) >= 4.5);
                }
            }
        }
    }

    #[test]
    fn follows_system_theme_changes() {
        let context = egui::Context::default();
        install(&context);
        for theme in [Theme::Light, Theme::Dark, Theme::Light] {
            let input = egui::RawInput { system_theme: Some(theme), ..Default::default() };
            let mut output = context.run_ui(input, |ui| {
                let palette = Palette::of(ui);
                assert_eq!(palette.dark, theme == Theme::Dark);
                assert_eq!(ui.visuals().text_color(), palette.text);
            });
            output.textures_delta.clear();
        }
    }
}
