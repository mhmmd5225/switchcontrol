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

use eframe::egui::{self, Color32, RichText};
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
use crate::theme::{self, Link, Palette};
use crate::tray::{Tray, TrayCommand};

const SETTINGS_FILE: &str = "settings.toml";
const LOG_LINES: usize = 500;
/// The window does not resize: both pages are laid out to fit it.
const WINDOW_SIZE: (f32, f32) = (460.0, 760.0);

/// The emergency hotkey as this OS spells it.
const HOTKEY: &str =
    if cfg!(target_os = "macos") { "Ctrl+Option+Shift+Esc" } else { "Ctrl+Alt+Shift+Esc" };

/// Opens the window. `at_login`: started by [`autostart`], so hide in the
/// tray and start sharing right away.
pub fn run(at_login: bool) -> Result<(), String> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("SwitchControl")
            .with_inner_size([WINDOW_SIZE.0, WINDOW_SIZE.1])
            .with_resizable(false)
            .with_maximize_button(false)
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
    perms: Permissions,
    perms_checked: Instant,
    /// Fingerprints of the paired computers.
    paired: Vec<Fingerprint>,
    local_ip: Option<IpAddr>,
    edge_bar: EdgeBar,
    /// Where the edge bar goes, if it is on. Its window stays open even while
    /// sharing is off, so it can show up while the main window is hidden.
    strip: Option<Strip>,
    /// The settings `strip` was made for; `None` to make it again.
    strip_for: Option<(Side, bool)>,
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
            perms: sys::permissions(),
            perms_checked: Instant::now(),
            local_ip: local_ip(),
            edge_bar: EdgeBar::default(),
            strip: None,
            strip_for: None,
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
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
        let status = self.status().0;
        let running = self.running.is_some();
        if let Some(tray) = &mut self.tray {
            tray.update(&status, running);
        }
    }

    /// Keeps the edge bar's window where the settings put it.
    fn edge_bar_ui(&mut self, ctx: &egui::Context) {
        let wanted = (self.form.side, self.form.edge_bar);
        if self.strip_for != Some(wanted) {
            self.strip_for = Some(wanted);
            // Monitor coordinates are physical pixels on Windows and points elsewhere.
            let scale =
                if cfg!(windows) { ctx.native_pixels_per_point().unwrap_or(1.0) } else { 1.0 };
            let screens: Vec<_> = sys::monitors().into_iter().map(|m| m.bounds).collect();
            self.strip =
                if self.form.edge_bar { Strip::new(&screens, self.form.side, scale) } else { None };
        }
        if let Some(strip) = self.strip {
            self.edge_bar.show(ctx, strip);
        }
    }

    fn start(&mut self) {
        self.error = None;
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
                ctx.request_repaint();
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
                }
                Report::Stopped(result) => {
                    if let Err(e) = result {
                        self.push_log(format!("stopped: {e}"));
                        self.error = Some(explain(&e));
                    }
                    self.stop();
                }
            }
        }
        if self.perms_checked.elapsed() >= Duration::from_secs(1) {
            self.perms = sys::permissions();
            self.perms_checked = Instant::now();
        }
    }

    /// Headline, detail line and picture for the current state.
    fn status(&self) -> (String, String, Link) {
        match (&self.running, &self.view.link) {
            (None, _) => {
                ("Durduruldu".into(), "Başlat'a basarak bağlantıyı açın.".into(), Link::Off)
            }
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
        let dry_run = self.dry_run && self.running.is_some();
        let page = &mut self.page;
        ui.horizontal(|ui| {
            ui.label(theme::semibold("SwitchControl").size(20.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                theme::segmented(
                    ui,
                    page,
                    &[(Page::Connection, "Bağlantı"), (Page::Settings, "Ayarlar")],
                );
                if dry_run {
                    ui.label(RichText::new("Deneme modu").small().weak());
                }
            });
        });
        ui.add_space(4.0);
    }

    fn status_ui(&self, ui: &mut egui::Ui) {
        let (title, detail, link) = self.status();
        let peer = match &self.view.link {
            Some(LinkState::Connected { peer, .. }) => peer.as_str(),
            _ => "—",
        };
        theme::card(ui, |ui| {
            let p = Palette::of(ui);
            theme::monitors(ui, self.form.side == Side::Left, link, self.form.name.trim(), peer);
            ui.vertical_centered(|ui| {
                ui.label(theme::semibold(title).size(16.0));
                if !detail.is_empty() {
                    ui.label(RichText::new(detail).color(p.muted));
                }
                if let Some(error) = &self.error {
                    ui.add_space(2.0);
                    ui.label(RichText::new(error).color(p.red));
                }
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
                    } else if ui.button("Ayarları aç").clicked() {
                        open_privacy_pane(pane);
                    }
                });
            }
            ui.horizontal(|ui| {
                if ui.button("İzin iste").clicked() {
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
                let pin = normalize_pin(&self.view.pin_input);
                ui.horizontal(|ui| {
                    let edit = ui
                        .add(
                            egui::TextEdit::singleline(&mut self.view.pin_input)
                                .hint_text("123 456")
                                .font(egui::TextStyle::Heading)
                                .desired_width(130.0),
                        )
                        .labelled_by(label.id);
                    if !edit.has_focus() && self.view.pin_input.is_empty() {
                        edit.request_focus();
                    }
                    let entered =
                        edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    let pair = ui.add_enabled(
                        pin.is_some(),
                        egui::Button::new(theme::semibold("Eşleştir").color(Color32::WHITE))
                            .fill(p.accent),
                    );
                    if pin.is_some() && (pair.clicked() || entered) {
                        answer = Some(pin.clone());
                    }
                    if ui.button("İptal").clicked() {
                        answer = Some(None);
                    }
                });
            });
            if let Some(answer) = answer {
                self.answer_pin(answer);
            }
        }
    }

    /// Who connects to whom: everything needed to press Başlat.
    fn connection_ui(&mut self, ui: &mut egui::Ui) {
        let enabled = self.running.is_none();
        let local_ip = self.local_ip;
        let paired = &self.paired;
        let form = &mut self.form;
        ui.add_enabled_ui(enabled, |ui| {
            ui.add_space(8.0);
            theme::card(ui, |ui| {
                theme::row(ui, "Rol", |ui| {
                    theme::segmented(
                        ui,
                        &mut form.mode,
                        &[(Mode::Listen, "Bağlantı bekle"), (Mode::Connect, "Bağlan")],
                    )
                });
                theme::divider(ui);
                match form.mode {
                    Mode::Connect => {
                        let (label, edit) = theme::row(ui, "Adres", |ui| {
                            let hint = if paired.is_empty() {
                                "192.168.1.20"
                            } else {
                                "boş: kendisi bulur"
                            };
                            ui.add(
                                egui::TextEdit::singleline(&mut form.address)
                                    .hint_text(hint)
                                    .desired_width(180.0),
                            )
                        });
                        edit.labelled_by(label.id);
                        found_ui(ui, &mut form.address, paired);
                    }
                    Mode::Listen => {
                        theme::row(ui, "Bu bilgisayarın adresi", |ui| match local_ip {
                            Some(ip) => {
                                if ui.small_button("Kopyala").clicked() {
                                    ui.ctx().copy_text(ip.to_string());
                                }
                                ui.label(theme::semibold(ip.to_string()));
                            }
                            None => {
                                ui.label(RichText::new("bulunamadı").weak());
                            }
                        });
                        theme::divider(ui);
                        let announce = "Ağda görünür ol";
                        theme::row(ui, announce, |ui| {
                            theme::switch(ui, &mut form.announce, announce)
                        });
                    }
                }
                theme::divider(ui);
                theme::row(ui, "Diğer bilgisayar", |ui| {
                    theme::segmented(
                        ui,
                        &mut form.side,
                        &[(Side::Left, "Solda"), (Side::Right, "Sağda")],
                    )
                });
                theme::divider(ui);
                let (label, edit) = theme::row(ui, "Bu bilgisayarın adı", |ui| {
                    ui.add(egui::TextEdit::singleline(&mut form.name).desired_width(180.0))
                });
                edit.labelled_by(label.id);
            });
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
                forget =
                    ui.add_enabled(can_forget, egui::Button::new("Eşleşmeleri unut")).clicked();
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
        let p = Palette::of(ui);
        let running = self.running.is_some();
        // The light palette's red keeps white text readable in both themes.
        let (text, fill) =
            if running { ("Durdur", Palette::LIGHT.red) } else { ("Başlat", p.accent) };
        if theme::primary_button(ui, text, fill).clicked() {
            if running { self.stop() } else { self.start() }
        }
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new(format!(
                    "{HOTKEY} klavye ve fareyi her zaman bu bilgisayara getirir"
                ))
                .small()
                .weak(),
            );
        });
    }
}

