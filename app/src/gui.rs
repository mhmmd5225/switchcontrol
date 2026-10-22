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

use crate::config::{self, FileConfig, RunFlags};
use crate::engine::{Engine, LinkState, Report};
use crate::identity::{self, TrustStore};
use crate::net::{self, PinSource, Waiting};
use crate::pairing::normalize_pin;

const SETTINGS_FILE: &str = "settings.toml";
const LOG_LINES: usize = 500;

const GREEN: Color32 = Color32::from_rgb(0x2e, 0xa0, 0x43);
const AMBER: Color32 = Color32::from_rgb(0xd0, 0x8c, 0x00);
const BLUE: Color32 = Color32::from_rgb(0x2f, 0x7d, 0xe1);
const RED: Color32 = Color32::from_rgb(0xd0, 0x3a, 0x3a);

/// The emergency hotkey as this OS spells it.
const HOTKEY: &str =
    if cfg!(target_os = "macos") { "Ctrl+Option+Shift+Esc" } else { "Ctrl+Alt+Shift+Esc" };

pub fn run() -> Result<(), String> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("SwitchControl")
            .with_inner_size([440.0, 680.0])
            .with_min_inner_size([380.0, 480.0]),
        ..Default::default()
    };
    let data_dir = identity::default_dir();
    // Other systems have no capture backend; the window still works for trying it out.
    let dry_run = cfg!(not(any(windows, target_os = "macos")));
    eframe::run_native(
        "SwitchControl",
        options,
        Box::new(move |cc| Ok(Box::new(GuiApp::new(cc.egui_ctx.clone(), data_dir, dry_run)))),
    )
    .map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Listen,
    Connect,
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
    running: Option<Running>,
    generation: u64,
    reports_tx: Sender<(u64, Report)>,
    reports_rx: Receiver<(u64, Report)>,
    view: View,
    error: Option<String>,
    perms: Permissions,
    perms_checked: Instant,
    paired: usize,
    local_ip: Option<IpAddr>,
}

impl GuiApp {
    pub fn new(ctx: egui::Context, data_dir: PathBuf, dry_run: bool) -> GuiApp {
        let file = config::load(&data_dir.join(SETTINGS_FILE)).unwrap_or_default();
        let (reports_tx, reports_rx) = mpsc::channel();
        GuiApp {
            ctx,
            form: Form::from_file(file),
            paired: paired_count(&data_dir),
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
        }
    }

