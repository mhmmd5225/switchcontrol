//! The window: settings, start/stop, connection status and PIN pairing.
//!
//! The engine runs on its own thread; its reports come back over a channel
//! and wake the window. Settings are kept in `settings.toml` in the data
//! folder, in the same format `--config` reads.

use std::collections::VecDeque;
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use eframe::egui::{self, RichText};
use sc_core::Side;
use sc_platform::Permissions;
use sc_platform::sys;

use crate::autostart;
use crate::config::{self, FileConfig, RunFlags};
use crate::discovery::Listener;
use crate::edgebar::{EdgeBar, Strip};
use crate::engine::{Engine, LinkState, Report};
use crate::identity::{self, Fingerprint, TrustStore};
use crate::net::{self, PinSource, Waiting};
use crate::pairing::normalize_pin;
use crate::theme::{self, Kind, Link, Palette};
use crate::tray::{Tray, TrayCommand};

const SETTINGS_FILE: &str = "settings.toml";
const LOG_LINES: usize = 500;
const MIN_WINDOW: egui::Vec2 = egui::vec2(340.0, 260.0);
const TWO_COLUMNS: f32 = 700.0;

/// The emergency hotkey as this OS spells it.
const HOTKEY: &str =
    if cfg!(target_os = "macos") { "Ctrl+Option+Shift+Esc" } else { "Ctrl+Alt+Shift+Esc" };

/// Opens the window. `at_login`: started by [`autostart`], so hide in the
/// tray and start sharing right away.
pub fn run(at_login: bool) -> Result<(), String> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("SwitchControl")
            .with_inner_size([800.0, 520.0])
            .with_min_inner_size(MIN_WINDOW)
            .with_icon(Arc::new(
                eframe::icon_data::from_png_bytes(include_bytes!("../../assets/icon-256.png"))
                    .expect("bundled icon is a valid PNG"),
            )),
        ..Default::default()
    };
    let data_dir = identity::default_dir();
    // Other systems have no capture backend; the window still works for trying it out.
    let dry_run = cfg!(not(any(windows, target_os = "macos")));
    eframe::run_native(
        "SwitchControl",
        options,
        Box::new(move |cc| {
            let mut app = GuiApp::new(cc.egui_ctx.clone(), data_dir, dry_run);
            match Tray::new(&cc.egui_ctx) {
                Ok(tray) => app.tray = Some(tray),
                Err(e) => app.push_log(format!("no tray icon: {e}")),
            }
            if at_login {
                app.launched_at_login();
            }
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Listen,
    Connect,
}

/// The two screens: connecting, and everything else that can be changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Connection,
    Settings,
}

/// The editable settings.
#[derive(Debug, Clone)]
struct Form {
    side: Side,
    mode: Mode,
    address: String,
    name: String,
    speed: f64,
    swap: bool,
    edge_delay_ms: u64,
    edge_bar: bool,
    clipboard: bool,
    announce: bool,
    /// Kept from the file; the window has no field for it.
    listen: Option<String>,
}

impl Form {
    fn from_file(file: FileConfig) -> Form {
        let side = match file.side.as_deref() {
            Some("right") => Side::Right,
            Some("left") => Side::Left,
            // Most setups put the Mac to the right of the PC.
            _ if cfg!(target_os = "macos") => Side::Left,
            _ => Side::Right,
        };
        Form {
            side,
            mode: if file.connect.is_some() { Mode::Connect } else { Mode::Listen },
            address: file.connect.unwrap_or_default(),
            name: file.name.unwrap_or_else(config::default_name),
            speed: file.speed.unwrap_or(1.0),
            swap: file.swap_ctrl_cmd.unwrap_or(true),
            edge_delay_ms: file.edge_delay_ms.unwrap_or(0),
            edge_bar: file.edge_bar.unwrap_or(true),
            clipboard: file.clipboard.unwrap_or(true),
            announce: file.announce.unwrap_or(true),
            listen: file.listen,
        }
    }

    fn to_file(&self) -> FileConfig {
        let connect = self.mode == Mode::Connect;
        FileConfig {
            name: Some(self.name.trim().to_string()),
            side: Some(if self.side == Side::Left { "left" } else { "right" }.into()),
            listen: if connect { None } else { self.listen.clone() },
            connect: connect.then(|| self.address.trim().to_string()),
            speed: Some(self.speed),
            swap_ctrl_cmd: Some(self.swap),
            edge_delay_ms: Some(self.edge_delay_ms),
            edge_bar: Some(self.edge_bar),
            clipboard: Some(self.clipboard),
            announce: Some(self.announce),
            data_dir: None,
        }
    }
}

/// What the engine last told us.
#[derive(Default)]
struct View {
    link: Option<LinkState>,
    remote: bool,
    show_pin: Option<String>,
    ask_pin: Option<SocketAddr>,
    pin_input: String,
    focus_pin: bool,
    log: VecDeque<String>,
}

struct Running {
    /// Bumped on every start, so reports from a stopped engine are ignored.
    generation: u64,
    engine: Option<Engine>,
    /// Answers `Report::AskPin`; dropping it cancels a pending question.
    pin_tx: Option<Sender<Option<String>>>,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.pin_tx = None;
        if let Some(engine) = self.engine.take() {
            engine.stop();
        }
    }
}