impl eframe::App for GuiApp {
    /// Also runs while the window is hidden in the tray.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll();
        self.tray_logic(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let p = Palette::of(ui);
        egui::Panel::bottom("footer")
            .show_separator_line(false)
            .frame(
                egui::Frame::new()
                    .fill(p.background)
                    .stroke(egui::Stroke::new(1.0, p.border))
                    .inner_margin(egui::Margin { left: 16, right: 16, top: 12, bottom: 10 }),
            )
            .show(ui, |ui| self.footer_ui(ui));
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new().fill(p.background).inner_margin(egui::Margin::symmetric(16, 14)),
            )
            .show(ui, |ui| {
                self.header_ui(ui);
                // Needed whichever page is open: a PIN question must not hide.
                if self.page == Page::Connection {
                    self.status_ui(ui);
                }
                self.permissions_ui(ui);
                self.pairing_ui(ui);
                // Both pages fit; the bar shows only if a long list or the open log overflow.
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match self
                    .page
                {
                    Page::Connection => self.connection_ui(ui),
                    Page::Settings => {
                        self.behavior_ui(ui);
                        self.app_ui(ui);
                        self.log_ui(ui);
                    }
                });
            });
        self.edge_bar_ui(ui.ctx());
        if std::mem::take(&mut self.hide_on_first_frame) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
        // Picks up permission changes made in System Settings. Otherwise the
        // window only redraws on input or when the engine reports something.
        if !self.perms.all_granted() {
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
    let ctx = ui.ctx().clone();
    listener.set_waker(move || ctx.request_repaint());
    // Drops computers that went quiet even when nothing new arrives.
    ui.ctx().request_repaint_after(std::time::Duration::from_secs(2));

    let p = Palette::of(ui);
    theme::divider(ui);
    ui.label(RichText::new("Ağda bulunanlar").small().color(p.muted));
    let found = listener.peers();
    if found.is_empty() {
        ui.label(
            RichText::new("Aranıyor… Diğer bilgisayarda Bağlantı bekle ile Başlat'a basın.")
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
        let addr = computer.addr.to_string();
        theme::row(ui, &format!("{} ({os})", computer.name), |ui| {
            if ui.add_enabled(*address != addr, egui::Button::new("Seç").small()).clicked() {
                *address = addr.clone();
            }
            if paired.contains(&computer.fingerprint) {
                ui.label(RichText::new("eşleşmiş").small().color(p.green));
            }
            ui.label(RichText::new(computer.addr.ip().to_string()).small().weak());
        });
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
        let dir = dir.to_path_buf();
        Harness::builder()
            .with_size([440.0, 680.0])
            .build_eframe(move |cc| GuiApp::new(cc.egui_ctx.clone(), dir, true))
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
        harness.get_by_label("Başlat");
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

    #[test]
    fn connect_needs_an_address() {
        let dir = temp_dir("no-address");
        let mut harness = harness(&dir);
        harness.state_mut().form.mode = Mode::Connect;
        harness.state_mut().form.address.clear();
        harness.step();
        harness.get_by_label("Başlat").click();
        harness.run_steps(2);
        assert!(harness.state().running.is_none());
        assert!(harness.state().error.is_some());
        harness.get_by_label("Bağlanılacak bilgisayarın adresini girin.");
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

        let mut harness = harness(&gui_dir);
        let form = &mut harness.state_mut().form;
        form.mode = Mode::Connect;
        form.address = format!("127.0.0.1:{port}");
        form.name = "Window".into();
        harness.step();
        harness.get_by_label("Başlat").click();
        step_until(&mut harness, "the PIN question", |app| app.view.ask_pin.is_some());

        let pin = peer_rx
            .iter()
            .find_map(|r| match r {
                Report::ShowPin(pin) => Some(pin),
                _ => None,
            })
            .unwrap();
        harness.get_by_label("Diğer bilgisayarda görünen 6 haneli PIN:").type_text(&pin);
        harness.step();
        harness.get_by_label("Eşleştir").click();
        step_until(&mut harness, "the connection", |app| connected_to(app).is_some());
        assert_eq!(connected_to(harness.state()), Some("Peer"));
        assert_eq!(harness.state().paired.len(), 1);
        harness.get_by_label("Bağlı: Peer");

        harness.get_by_label("Durdur").click();
        harness.run_steps(2);
        assert!(harness.state().running.is_none());
        harness.get_by_label("Durduruldu");

        // Paired now: the second start connects without asking.
        harness.get_by_label("Başlat").click();
        step_until(&mut harness, "the reconnection", |app| connected_to(app).is_some());
        assert!(harness.state().view.ask_pin.is_none());
        let saved = config::load(&gui_dir.join(SETTINGS_FILE)).unwrap();
        assert_eq!(saved.connect, Some(format!("127.0.0.1:{port}")));

        // Paired now: an empty address is allowed, it means "find it on the network".
        harness.get_by_label("Durdur").click();
        harness.run_steps(2);
        harness.state_mut().form.address.clear();
        harness.step();
        harness.get_by_label("Başlat").click();
        harness.run_steps(2);
        assert!(harness.state().running.is_some());
        assert!(harness.state().error.is_none());

        drop(harness);
        peer.stop();
        let _ = std::fs::remove_dir_all(gui_dir);
        let _ = std::fs::remove_dir_all(peer_dir);
    }
}
