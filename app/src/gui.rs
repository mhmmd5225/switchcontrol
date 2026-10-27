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
use sc_proto::{Os, Power};

use crate::autostart;
use crate::config::{self, FileConfig, RunFlags};
use crate::discovery::Listener;
use crate::edgebar::{EdgeBar, Strip};
use crate::engine::{Engine, LinkState, Report};
use crate::identity::{self, Fingerprint, TrustStore};
use crate::net::{self, PinSource, Waiting};
use crate::pairing::normalize_pin;
use crate::power;
use crate::theme::{self, Kind, Link, Palette};
use crate::tray::{PowerMenu, Tray, TrayCommand};
use crate::wake;

const SETTINGS_FILE: &str = "settings.toml";
const LOG_LINES: usize = 500;
/// The window does not resize: both pages are laid out to fit it.
const WINDOW_SIZE: (f32, f32) = (460.0, 920.0);
/// How long "packet sent" stays under the status after waking the other computer.
const WAKE_NOTE: Duration = Duration::from_secs(6);
/// Waking at start is repeated this often, a few times, until connected:
/// right after login the network may not be up yet.
const WAKE_RETRY: Duration = Duration::from_secs(10);
const WAKE_TRIES: u32 = 4;

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
    scroll_speed: f64,
    swap: bool,
    edge_delay_ms: u64,
    edge_bar: bool,
    clipboard: bool,
    announce: bool,
    /// The other computer's network card, learned on connecting or typed in.
    wake_mac: String,
    /// Wake the other computer whenever sharing starts.
    wake_on_start: bool,
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
            scroll_speed: file.scroll_speed.unwrap_or(1.0),
            swap: file.swap_ctrl_cmd.unwrap_or(true),
            edge_delay_ms: file.edge_delay_ms.unwrap_or(0),
            edge_bar: file.edge_bar.unwrap_or(true),
            clipboard: file.clipboard.unwrap_or(true),
            announce: file.announce.unwrap_or(true),
            wake_mac: file.wake_mac.unwrap_or_default(),
            wake_on_start: file.wake_on_start.unwrap_or(false),
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
            scroll_speed: Some(self.scroll_speed),
            swap_ctrl_cmd: Some(self.swap),
            edge_delay_ms: Some(self.edge_delay_ms),
            edge_bar: Some(self.edge_bar),
            clipboard: Some(self.clipboard),
            announce: Some(self.announce),
            wake_mac: Some(self.wake_mac.trim().to_string()).filter(|m| !m.is_empty()),
            wake_on_start: Some(self.wake_on_start),
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
    /// Asking whether to really shut the other computer down.
    confirm_shutdown: bool,
    /// When the last Wake-on-LAN packet went out.
    woken_at: Option<Instant>,
    /// Waking at start: when to send again, and how many more times.
    wake_retry: Option<(Instant, u32)>,
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
            confirm_shutdown: false,
            woken_at: None,
            wake_retry: None,
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
                TrayCommand::Sleep => self.send_power(Power::Sleep),
                // Shutting down is asked about in the window.
                TrayCommand::Shutdown => {
                    self.page = Page::Connection;
                    self.confirm_shutdown = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                TrayCommand::Wake => self.wake(),
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
        let os = self.peer_os();
        let menu = PowerMenu {
            sleep: os.is_some_and(|os| power::supported(os, Power::Sleep)),
            shutdown: os.is_some_and(|os| power::supported(os, Power::Shutdown)),
            wake: os.is_none() && self.wake_mac().is_some(),
        };
        if let Some(tray) = &mut self.tray {
            tray.update(&status, running, menu);
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
        self.save_settings();
        let file = self.form.to_file();
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
        if self.form.wake_on_start && self.wake_mac().is_some() {
            self.wake_retry = Some((Instant::now(), WAKE_TRIES));
            self.wake_at_start();
        }
    }

    /// Wakes the other computer while sharing has started but is not
    /// connected yet, a few times over.
    fn wake_at_start(&mut self) {
        let Some((at, left)) = self.wake_retry else { return };
        if self.running.is_none() || self.peer_os().is_some() || left == 0 {
            self.wake_retry = None;
            return;
        }
        if Instant::now() < at {
            return;
        }
        let Some(mac) = self.wake_mac() else { return };
        match wake::send(mac) {
            Ok(()) => {
                self.push_log(format!("Wake-on-LAN packet sent to {}", wake::format_mac(mac)))
            }
            // Likely no network yet; the next try may find one.
            Err(e) => self.push_log(format!("could not send the Wake-on-LAN packet: {e}")),
        }
        self.wake_retry = Some((Instant::now() + WAKE_RETRY, left - 1));
        self.ctx.request_repaint_after(WAKE_RETRY);
    }

    fn stop(&mut self) {
        // Dropping stops the engine and waits for it.
        self.running = None;
        self.edge_bar.set(&self.ctx, None);
        self.view.link = None;
        self.view.remote = false;
        self.view.show_pin = None;
        self.view.ask_pin = None;
        self.confirm_shutdown = false;
        self.wake_retry = None;
        self.paired = paired_list(&self.data_dir);
    }

    /// The other computer's system, while connected.
    fn peer_os(&self) -> Option<Os> {
        match &self.view.link {
            Some(LinkState::Connected { os, .. }) => Some(*os),
            _ => None,
        }
    }

    fn wake_mac(&self) -> Option<wake::Mac> {
        wake::parse_mac(&self.form.wake_mac)
    }

    fn send_power(&mut self, power: Power) {
        self.confirm_shutdown = false;
        match self.running.as_ref().and_then(|r| r.engine.as_ref()) {
            Some(engine) if self.peer_os().is_some() => engine.send_power(power),
            _ => self.push_log("not connected to the other computer".into()),
        }
    }

    /// Sends the Wake-on-LAN packet; works whether sharing runs or not.
    fn wake(&mut self) {
        let Some(mac) = self.wake_mac() else { return };
        match wake::send(mac) {
            Ok(()) => {
                self.push_log(format!("Wake-on-LAN packet sent to {}", wake::format_mac(mac)));
                self.woken_at = Some(Instant::now());
                self.ctx.request_repaint_after(WAKE_NOTE);
            }
            Err(e) => self.error = Some(format!("Uyandırma paketi gönderilemedi: {e}")),
        }
    }

    /// Keeps the address the other computer told us, for waking it later.
    fn learn_mac(&mut self, mac: wake::Mac) {
        if self.wake_mac() == Some(mac) {
            return;
        }
        self.form.wake_mac = wake::format_mac(mac);
        self.save_settings();
    }

    fn save_settings(&mut self) {
        if let Err(e) = config::save(&self.data_dir.join(SETTINGS_FILE), &self.form.to_file()) {
            self.push_log(format!("could not save settings: {e}"));
        }
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
                    if let LinkState::Connected { mac, .. } = &state {
                        self.paired = paired_list(&self.data_dir);
                        if let Some(mac) = *mac {
                            self.learn_mac(mac);
                        }
                    } else {
                        self.view.remote = false;
                        self.confirm_shutdown = false;
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
        let (_, _, link) = self.status();
        let dry_run = self.dry_run && self.running.is_some();
        ui.horizontal(|ui| {
            ui.label(theme::semibold("SwitchControl").size(20.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                theme::status_pill(ui, link);
                if dry_run {
                    ui.label(RichText::new("Deneme modu").small().weak());
                }
            });
        });
        ui.add_space(2.0);
        theme::tabs(
            ui,
            &mut self.page,
            &[(Page::Connection, "Bağlantı"), (Page::Settings, "Ayarlar")],
        );
    }

    fn status_ui(&mut self, ui: &mut egui::Ui) {
        let (title, detail, link) = self.status();
        let peer = match &self.view.link {
            Some(LinkState::Connected { peer, .. }) => peer.clone(),
            _ => "—".into(),
        };
        theme::card(ui, |ui| {
            let p = Palette::of(ui);
            theme::monitors(ui, self.form.side == Side::Left, link, self.form.name.trim(), &peer);
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
            self.power_ui(ui, &peer);
        });
        self.confirm_shutdown_ui(ui);
    }

    /// Sleep and shut down while connected, wake while not.
    fn power_ui(&mut self, ui: &mut egui::Ui, peer: &str) {
        let p = Palette::of(ui);
        let mut pick = None;
        if let Some(os) = self.peer_os() {
            let actions =
                [(Power::Sleep, "Uyut", Kind::Secondary), (Power::Shutdown, "Kapat", Kind::Danger)];
            let actions: Vec<_> =
                actions.into_iter().filter(|(power, ..)| power::supported(os, *power)).collect();
            if actions.is_empty() {
                return;
            }
            ui.add_space(6.0);
            centered(ui, |ui| {
                ui.label(RichText::new(format!("{peer}:")).color(p.muted));
                for (power, label, kind) in actions {
                    if theme::button(ui, label, kind).clicked() {
                        pick = Some(power);
                    }
                }
            });
        } else if self.wake_mac().is_some() {
            ui.add_space(6.0);
            centered(ui, |ui| {
                if theme::button(ui, "Diğer bilgisayarı uyandır", Kind::Secondary).clicked() {
                    self.wake();
                }
            });
            if self.woken_at.is_some_and(|at| at.elapsed() < WAKE_NOTE) {
                ui.vertical_centered(|ui| {
                    ui.label(
                        RichText::new(
                            "Uyandırma paketi gönderildi; açılması birkaç saniye sürebilir.",
                        )
                        .small()
                        .color(p.muted),
                    );
                });
            }
        }
        match pick {
            Some(Power::Shutdown) => self.confirm_shutdown = true,
            Some(power) => self.send_power(power),
            None => {}
        }
    }

    fn confirm_shutdown_ui(&mut self, ui: &mut egui::Ui) {
        let Some(os) = self.peer_os().filter(|_| self.confirm_shutdown) else { return };
        let peer = match &self.view.link {
            Some(LinkState::Connected { peer, .. }) => peer.clone(),
            _ => return,
        };
        let p = Palette::of(ui);
        ui.add_space(8.0);
        let mut answer = None;
        theme::notice(ui, p.red, |ui| {
            ui.label(theme::semibold(format!("{peer} kapatılsın mı?")));
            ui.label(RichText::new("Kaydedilmemiş işler kaybolabilir.").color(p.muted));
            if os == Os::MacOs {
                ui.label(
                    RichText::new(
                        "Kapalı bir Mac ağdan uyandırılamaz; sonra uyandırmak için Uyut'u seçin.",
                    )
                    .color(p.amber),
                );
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if theme::button(ui, "Evet, kapat", Kind::Danger).clicked() {
                    answer = Some(true);
                }
                if theme::button(ui, "Vazgeç", Kind::Ghost).clicked() {
                    answer = Some(false);
                }
            });
        });
        match answer {
            Some(true) => self.send_power(Power::Shutdown),
            Some(false) => self.confirm_shutdown = false,
            None => {}
        }
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
            ui.horizontal(|ui| {
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
                let pin = normalize_pin(&self.view.pin_input);
                ui.horizontal(|ui| {
                    let edit = theme::text_field(
                        ui,
                        &mut self.view.pin_input,
                        "123 456",
                        150.0,
                        egui::TextStyle::Heading,
                    )
                    .labelled_by(label.id);
                    if !edit.has_focus() && self.view.pin_input.is_empty() {
                        edit.request_focus();
                    }
                    let entered =
                        edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    let pair = ui
                        .add_enabled_ui(pin.is_some(), |ui| {
                            theme::button(ui, "Eşleştir", Kind::Primary)
                        })
                        .inner;
                    if pin.is_some() && (pair.clicked() || entered) {
                        answer = Some(pin.clone());
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

    /// Who connects to whom: everything needed to press Başlat.
    fn connection_ui(&mut self, ui: &mut egui::Ui) {
        // Settings are locked while sharing runs, row by row: this computer's
        // address stays copyable, which is when it is needed.
        let enabled = self.running.is_none();
        let local_ip = self.local_ip;
        let paired = &self.paired;
        let form = &mut self.form;
        ui.add_space(8.0);
        theme::card(ui, |ui| {
            ui.add_enabled_ui(enabled, |ui| {
                theme::row(ui, "Rol", |ui| {
                    theme::segmented(
                        ui,
                        &mut form.mode,
                        &[(Mode::Listen, "Bağlantı bekle"), (Mode::Connect, "Bağlan")],
                    )
                });
            });
            theme::divider(ui);
            match form.mode {
                Mode::Connect => {
                    ui.add_enabled_ui(enabled, |ui| {
                        let (label, edit) = theme::row(ui, "Adres", |ui| {
                            let hint = if paired.is_empty() {
                                "192.168.1.20"
                            } else {
                                "boş: kendisi bulur"
                            };
                            theme::text_field(
                                ui,
                                &mut form.address,
                                hint,
                                190.0,
                                egui::TextStyle::Body,
                            )
                        });
                        edit.labelled_by(label.id);
                        found_ui(ui, &mut form.address, paired);
                    });
                }
                Mode::Listen => {
                    theme::row(ui, "Bu bilgisayarın adresi", |ui| match local_ip {
                        Some(ip) => {
                            if theme::button(ui, "Kopyala", Kind::Ghost).clicked() {
                                ui.ctx().copy_text(ip.to_string());
                            }
                            ui.label(theme::semibold(ip.to_string()));
                        }
                        None => {
                            ui.label(RichText::new("bulunamadı").weak());
                        }
                    });
                    theme::divider(ui);
                    ui.add_enabled_ui(enabled, |ui| {
                        let announce = "Ağda görünür ol";
                        theme::row(ui, announce, |ui| {
                            theme::switch(ui, &mut form.announce, announce)
                        });
                    });
                }
            }
            theme::divider(ui);
            ui.add_enabled_ui(enabled, |ui| {
                theme::row(ui, "Diğer bilgisayar", |ui| {
                    theme::segmented(
                        ui,
                        &mut form.side,
                        &[(Side::Left, "Solda"), (Side::Right, "Sağda")],
                    )
                });
                theme::divider(ui);
                let (label, edit) = theme::row(ui, "Bu bilgisayarın adı", |ui| {
                    theme::text_field(ui, &mut form.name, "", 190.0, egui::TextStyle::Body)
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
                let (label, slider) = theme::row(ui, "Kaydırma hızı", |ui| {
                    ui.label(format!("{:.2}×", form.scroll_speed));
                    ui.add(
                        egui::Slider::new(&mut form.scroll_speed, 0.25..=4.0)
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
            self.wake_row_ui(ui);
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

    /// The other computer's network card, for waking it.
    fn wake_row_ui(&mut self, ui: &mut egui::Ui) {
        let p = Palette::of(ui);
        let valid = self.form.wake_mac.trim().is_empty() || self.wake_mac().is_some();
        let (label, edit) = theme::row(ui, "Uyandırma adresi (MAC)", |ui| {
            theme::text_field(
                ui,
                &mut self.form.wake_mac,
                "bağlanınca dolar",
                170.0,
                egui::TextStyle::Monospace,
            )
        });
        let edit = edit.labelled_by(label.id);
        if edit.lost_focus() && valid {
            self.save_settings();
        }
        let note = if valid {
            RichText::new(
                "Mac: Sistem Ayarları > Enerji > Ağ erişimi için uyandır. \
                 Windows: ağ kartında Wake on Magic Packet.",
            )
            .small()
            .weak()
        } else {
            RichText::new("Adres AA:BB:CC:DD:EE:FF biçiminde olmalı.").small().color(p.red)
        };
        ui.label(note);
        let label = "Başlayınca diğer bilgisayarı uyandır";
        let mut on = self.form.wake_on_start;
        let (_, switch) = theme::row(ui, label, |ui| theme::switch(ui, &mut on, label));
        if switch.changed() {
            self.form.wake_on_start = on;
            self.save_settings();
        }
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
        let (text, kind) =
            if running { ("Durdur", Kind::Danger) } else { ("Başlat", Kind::Primary) };
        if theme::big_button(ui, text, kind).clicked() {
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
        self.wake_at_start();
        self.tray_logic(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        theme::paint_backdrop(ui);
        egui::Panel::bottom("footer")
            .show_separator_line(false)
            .frame(
                egui::Frame::new()
                    .fill(egui::Color32::from_black_alpha(70))
                    .inner_margin(egui::Margin { left: 16, right: 16, top: 12, bottom: 10 }),
            )
            .show(ui, |ui| self.footer_ui(ui));
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(16, 14)))
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

/// Lays out a row of widgets centred in the available width.
fn centered(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    // Measured on the first pass, centred from then on.
    let id = ui.scope_id().with("centered-width");
    let width: f32 = ui.ctx().data(|d| d.get_temp(id)).unwrap_or(0.0);
    ui.horizontal(|ui| {
        ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
        let start = ui.cursor().min.x;
        add(ui);
        let used = ui.min_rect().right() - start;
        if (used - width).abs() > 0.5 {
            ui.ctx().data_mut(|d| d.insert_temp(id, used));
            ui.ctx().request_repaint();
        }
    });
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
            let pick = ui
                .add_enabled_ui(*address != addr, |ui| theme::button(ui, "Seç", Kind::Ghost))
                .inner;
            if pick.clicked() {
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

    /// Hovering a button changes its colour only: its size and place stay put.
    #[test]
    fn hover_does_not_move_widgets() {
        let dir = temp_dir("hover");
        let mut harness = harness(&dir);
        harness.step();
        for label in ["Başlat", "Ayarlar", "Bağlan"] {
            let before = harness.get_by_label(label).rect();
            harness.get_by_label(label).hover();
            harness.run_steps(8);
            assert_eq!(harness.get_by_label(label).rect(), before, "{label} moved on hover");
        }
    }

    fn connected(harness: &mut Harness<'_, GuiApp>, os: Os) {
        harness.state_mut().view.link = Some(LinkState::Connected {
            peer: "Öteki".into(),
            addr: "192.0.2.5:24800".parse().unwrap(),
            os,
            mac: Some([0x3C, 0x22, 0xFB, 0x0A, 0xB1, 0x07]),
        });
        harness.run_steps(3);
    }

    /// A Mac can be put to sleep or shut down; shutting down asks first and
    /// warns that a Mac that is off cannot be woken.
    #[test]
    fn power_buttons_follow_the_other_computer() {
        let dir = temp_dir("power");
        let mut harness = harness(&dir);
        connected(&mut harness, Os::MacOs);
        harness.get_by_label("Uyut");
        harness.get_by_label("Kapat").click();
        harness.run_steps(3);
        assert!(harness.state().confirm_shutdown);
        harness.get_by_label("Öteki kapatılsın mı?");
        harness.get_by_label_contains("Kapalı bir Mac ağdan uyandırılamaz");
        harness.get_by_label("Vazgeç").click();
        harness.run_steps(3);
        assert!(!harness.state().confirm_shutdown);

        // Windows: shutting down only.
        connected(&mut harness, Os::Windows);
        assert!(harness.query_by_label("Uyut").is_none());
        harness.get_by_label("Kapat").click();
        harness.run_steps(3);
        assert!(harness.query_by_label_contains("Kapalı bir Mac").is_none());
        harness.get_by_label("Evet, kapat");
    }

    /// The address the other computer sends is kept, and offers waking it
    /// once it is gone.
    #[test]
    fn learned_address_offers_waking() {
        let dir = temp_dir("wake");
        let mut harness = harness(&dir);
        harness.step();
        assert!(harness.query_by_label("Diğer bilgisayarı uyandır").is_none());

        harness.state_mut().learn_mac([0x3C, 0x22, 0xFB, 0x0A, 0xB1, 0x07]);
        harness.run_steps(3);
        harness.get_by_label("Diğer bilgisayarı uyandır");
        let saved = config::load(&dir.join(SETTINGS_FILE)).unwrap();
        assert_eq!(saved.wake_mac.as_deref(), Some("3C:22:FB:0A:B1:07"));

        connected(&mut harness, Os::Windows);
        assert!(harness.query_by_label("Diğer bilgisayarı uyandır").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    /// With the switch on, starting wakes the other computer, again a few
    /// times until it connects; stopping ends it.
    #[test]
    fn starting_wakes_the_other_computer() {
        let dir = temp_dir("wake-start");
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let mut harness = harness(&dir);
        let form = &mut harness.state_mut().form;
        form.mode = Mode::Connect;
        form.address = format!("127.0.0.1:{port}");
        form.wake_mac = "3C:22:FB:0A:B1:07".into();
        form.wake_on_start = true;
        harness.step();
        harness.get_by_label("Başlat").click();
        harness.run_steps(2);
        let woken = |app: &GuiApp| app.view.log.iter().any(|l| l.contains("Wake-on-LAN packet"));
        assert!(woken(harness.state()));
        assert_eq!(harness.state().wake_retry.map(|(_, left)| left), Some(WAKE_TRIES - 1));

        harness.get_by_label("Durdur").click();
        harness.run_steps(2);
        assert!(harness.state().wake_retry.is_none());
        let saved = config::load(&dir.join(SETTINGS_FILE)).unwrap();
        assert_eq!(saved.wake_on_start, Some(true));
        let _ = std::fs::remove_dir_all(dir);
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