pub struct GuiApp {
    ctx: egui::Context,
    data_dir: PathBuf,
    dry_run: bool,
    form: Form,
    page: Page,
    running: Option<Running>,
    generation: u64,
    reports_tx: Sender<(u64, Report)>,
    reports_rx: Receiver<(u64, Report)>,
    view: View,
    error: Option<String>,
    reveal_error: bool,
    perms: Permissions,
    perms_checked: Instant,
    /// Fingerprints of the paired computers.
    paired: Vec<Fingerprint>,
    local_ip: Option<IpAddr>,
    edge_bar: EdgeBar,
    /// The edge bar has a separate viewport while sharing is enabled.
    strip: Option<Strip>,
    strip_for: Option<Side>,
    browsing: bool,
    in_tray: bool,
    fit_on_first_frame: bool,
    tray: Option<Tray>,
    /// Hide into the tray once the first frame has opened the edge bar.
    hide_on_first_frame: bool,
    /// Closing the window really quits (picked from the tray menu).
    quitting: bool,
    /// Whether the OS starts SwitchControl at login.
    autostart: bool,
}

impl GuiApp {
    pub fn new(ctx: egui::Context, data_dir: PathBuf, dry_run: bool) -> GuiApp {
        theme::install(&ctx);
        let file = config::load(&data_dir.join(SETTINGS_FILE)).unwrap_or_default();
        let (reports_tx, reports_rx) = mpsc::channel();
        GuiApp {
            ctx,
            form: Form::from_file(file),
            page: Page::Connection,
            paired: paired_list(&data_dir),
            data_dir,
            dry_run,
            running: None,
            generation: 0,
            reports_tx,
            reports_rx,
            view: View::default(),
            error: None,
            reveal_error: false,
            perms: sys::permissions(),
            perms_checked: Instant::now(),
            local_ip: local_ip(),
            edge_bar: EdgeBar::default(),
            strip: None,
            strip_for: None,
            browsing: false,
            in_tray: false,
            fit_on_first_frame: true,
            tray: None,
            hide_on_first_frame: false,
            quitting: false,
            autostart: autostart::is_enabled(),
        }
    }

    /// Started at login: go to the tray and share with the saved settings.
    fn launched_at_login(&mut self) {
        self.hide_on_first_frame = self.tray.is_some();
        if self.form.mode == Mode::Listen
            || !self.form.address.trim().is_empty()
            || !self.paired.is_empty()
        {
            self.start();
        }
    }

