//! The icon in the Windows notification area / macOS menu bar.
//!
//! Its menu shows the status and offers start/stop, showing the window and
//! quitting. Closing the window only hides it while the icon is there. Menu
//! and click events arrive on other threads; they are queued and wake the
//! window, which handles them in [`crate::gui`].

/// What the user picked from the icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
pub enum TrayCommand {
    Show,
    StartStop,
    Quit,
}

#[cfg(any(windows, target_os = "macos"))]
mod imp {
    use std::sync::mpsc::{self, Receiver};

    use eframe::egui;
    use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{
        Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
    };

    use super::TrayCommand;

    pub struct Tray {
        icon: TrayIcon,
        status: MenuItem,
        start_stop: MenuItem,
        commands: Receiver<TrayCommand>,
        shown: Option<(String, bool)>,
    }

    impl Tray {
        /// Creates the icon. Call on the main thread once the window exists.
        pub fn new(ctx: &egui::Context) -> Result<Tray, String> {
            let status = MenuItem::new("SwitchControl", false, None);
            let start_stop = MenuItem::new("Başlat", true, None);
            let show = MenuItem::new("Pencereyi göster", true, None);
            let quit = MenuItem::new("Çıkış", true, None);
            let menu = Menu::new();
            menu.append_items(&[
                &status,
                &PredefinedMenuItem::separator(),
                &start_stop,
                &show,
                &PredefinedMenuItem::separator(),
                &quit,
            ])
            .map_err(|e| e.to_string())?;

            let png = eframe::icon_data::from_png_bytes(include_bytes!("../../assets/tray.png"))
                .map_err(|e| e.to_string())?;
            let image =
                Icon::from_rgba(png.rgba, png.width, png.height).map_err(|e| e.to_string())?;
            let icon = TrayIconBuilder::new()
                .with_icon(image)
                .with_tooltip("SwitchControl")
                .with_menu(Box::new(menu))
                // Windows: a left click shows the window, the menu is on the right button.
                .with_menu_on_left_click(cfg!(target_os = "macos"))
                .build()
                .map_err(|e| e.to_string())?;

            let (tx, commands) = mpsc::channel();
            let ids = (start_stop.id().clone(), show.id().clone(), quit.id().clone());
            let (menu_tx, menu_ctx) = (tx.clone(), ctx.clone());
            MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
                let command = if event.id == ids.0 {
                    TrayCommand::StartStop
                } else if event.id == ids.1 {
                    TrayCommand::Show
                } else if event.id == ids.2 {
                    TrayCommand::Quit
                } else {
                    return;
                };
                let _ = menu_tx.send(command);
                menu_ctx.request_repaint();
            }));
            let click_ctx = ctx.clone();
            TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
                let left_click = matches!(
                    event,
                    TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    }
                );
                let double_click = matches!(event, TrayIconEvent::DoubleClick { .. });
                // On macOS a click opens the menu instead.
                if double_click || (left_click && cfg!(windows)) {
                    let _ = tx.send(TrayCommand::Show);
                    click_ctx.request_repaint();
                }
            }));

            Ok(Tray { icon, status, start_stop, commands, shown: None })
        }

        pub fn commands(&self) -> Vec<TrayCommand> {
            self.commands.try_iter().collect()
        }

        /// Shows the status line and the right start/stop entry.
        pub fn update(&mut self, status: &str, running: bool) {
            let wanted = (status.to_string(), running);
            if self.shown.as_ref() == Some(&wanted) {
                return;
            }
            self.status.set_text(status);
            self.start_stop.set_text(if running { "Durdur" } else { "Başlat" });
            let _ = self.icon.set_tooltip(Some(format!("SwitchControl: {status}")));
            self.shown = Some(wanted);
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    use eframe::egui;

    use super::TrayCommand;

    /// No tray on other systems (it would need GTK).
    pub struct Tray;

    impl Tray {
        pub fn new(_ctx: &egui::Context) -> Result<Tray, String> {
            Err("no tray icon on this system".into())
        }

        pub fn commands(&self) -> Vec<TrayCommand> {
            Vec::new()
        }

        pub fn update(&mut self, _status: &str, _running: bool) {}
    }
}

pub use imp::Tray;