    fn start(&mut self) {
        self.error = None;
        if self.form.mode == Mode::Connect && self.form.address.trim().is_empty() {
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
        let (tx, ctx) = (self.reports_tx.clone(), self.ctx.clone());
        let report: crate::engine::Reporter = Arc::new(move |r| {
            let _ = tx.send((generation, r));
            ctx.request_repaint();
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
        self.running = Some(Running {
            generation,
            engine: Some(Engine::start(settings, pin, report)),
            pin_tx: Some(pin_tx),
        });
    }

    fn stop(&mut self) {
        // Dropping stops the engine and waits for it.
        self.running = None;
        self.view.link = None;
        self.view.remote = false;
        self.view.show_pin = None;
        self.view.ask_pin = None;
        self.paired = paired_count(&self.data_dir);
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
                        self.paired = paired_count(&self.data_dir);
                    } else {
                        self.view.remote = false;
                    }
                    self.view.link = Some(state);
                }
                Report::Remote(on) => self.view.remote = on,
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

    fn status_ui(&self, ui: &mut egui::Ui) {
        let (color, text) = match (&self.running, &self.view.link) {
            (None, _) => (Color32::GRAY, "Durduruldu".to_string()),
            (Some(_), None) => (AMBER, "Başlatılıyor…".to_string()),
            (Some(_), Some(LinkState::Waiting(Waiting::Listening(addr)))) => {
                (AMBER, format!("Bağlantı bekleniyor (port {})", port_of(addr)))
            }
            (Some(_), Some(LinkState::Waiting(Waiting::Connecting(addr)))) => {
                (AMBER, format!("{addr} adresine bağlanılıyor…"))
            }
            (Some(_), Some(LinkState::Connected { peer, .. })) => {
                if self.view.remote {
                    (BLUE, format!("Klavye ve fare şu an {peer} bilgisayarında"))
                } else {
                    (GREEN, format!("Bağlı: {peer}"))
                }
            }
        };
        ui.horizontal(|ui| {
            // Painted rather than a glyph: the built-in fonts have no circle.
            let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
            ui.painter().circle_filled(rect.center(), 6.0, color);
            ui.label(RichText::new(text).strong().size(15.0));
        });
        if self.dry_run && self.running.is_some() {
            ui.label(RichText::new("Deneme modu: girdi yakalanmıyor").small().weak());
        }
        if let Some(error) = &self.error {
            ui.add_space(4.0);
            ui.label(RichText::new(error).color(RED));
        }
    }

    fn permissions_ui(&mut self, ui: &mut egui::Ui) {
        if self.perms.all_granted() {
            return;
        }
        card(ui, AMBER, |ui| {
            ui.label(RichText::new("macOS izinleri gerekli").strong());
            ui.label(
                "Klavye ve fareyi paylaşmak için Sistem Ayarları > Gizlilik ve Güvenlik \
                 altında SwitchControl'e şu izinleri verin:",
            );
            let rows = [
                ("Girdi İzleme", self.perms.listen, "Privacy_ListenEvent"),
                ("Erişilebilirlik", self.perms.post, "Privacy_Accessibility"),
            ];
            for (name, granted, pane) in rows {
                ui.horizontal(|ui| {
                    ui.label(name);
                    if granted {
                        ui.label(RichText::new("verildi").color(GREEN));
                    } else {
                        ui.label(RichText::new("verilmedi").color(RED));
                    }
                    if !granted && ui.button("Ayarları aç").clicked() {
                        open_privacy_pane(pane);
                    }
                });
            }
            ui.horizontal(|ui| {
                if ui.button("İzin iste").clicked() {
                    sys::init();
                }
                ui.label(RichText::new("İzin verdikten sonra uygulamayı yeniden açın.").small());
            });
        });
        ui.add_space(8.0);
    }

    fn pairing_ui(&mut self, ui: &mut egui::Ui) {
        if let Some(pin) = &self.view.show_pin {
            card(ui, BLUE, |ui| {
                ui.label(RichText::new("Eşleştirme isteği").strong());
                ui.label(
                    RichText::new(format!("{} {}", &pin[..3], &pin[3..]))
                        .monospace()
                        .size(34.0)
                        .strong(),
                );
                ui.label("Bu PIN'i diğer bilgisayara girin.");
            });
            ui.add_space(8.0);
        }
        if let Some(addr) = self.view.ask_pin {
            let mut answer = None;
            card(ui, BLUE, |ui| {
                ui.label(RichText::new(format!("{} ile eşleştirme", addr.ip())).strong());
                let label = ui.label("Diğer bilgisayarda görünen 6 haneli PIN:");
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut self.view.pin_input)
                        .hint_text("123 456")
                        .font(egui::TextStyle::Heading)
                        .desired_width(140.0),
                );
                let edit = edit.labelled_by(label.id);
                if !edit.has_focus() && self.view.pin_input.is_empty() {
                    edit.request_focus();
                }
                let pin = normalize_pin(&self.view.pin_input);
                let entered = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                ui.horizontal(|ui| {
                    let pair = ui.add_enabled(pin.is_some(), egui::Button::new("Eşleştir"));
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
            ui.add_space(8.0);
        }
    }

    fn settings_ui(&mut self, ui: &mut egui::Ui) {
        let enabled = self.running.is_none();
        let form = &mut self.form;
        ui.add_enabled_ui(enabled, |ui| {
            egui::Grid::new("settings").num_columns(2).spacing([12.0, 10.0]).show(ui, |ui| {
                ui.label("Diğer bilgisayar");
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut form.side, Side::Left, "Solda");
                    ui.selectable_value(&mut form.side, Side::Right, "Sağda");
                });
                ui.end_row();

                ui.label("Bağlantı");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut form.mode, Mode::Listen, "Bağlantı bekle");
                    ui.radio_value(&mut form.mode, Mode::Connect, "Bağlan");
                });
                ui.end_row();

                match form.mode {
                    Mode::Connect => {
                        let label = ui.label("Adres");
                        ui.add(
                            egui::TextEdit::singleline(&mut form.address)
                                .hint_text("192.168.1.20")
                                .desired_width(180.0),
                        )
                        .labelled_by(label.id);
                    }
                    Mode::Listen => {
                        ui.label("Bu bilgisayar");
                        let ip = match self.local_ip {
                            Some(ip) => ip.to_string(),
                            None => "adres bulunamadı".into(),
                        };
                        ui.vertical(|ui| {
                            ui.label(RichText::new(ip).monospace().strong());
                            ui.label(RichText::new("Diğer bilgisayarda bu adresi girin.").small());
                        });
                    }
                }
                ui.end_row();

                let label = ui.label("Bu bilgisayarın adı");
                ui.add(egui::TextEdit::singleline(&mut form.name).desired_width(180.0))
                    .labelled_by(label.id);
                ui.end_row();

                let label = ui.label("İmleç hızı");
                ui.add(
                    egui::Slider::new(&mut form.speed, 0.25..=4.0)
                        .logarithmic(true)
                        .max_decimals(2)
                        .suffix("×"),
                )
                .labelled_by(label.id);
                ui.end_row();

                ui.label("Tuşlar");
                ui.checkbox(&mut form.swap, "Ctrl ile Cmd yer değiştirsin");
                ui.end_row();
            });
        });
    }

    fn controls_ui(&mut self, ui: &mut egui::Ui) {
        let running = self.running.is_some();
        let (text, fill) = if running { ("Durdur", RED) } else { ("Başlat", GREEN) };
        let button =
            egui::Button::new(RichText::new(text).size(16.0).color(Color32::WHITE)).fill(fill);
        if ui.add_sized([ui.available_width(), 36.0], button).clicked() {
            if running { self.stop() } else { self.start() }
        }

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(format!("Eşleşmiş bilgisayar: {}", self.paired));
            let can_forget = !running && self.paired > 0;
            if ui.add_enabled(can_forget, egui::Button::new("Eşleşmeleri unut").small()).clicked()
            {
                match TrustStore::load(&self.data_dir).and_then(|mut t| t.clear()) {
                    Ok(()) => self.push_log("forgot all paired computers".into()),
                    Err(e) => self.error = Some(format!("Eşleşmeler silinemedi: {e}")),
                }
                self.paired = paired_count(&self.data_dir);
            }
        });
        ui.label(
            RichText::new(format!(
                "Acil durum: {HOTKEY} klavye ve fareyi bu bilgisayara geri getirir."
            ))
            .small()
            .weak(),
        );
    }

    fn log_ui(&self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("Günlük").default_open(false).show(ui, |ui| {
            egui::ScrollArea::vertical().max_height(180.0).stick_to_bottom(true).show(ui, |ui| {
                for line in &self.view.log {
                    ui.label(RichText::new(line).monospace().small());
                }
            });
        });
    }
}