    /// Carries out what was picked from the tray icon, and keeps its status current.
    fn tray_logic(&mut self, ctx: &egui::Context) {
        let Some(tray) = &self.tray else { return };
        for command in tray.commands() {
            match command {
                TrayCommand::Show => {
                    self.in_tray = false;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                TrayCommand::StartStop => {
                    if self.running.is_some() {
                        self.stop()
                    } else {
                        self.start()
                    }
                }
                TrayCommand::Quit => {
                    self.quitting = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
        // The close button hides the window; the tray icon brings it back.
        if ctx.input(|i| i.viewport().close_requested()) && !self.quitting {
            self.in_tray = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
        let status = self.status().0;
        let running = self.running.is_some();
        if let Some(tray) = &mut self.tray {
            tray.update(&status, running);
        }
    }

    fn wants_edge_bar(&self) -> bool {
        self.running.is_some() && self.form.edge_bar && !self.ctx.embed_viewports()
    }

    /// Keep the overlay registered even when only its window is visible.
    fn edge_bar_ui(&mut self, ctx: &egui::Context) {
        if !self.wants_edge_bar() {
            self.strip_for = None;
            self.strip = None;
            return;
        }
        if self.strip_for != Some(self.form.side) {
            self.strip_for = Some(self.form.side);
            let scale = if cfg!(windows) { ctx.pixels_per_point() } else { 1.0 };
            let screens: Vec<_> = sys::monitors().into_iter().map(|m| m.bounds).collect();
            self.strip = Strip::new(&screens, self.form.side, scale);
        }
        if let Some(strip) = self.strip {
            self.edge_bar.show(ctx, strip);
        }
    }

    fn window_hidden(&self, ctx: &egui::Context) -> bool {
        self.in_tray || ctx.input(|i| i.viewport().visible() == Some(false))
    }

    fn update_discovery(&mut self, visible: bool) {
        let browsing = visible
            && self.page == Page::Connection
            && self.form.mode == Mode::Connect
            && self.running.is_none()
            && !self.ctx.embed_viewports();
        if browsing == self.browsing {
            return;
        }
        self.browsing = browsing;
        if browsing {
            let ctx = self.ctx.clone();
            Listener::global().set_waker(move || ctx.request_repaint_of(egui::ViewportId::ROOT));
        } else {
            Listener::global().clear_waker();
        }
    }

    fn start(&mut self) {
        self.error = None;
        self.reveal_error = true;
        // Without an address only a paired computer heard on the network will do.
        if self.form.mode == Mode::Connect
            && self.form.address.trim().is_empty()
            && self.paired.is_empty()
        {
            self.error = Some("Bağlanılacak bilgisayarın adresini girin.".into());
            return;
        }
        let file = self.form.to_file();
        if let Err(e) = config::save(&self.data_dir.join(SETTINGS_FILE), &file) {
            self.push_log(format!("could not save settings: {e}"));
        }
        let flags = RunFlags { dry_run: self.dry_run, ..RunFlags::default() };
        let mut settings = match config::resolve(file, flags) {
            Ok(settings) => settings,
            Err(e) => {
                self.error = Some(e);
                return;
            }
        };
        settings.data_dir = self.data_dir.clone();

        self.generation += 1;
        let generation = self.generation;
        let (tx, ctx, bar) = (self.reports_tx.clone(), self.ctx.clone(), self.edge_bar.clone());
        let report: crate::engine::Reporter = Arc::new(move |r| match r {
            // Only the bar's own window needs redrawing.
            Report::Edge(hint) => bar.set(&ctx, hint),
            r => {
                let _ = tx.send((generation, r));
                ctx.request_repaint_of(egui::ViewportId::ROOT);
            }
        });
        let (pin_tx, pin_rx) = mpsc::channel::<Option<String>>();
        let ask_report = report.clone();
        let pin = PinSource::Ask(Box::new(move |addr| {
            // Drop answers meant for an earlier question.
            while pin_rx.try_recv().is_ok() {}
            ask_report(Report::AskPin(addr));
            pin_rx.recv_timeout(net::PAIRING_TIMEOUT).ok().flatten()
        }));

        self.view = View { log: std::mem::take(&mut self.view.log), ..View::default() };
        // Monitors may have changed since the bar was placed.
        self.strip_for = None;
        self.running = Some(Running {
            generation,
            engine: Some(Engine::start(settings, pin, report)),
            pin_tx: Some(pin_tx),
        });
    }

    fn stop(&mut self) {
        // Dropping stops the engine and waits for it.
        self.running = None;
        self.edge_bar.set(&self.ctx, None);
        self.view.link = None;
        self.view.remote = false;
        self.view.show_pin = None;
        self.view.ask_pin = None;
        self.view.pin_input.clear();
        self.paired = paired_list(&self.data_dir);
    }

    fn answer_pin(&mut self, pin: Option<String>) {
        if let Some(tx) = self.running.as_ref().and_then(|r| r.pin_tx.as_ref()) {
            let _ = tx.send(pin);
        }
        self.view.ask_pin = None;
        self.view.pin_input.clear();
    }

    fn push_log(&mut self, text: String) {
        if self.view.log.len() == LOG_LINES {
            self.view.log.pop_front();
        }
        self.view.log.push_back(text);
    }

    /// Takes in what the engine reported since the last frame.
    fn poll(&mut self) {
        while let Ok((generation, report)) = self.reports_rx.try_recv() {
            if self.running.as_ref().map(|r| r.generation) != Some(generation) {
                continue;
            }
            match report {
                Report::Log(text) => self.push_log(text),
                Report::Link(state) => {
                    // A new session (or the end of one) ends any pairing on screen.
                    self.view.show_pin = None;
                    self.view.ask_pin = None;
                    if matches!(state, LinkState::Connected { .. }) {
                        self.paired = paired_list(&self.data_dir);
                    } else {
                        self.view.remote = false;
                    }
                    self.view.link = Some(state);
                }
                Report::Remote(on) => self.view.remote = on,
                // Handled by the reporter itself.
                Report::Edge(_) => {}
                Report::ShowPin(pin) => self.view.show_pin = Some(pin),
                Report::AskPin(addr) => {
                    self.view.ask_pin = Some(addr);
                    self.view.pin_input.clear();
                    self.view.focus_pin = true;
                }
                Report::Stopped(result) => {
                    if let Err(e) = result {
                        self.push_log(format!("stopped: {e}"));
                        self.error = Some(explain(&e));
                        self.reveal_error = true;
                    }
                    self.stop();
                }
            }
        }
    }

    /// Headline, detail line and picture for the current state.
    fn status(&self) -> (String, String, Link) {
        match (&self.running, &self.view.link) {
            (None, _) => ("Durduruldu".into(), String::new(), Link::Off),
            (Some(_), None) => ("Başlatılıyor…".into(), String::new(), Link::Waiting),
            (Some(_), Some(LinkState::Waiting(Waiting::Listening(addr)))) => {
                let detail = match self.local_ip {
                    Some(ip) => format!("Diğer bilgisayardan {ip} adresine bağlanın"),
                    None => format!("Port {}", port_of(addr)),
                };
                ("Bağlantı bekleniyor".into(), detail, Link::Waiting)
            }
            (Some(_), Some(LinkState::Waiting(Waiting::Connecting(addr)))) => {
                if addr.is_empty() {
                    (
                        "Eşleşmiş bilgisayar aranıyor…".into(),
                        "Ağda görünen eşleşmiş bilgisayara kendiliğinden bağlanılacak.".into(),
                        Link::Waiting,
                    )
                } else {
                    ("Bağlanıyor…".into(), addr.clone(), Link::Waiting)
                }
            }
            (Some(_), Some(LinkState::Connected { peer, .. })) => {
                if self.view.remote {
                    (
                        format!("Kontrol {peer} bilgisayarında"),
                        "Geri dönmek için imleci ortak kenara getirin.".into(),
                        Link::Remote,
                    )
                } else {
                    (
                        format!("Bağlı: {peer}"),
                        "İmleci ortak kenardan geçirerek diğer bilgisayara geçin.".into(),
                        Link::Connected,
                    )
                }
            }
        }
    }

    fn header_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label(theme::semibold("SwitchControl").size(22.0));
            if self.dry_run {
                ui.label(RichText::new("Deneme modu").small().weak());
            }
        });
        theme::tabs(
            ui,
            &mut self.page,
            &[(Page::Connection, "Bağlantı"), (Page::Settings, "Ayarlar")],
        );
    }

    fn status_ui(&self, ui: &mut egui::Ui) {
        let (title, detail, link) = self.status();
        theme::card(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                theme::status_indicator(ui, link);
                ui.label(theme::semibold(title).size(20.0));
            });
            if !detail.is_empty() {
                ui.label(RichText::new(detail).weak());
            }
        });
    }

    fn layout_ui(&mut self, ui: &mut egui::Ui) {
        let (_, _, link) = self.status();
        theme::card(ui, |ui| {
            ui.label(theme::semibold("Ekran düzeni").size(17.0));
            theme::monitors(ui, self.form.side == Side::Left, link);
            ui.add_enabled_ui(self.running.is_none(), |ui| {
                ui.label("Diğer bilgisayar");
                theme::segmented(
                    ui,
                    &mut self.form.side,
                    &[(Side::Left, "Solda"), (Side::Right, "Sağda")],
                );
            });
        });
    }

    fn permissions_ui(&mut self, ui: &mut egui::Ui) {
        if self.perms.all_granted() {
            return;
        }
        ui.add_space(8.0);
        let p = Palette::of(ui);
        theme::notice(ui, p.amber, |ui| {
            ui.label(theme::semibold("macOS izinleri gerekli"));
            ui.label(
                RichText::new(
                    "Klavye ve fareyi paylaşmak için Sistem Ayarları > Gizlilik ve Güvenlik \
                     altında SwitchControl'e şu izinleri verin:",
                )
                .color(p.muted),
            );
            let rows = [
                ("Girdi İzleme", self.perms.listen, "Privacy_ListenEvent"),
                ("Erişilebilirlik", self.perms.post, "Privacy_Accessibility"),
            ];
            for (name, granted, pane) in rows {
                theme::row(ui, name, |ui| {
                    if granted {
                        ui.label(RichText::new("verildi").color(p.green));
                    } else if theme::button(ui, "Ayarları aç", Kind::Secondary).clicked() {
                        open_privacy_pane(pane);
                    }
                });
            }
            ui.horizontal_wrapped(|ui| {
                if theme::button(ui, "İzin iste", Kind::Secondary).clicked() {
                    sys::init();
                }
                ui.label(
                    RichText::new("İzin verdikten sonra uygulamayı yeniden açın.").small().weak(),
                );
            });
        });
    }

    fn pairing_ui(&mut self, ui: &mut egui::Ui) {
        let p = Palette::of(ui);
        if let Some(pin) = &self.view.show_pin {
            ui.add_space(8.0);
            theme::notice(ui, p.accent, |ui| {
                ui.vertical_centered(|ui| {
                    ui.label(theme::semibold("Eşleştirme isteği"));
                    ui.label(
                        RichText::new(format!("{} {}", &pin[..3], &pin[3..]))
                            .monospace()
                            .size(36.0)
                            .color(p.accent),
                    );
                    ui.label(RichText::new("Bu PIN'i diğer bilgisayara girin.").color(p.muted));
                });
            });
        }
        if let Some(addr) = self.view.ask_pin {
            ui.add_space(8.0);
            let mut answer = None;
            theme::notice(ui, p.accent, |ui| {
                ui.label(theme::semibold(format!("{} ile eşleştirme", addr.ip())));
                let label = ui.label(
                    RichText::new("Diğer bilgisayarda görünen 6 haneli PIN:").color(p.muted),
                );
                ui.horizontal_wrapped(|ui| {
                    let edit = theme::text_field(
                        ui,
                        &mut self.view.pin_input,
                        "123 456",
                        150.0,
                        egui::TextStyle::Heading,
                    )
                    .labelled_by(label.id);
                    if std::mem::take(&mut self.view.focus_pin) {
                        edit.request_focus();
                        edit.scroll_to_me(None);
                    }
                    let entered =
                        edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    let pin = normalize_pin(&self.view.pin_input);
                    let pair = ui
                        .add_enabled_ui(pin.is_some(), |ui| {
                            theme::button(ui, "Eşleştir", Kind::Primary)
                        })
                        .inner;
                    if pin.is_some() && (pair.clicked() || entered) {
                        answer = Some(pin);
                    }
                    if theme::button(ui, "İptal", Kind::Ghost).clicked() {
                        answer = Some(None);
                    }
                });
            });
            if let Some(answer) = answer {
                self.answer_pin(answer);
            }
        }
    }

    fn connection_ui(&mut self, ui: &mut egui::Ui) {
        let enabled = self.running.is_none();
        let form = &mut self.form;
        theme::card(ui, |ui| {
            ui.label(theme::semibold("Hızlı bağlantı").size(17.0));
            ui.add_enabled_ui(enabled, |ui| {
                theme::segmented(
                    ui,
                    &mut form.mode,
                    &[(Mode::Connect, "Bağlan"), (Mode::Listen, "Bağlantı bekle")],
                );
                theme::divider(ui);
                match form.mode {
                    Mode::Connect => {
                        if enabled {
                            found_ui(ui, &mut form.address, &self.paired);
                        }
                        let label = ui.label("Bilgisayar adresi");
                        let hint = if self.paired.is_empty() {
                            "Örn. 192.168.1.20"
                        } else {
                            "Boş bırakırsanız eşleşmiş bilgisayar aranır"
                        };
                        theme::text_field(
                            ui,
                            &mut form.address,
                            hint,
                            ui.available_width(),
                            egui::TextStyle::Body,
                        )
                        .labelled_by(label.id);
                    }
                    Mode::Listen => {
                        ui.label("Bu bilgisayarın adresi");
                        ui.horizontal_wrapped(|ui| {
                            if let Some(ip) = self.local_ip {
                                ui.label(theme::semibold(ip.to_string()));
                                if theme::button(ui, "Kopyala", Kind::Ghost).clicked() {
                                    ui.ctx().copy_text(ip.to_string());
                                }
                            } else {
                                ui.label(RichText::new("Adres bulunamadı").weak());
                            }
                        });
                        let label = "Ağda görünür ol";
                        theme::row(ui, label, |ui| theme::switch(ui, &mut form.announce, label));
                    }
                }
            });
            if let Some(error) = &self.error {
                let label = ui.label(RichText::new(error).color(Palette::of(ui).red));
                if std::mem::take(&mut self.reveal_error) {
                    label.scroll_to_me(None);
                }
            }
        });
    }

    fn behavior_ui(&mut self, ui: &mut egui::Ui) {
        let enabled = self.running.is_none();
        let form = &mut self.form;
        ui.add_enabled_ui(enabled, |ui| {
            theme::section(ui, "Davranış");
            theme::card(ui, |ui| {
                let (label, slider) = theme::row(ui, "İmleç hızı", |ui| {
                    // Right to left: the value ends up after the slider.
                    ui.label(format!("{:.2}×", form.speed));
                    ui.add(
                        egui::Slider::new(&mut form.speed, 0.25..=4.0)
                            .logarithmic(true)
                            .show_value(false),
                    )
                });
                slider.labelled_by(label.id);
                theme::divider(ui);
                let swap = "Ctrl ile Cmd yer değiştirsin";
                theme::row(ui, swap, |ui| theme::switch(ui, &mut form.swap, swap));
                theme::divider(ui);
                theme::row(ui, "Geçiş", |ui| {
                    theme::segmented(
                        ui,
                        &mut form.edge_delay_ms,
                        &[(0, "Değince"), (300, "Kısa itince"), (600, "Uzun itince")],
                    )
                });
                theme::divider(ui);
                let bar = "Kenar çubuğu";
                theme::row(ui, bar, |ui| theme::switch(ui, &mut form.edge_bar, bar));
                theme::divider(ui);
                let clip = "Pano paylaşımı";
                theme::row(ui, clip, |ui| theme::switch(ui, &mut form.clipboard, clip));
            });
        });
    }

    fn app_ui(&mut self, ui: &mut egui::Ui) {
        let running = self.running.is_some();
        theme::section(ui, "Uygulama");
        theme::card(ui, |ui| {
            ui.add_enabled_ui(!running, |ui| {
                let (label, edit) = theme::row(ui, "Bu bilgisayarın adı", |ui| {
                    theme::text_field(ui, &mut self.form.name, "", 190.0, egui::TextStyle::Body)
                });
                edit.labelled_by(label.id);
            });
            theme::divider(ui);
            let label = "Oturum açılınca başlat";
            let mut on = self.autostart;
            let (_, switch) = theme::row(ui, label, |ui| theme::switch(ui, &mut on, label));
            if switch.changed() {
                match autostart::set(on) {
                    Ok(()) => self.autostart = on,
                    Err(e) => self.error = Some(format!("Otomatik başlatma ayarlanamadı: {e}")),
                }
            }
            if self.tray.is_some() {
                ui.label(
                    RichText::new("Pencereyi kapatınca simgeye iner; çıkış simgenin menüsünde.")
                        .small()
                        .weak(),
                );
            }
            theme::divider(ui);
            let mut forget = false;
            theme::row(ui, &format!("Eşleşmiş bilgisayar: {}", self.paired.len()), |ui| {
                let can_forget = !running && !self.paired.is_empty();
                forget = ui
                    .add_enabled_ui(can_forget, |ui| {
                        theme::button(ui, "Eşleşmeleri unut", Kind::Danger)
                    })
                    .inner
                    .clicked();
            });
            if forget {
                match TrustStore::load(&self.data_dir).and_then(|mut t| t.clear()) {
                    Ok(()) => self.push_log("forgot all paired computers".into()),
                    Err(e) => self.error = Some(format!("Eşleşmeler silinemedi: {e}")),
                }
                self.paired = paired_list(&self.data_dir);
            }
        });
    }

    fn log_ui(&self, ui: &mut egui::Ui) {
        ui.add_space(10.0);
        theme::card(ui, |ui| {
            egui::CollapsingHeader::new("Günlük").default_open(false).show(ui, |ui| {
                egui::ScrollArea::vertical().max_height(180.0).stick_to_bottom(true).show(
                    ui,
                    |ui| {
                        for line in &self.view.log {
                            ui.label(RichText::new(line).monospace().small());
                        }
                    },
                );
            });
        });
    }

    fn footer_ui(&mut self, ui: &mut egui::Ui) {
        let running = self.running.is_some();
        let (text, kind) = match (running, self.form.mode) {
            (true, _) => ("Durdur", Kind::Danger),
            (false, Mode::Connect) => ("Bağlantıyı başlat", Kind::Primary),
            (false, Mode::Listen) => ("Beklemeyi başlat", Kind::Primary),
        };
        if theme::big_button(ui, text, kind).clicked() {
            if running { self.stop() } else { self.start() }
        }
        if running {
            ui.label(
                RichText::new(format!(
                    "{HOTKEY} klavye ve fareyi her zaman bu bilgisayara getirir"
                ))
                .small()
                .weak(),
            );
        }
    }
}