impl eframe::App for GuiApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll();
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading(RichText::new("SwitchControl").strong());
                ui.add_space(4.0);
                self.status_ui(ui);
                ui.separator();
                self.permissions_ui(ui);
                self.pairing_ui(ui);
                self.settings_ui(ui);
                ui.add_space(10.0);
                self.controls_ui(ui);
                ui.add_space(6.0);
                self.log_ui(ui);
            });
        });
        // Picks up permission changes made in System Settings. Otherwise the
        // window only redraws on input or when the engine reports something.
        if !self.perms.all_granted() {
            ui.ctx().request_repaint_after(Duration::from_secs(1));
        }
    }

    /// Quitting (e.g. Cmd+Q) must give keyboard and mouse back first.
    fn on_exit(&mut self) {
        self.running = None;
    }
}

fn card(ui: &mut egui::Ui, accent: Color32, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::group(ui.style()).stroke(egui::Stroke::new(1.5, accent)).inner_margin(10.0).show(
        ui,
        |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        },
    );
}

fn paired_count(dir: &Path) -> usize {
    TrustStore::load(dir).map(|t| t.len()).unwrap_or(0)
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
        assert_eq!(harness.state().paired, 1);
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

        drop(harness);
        peer.stop();
        let _ = std::fs::remove_dir_all(gui_dir);
        let _ = std::fs::remove_dir_all(peer_dir);
    }
}