impl eframe::App for GuiApp {
    /// Also runs while the window is hidden in the tray.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll();
        self.tray_logic(ctx);
        let hidden = self.window_hidden(ctx);
        self.update_discovery(!hidden);
        if hidden && self.wants_edge_bar() && self.strip_for.is_none() {
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaintWhileHidden);
            ctx.request_repaint_of(egui::ViewportId::ROOT);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.window_hidden(ui.ctx()) {
            self.edge_bar_ui(ui.ctx());
            return;
        }
        if std::mem::take(&mut self.fit_on_first_frame) {
            let (monitor, inner) =
                ui.input(|i| (i.viewport().monitor_size, i.viewport().inner_rect));
            if let (Some(monitor), Some(inner)) = (monitor, inner) {
                // Reserve space for the title bar and the OS taskbar or Dock.
                let limit = (monitor - egui::vec2(24.0, 68.0)).max(MIN_WINDOW);
                let size = inner.size().min(limit);
                if size != inner.size() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
                }
            }
        }
        if self.perms_checked.elapsed() >= Duration::from_secs(1) {
            self.perms = sys::permissions();
            self.perms_checked = Instant::now();
        }

        theme::paint_backdrop(ui);
        if self.page == Page::Connection || self.running.is_some() {
            egui::Panel::bottom("footer")
                .show_separator_line(false)
                .frame(
                    egui::Frame::new()
                        .fill(Palette::of(ui).top)
                        .inner_margin(egui::Margin::symmetric(16, 10)),
                )
                .show(ui, |ui| self.footer_ui(ui));
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(16, 12)))
            .show(ui, |ui| {
                ui.set_max_width(ui.available_width().min(1000.0));
                self.header_ui(ui);
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    self.status_ui(ui);
                    // Pairing is available from either page.
                    self.pairing_ui(ui);
                    self.permissions_ui(ui);
                    match self.page {
                        Page::Connection if ui.available_width() >= TWO_COLUMNS => {
                            ui.columns(2, |columns| {
                                self.connection_ui(&mut columns[0]);
                                self.layout_ui(&mut columns[1]);
                            });
                        }
                        Page::Connection => {
                            self.connection_ui(ui);
                            self.layout_ui(ui);
                        }
                        Page::Settings => {
                            if let Some(error) = &self.error {
                                ui.label(RichText::new(error).color(Palette::of(ui).red));
                            }
                            self.behavior_ui(ui);
                            self.app_ui(ui);
                            self.log_ui(ui);
                        }
                    }
                });
            });
        self.edge_bar_ui(ui.ctx());
        if std::mem::take(&mut self.hide_on_first_frame) {
            self.in_tray = true;
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
        self.update_discovery(!self.in_tray);
        if !self.in_tray && !self.perms.all_granted() {
            ui.ctx().request_repaint_after(Duration::from_secs(1));
        }
    }

    /// Transparent, for the edge bar's window; the main window is covered by its panel.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }

    /// Quitting (e.g. Cmd+Q) must give keyboard and mouse back first.
    fn on_exit(&mut self) {
        self.running = None;
    }
}

/// The computers heard on the network; picking one fills in its address.
fn found_ui(ui: &mut egui::Ui, address: &mut String, paired: &[Fingerprint]) {
    let listener = Listener::global();
    // Drop quiet computers even when no new beacons arrive.
    ui.ctx().request_repaint_after(Duration::from_secs(2));

    let p = Palette::of(ui);
    ui.label(theme::semibold("Ağdaki bilgisayarlar"));
    let found = listener.peers();
    if found.is_empty() {
        ui.label(
            RichText::new("Bilgisayar bulunamadı. Diğer bilgisayarda beklemeyi başlatın.")
                .small()
                .weak(),
        );
    }
    for computer in found {
        let os = match computer.os {
            sc_proto::Os::Windows => "Windows",
            sc_proto::Os::MacOs => "Mac",
            sc_proto::Os::Other => "Linux",
        };
        let paired = paired.contains(&computer.fingerprint);
        ui.push_id(computer.addr, |ui| {
            let label = format!("{} · {os} · {}", computer.name, computer.addr.ip());
            peer_choice(ui, address, computer.addr, &label);
            if paired {
                ui.label(RichText::new("Eşleşmiş").small().color(p.green));
            }
        });
    }
}

fn peer_choice(ui: &mut egui::Ui, address: &mut String, peer: SocketAddr, label: &str) {
    let peer_address = peer.to_string();
    let selected = *address == peer_address;
    if ui.add(egui::Button::selectable(selected, label).wrap()).clicked() {
        *address = peer_address;
    }
}

fn paired_list(dir: &Path) -> Vec<Fingerprint> {
    TrustStore::load(dir).map(|t| t.fingerprints()).unwrap_or_default()
}

/// The address other computers on the LAN reach us at. Connecting a UDP
/// socket only picks a route; nothing is sent.
fn local_ip() -> Option<IpAddr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:9").ok()?;
    let ip = socket.local_addr().ok()?.ip();
    (!ip.is_unspecified()).then_some(ip)
}

fn port_of(addr: &str) -> &str {
    addr.rsplit(':').next().unwrap_or(addr)
}

/// Turns the errors users can act on into instructions.
fn explain(error: &str) -> String {
    if error.contains("event tap") {
        "macOS girdi izni vermedi. Girdi İzleme ve Erişilebilirlik izinlerini verip \
         uygulamayı yeniden açın."
            .into()
    } else if error.contains("wrong PIN") {
        "PIN yanlış. Başlat'a basıp yeniden deneyin.".into()
    } else if error.contains("pairing cancelled") {
        "Eşleştirme iptal edildi.".into()
    } else if error.contains("in use") || error.contains("AddrInUse") {
        format!("Port kullanımda; SwitchControl başka bir pencerede açık olabilir. ({error})")
    } else {
        error.to_string()
    }
}

#[cfg(target_os = "macos")]
fn open_privacy_pane(pane: &str) {
    let url = format!("x-apple.systempreferences:com.apple.preference.security?{pane}");
    let _ = std::process::Command::new("open").arg(url).spawn();
}

#[cfg(not(target_os = "macos"))]
fn open_privacy_pane(_pane: &str) {}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;

    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sc-gui-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn harness(dir: &Path) -> Harness<'static, GuiApp> {
        harness_at(dir, [440.0, 680.0], 1.0)
    }

    fn harness_at(dir: &Path, size: [f32; 2], scale: f32) -> Harness<'static, GuiApp> {
        let dir = dir.to_path_buf();
        Harness::builder().with_size(size).with_pixels_per_point(scale).build_eframe(move |cc| {
            let mut app = GuiApp::new(cc.egui_ctx.clone(), dir, true);
            app.perms = Permissions { listen: true, post: true };
            app
        })
    }

    /// Steps the window until `done` holds, failing after a few seconds.
    fn step_until(harness: &mut Harness<'_, GuiApp>, what: &str, done: impl Fn(&GuiApp) -> bool) {
        let end = Instant::now() + Duration::from_secs(10);
        while !done(harness.state()) {
            assert!(Instant::now() < end, "timed out waiting for {what}");
            harness.step();
            std::thread::sleep(Duration::from_millis(20));
        }
        harness.step();
    }

    fn connected_to(app: &GuiApp) -> Option<&str> {
        match &app.view.link {
            Some(LinkState::Connected { peer, .. }) => Some(peer),
            _ => None,
        }
    }

    /// Connecting fits on one screen; the rest of the settings is a click away.
    #[test]
    fn settings_are_on_their_own_page() {
        let dir = temp_dir("pages");
        let mut harness = harness(&dir);
        harness.step();
        harness.get_by_label("Beklemeyi başlat");
        assert!(harness.query_by_label("İmleç hızı").is_none());

        harness.get_by_label("Ayarlar").click();
        harness.run_steps(2);
        harness.get_by_label("İmleç hızı");
        assert!(harness.query_by_label("Diğer bilgisayar").is_none());

        harness.get_by_label("Bağlantı").click();
        harness.run_steps(2);
        harness.get_by_label("Diğer bilgisayar");
        assert!(harness.query_by_label("İmleç hızı").is_none());
    }

    /// Hovering a button changes its colour only: its size and place stay put.
    #[test]
    fn hover_does_not_move_widgets() {
        let dir = temp_dir("hover");
        let mut harness = harness(&dir);
        harness.step();
        for label in ["Beklemeyi başlat", "Ayarlar", "Bağlan"] {
            let before = harness.get_by_label(label).rect();
            harness.get_by_label(label).hover();
            harness.run_steps(8);
            assert_eq!(harness.get_by_label(label).rect(), before, "{label} moved on hover");
        }
    }

    #[test]
    fn connect_needs_an_address() {
        let dir = temp_dir("no-address");
        let mut harness = harness(&dir);
        harness.state_mut().form.mode = Mode::Connect;
        harness.state_mut().form.address.clear();
        harness.step();
        harness.get_by_label("Bağlantıyı başlat").click();
        harness.run_steps(2);
        assert!(harness.state().running.is_none());
        assert!(harness.state().error.is_some());
        harness.get_by_label("Bağlanılacak bilgisayarın adresini girin.");
    }

    #[test]
    fn connection_reflows_without_hiding_primary_action() {
        for (width, height, scale) in [(340.0, 360.0, 2.0), (800.0, 520.0, 1.5)] {
            let dir = temp_dir(&format!("layout-{width}"));
            let mut harness = harness_at(&dir, [width, height], scale);
            harness.run_steps(3);

            let connection = harness.get_by_label("Hızlı bağlantı").rect();
            let layout = harness.get_by_label("Ekran düzeni").rect();
            if width >= TWO_COLUMNS {
                assert!(connection.right() < layout.left());
                assert_eq!(connection.top(), layout.top());
            } else {
                assert_eq!(connection.left(), layout.left());
                assert!(connection.bottom() < layout.top());
            }
            let action = harness.get_by_label("Beklemeyi başlat").rect();
            assert!(action.right() <= width && action.bottom() <= height);
        }
    }

    #[test]
    fn selects_discovered_peer_with_keyboard() {
        let peer = "192.0.2.2:24800".parse().unwrap();
        let label = "Diğer bilgisayar · Windows · 192.0.2.2";
        let mut harness = Harness::new_ui_state(
            |ui, address| peer_choice(ui, address, peer, label),
            String::new(),
        );
        harness.key_press(egui::Key::Tab);
        harness.run_steps(2);
        assert!(harness.get_by_label(label).is_focused());
        harness.key_press(egui::Key::Enter);
        harness.run_steps(2);
        assert_eq!(harness.state(), &peer.to_string());
    }

    #[test]
    fn connection_diagram_does_not_schedule_periodic_redraws() {
        for link in [Link::Waiting, Link::Remote] {
            let mut harness = Harness::new_ui(move |ui| {
                theme::status_indicator(ui, link);
                theme::monitors(ui, false, link);
            });
            harness.run_steps(4);
            let root = &harness.output().viewport_output[&egui::ViewportId::ROOT];
            assert_eq!(root.repaint_delay, Duration::MAX);
        }
    }

    /// Pairs the window with a listening engine by typing the PIN, then stops
    /// and reconnects without a PIN.
    #[test]
    fn pairs_with_pin_and_reconnects() {
        let (gui_dir, peer_dir) = (temp_dir("window"), temp_dir("peer"));
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let peer_settings = config::resolve(
            FileConfig {
                name: Some("Peer".into()),
                side: Some("left".into()),
                listen: Some(format!("127.0.0.1:{port}")),
                announce: Some(false),
                data_dir: Some(peer_dir.clone()),
                ..FileConfig::default()
            },
            RunFlags { dry_run: true, ..RunFlags::default() },
        )
        .unwrap();
        let (peer_tx, peer_rx) = mpsc::channel();
        let peer = Engine::start(
            peer_settings,
            PinSource::Console,
            Arc::new(move |r| drop(peer_tx.send(r))),
        );

        let mut harness = harness_at(&gui_dir, [340.0, 360.0], 2.0);
        let form = &mut harness.state_mut().form;
        form.mode = Mode::Connect;
        form.address = format!("127.0.0.1:{port}");
        form.name = "Window".into();
        harness.step();
        harness.get_by_label("Bağlantıyı başlat").click();
        step_until(&mut harness, "the PIN question", |app| app.view.ask_pin.is_some());

        let pin = peer_rx
            .iter()
            .find_map(|r| match r {
                Report::ShowPin(pin) => Some(pin),
                _ => None,
            })
            .unwrap();
        let field = harness.get_by_label("Diğer bilgisayarda görünen 6 haneli PIN:");
        assert!(field.is_focused());
        field.type_text(&pin);
        harness.step();
        harness.key_press(egui::Key::Enter);
        step_until(&mut harness, "the connection", |app| connected_to(app).is_some());
        assert_eq!(connected_to(harness.state()), Some("Peer"));
        assert_eq!(harness.state().paired.len(), 1);
        harness.get_by_label("Bağlı: Peer");

        harness.get_by_label("Durdur").click();
        harness.run_steps(2);
        assert!(harness.state().running.is_none());
        harness.get_by_label("Durduruldu");

        // Paired now: the second start connects without asking.
        harness.set_size(egui::vec2(800.0, 520.0));
        harness.run_steps(2);
        harness.get_by_label("Bağlantıyı başlat").click();
        step_until(&mut harness, "the reconnection", |app| connected_to(app).is_some());
        assert!(harness.state().view.ask_pin.is_none());
        let saved = config::load(&gui_dir.join(SETTINGS_FILE)).unwrap();
        assert_eq!(saved.connect, Some(format!("127.0.0.1:{port}")));

        // Paired now: an empty address is allowed, it means "find it on the network".
        harness.get_by_label("Durdur").click();
        harness.run_steps(2);
        harness.state_mut().form.address.clear();
        harness.step();
        harness.get_by_label("Bağlantıyı başlat").click();
        harness.run_steps(2);
        assert!(harness.state().running.is_some());
        assert!(harness.state().error.is_none());

        drop(harness);
        peer.stop();
        let _ = std::fs::remove_dir_all(gui_dir);
        let _ = std::fs::remove_dir_all(peer_dir);
    }
}
