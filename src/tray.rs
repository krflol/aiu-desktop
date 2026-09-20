use crate::wake::{Heartbeat, Wake};
use anyhow::Result;
#[cfg(target_os = "linux")]
use anyhow::anyhow;
use eframe::{Frame, egui};
use std::sync::mpsc::{self, Receiver};
use tray_icon::{
    Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
};

#[derive(Clone, Copy)]
pub enum Command {
    Show,
    Refresh,
    Quit,
}
pub struct Tray {
    commands: Receiver<Command>,
    _wake: Wake,
    _heartbeat: Heartbeat,
    #[cfg(not(target_os = "linux"))]
    _icon: TrayIcon,
    #[cfg(target_os = "linux")]
    stop: mpsc::Sender<()>,
    #[cfg(target_os = "linux")]
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Tray {
    pub fn new(ctx: egui::Context, frame: &Frame) -> Result<Self> {
        let wake = Wake::new(ctx, frame)?;
        let heartbeat = Heartbeat::new(wake.clone());
        let (tx, commands) = mpsc::channel();
        let menu_tx = tx.clone();
        let menu_wake = wake.clone();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            let command = match event.id.0.as_str() {
                "aiu-show" => Command::Show,
                "aiu-refresh" => Command::Refresh,
                "aiu-quit" => Command::Quit,
                _ => return,
            };
            let _ = menu_tx.send(command);
            menu_wake.request_repaint();
        }));
        let click_tx = tx.clone();
        let click_wake = wake.clone();
        TrayIconEvent::set_event_handler(Some(move |event| {
            if !cfg!(target_os = "macos")
                && matches!(
                    event,
                    TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    }
                )
            {
                let _ = click_tx.send(Command::Show);
                click_wake.request_repaint();
            }
        }));
        #[cfg(not(target_os = "linux"))]
        {
            Ok(Self {
                commands,
                _wake: wake,
                _heartbeat: heartbeat,
                _icon: build_icon()?,
            })
        }
        #[cfg(target_os = "linux")]
        {
            let (stop, stopped) = mpsc::channel();
            let (ready, started) = mpsc::sync_channel(1);
            let thread = std::thread::spawn(move || {
                let result = (|| {
                    gtk::init().map_err(|e| anyhow!("GTK: {e}"))?;
                    ensure_host()?;
                    build_icon()
                })();
                match result {
                    Ok(icon) => {
                        if ready.send(Ok(())).is_err() {
                            return;
                        }
                        let _icon = icon;
                        loop {
                            while gtk::events_pending() {
                                gtk::main_iteration_do(false);
                            }
                            match stopped.recv_timeout(std::time::Duration::from_millis(100)) {
                                Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                                Err(mpsc::RecvTimeoutError::Timeout) => {}
                            }
                        }
                    }
                    Err(error) => {
                        let _ = ready.send(Err(error));
                    }
                }
            });
            started.recv_timeout(std::time::Duration::from_secs(5))??;
            Ok(Self {
                commands,
                _wake: wake,
                _heartbeat: heartbeat,
                stop,
                thread: Some(thread),
            })
        }
    }
    pub fn commands(&self) -> impl Iterator<Item = Command> + '_ {
        self.commands.try_iter()
    }
}
#[cfg(target_os = "linux")]
impl Drop for Tray {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
fn build_icon() -> Result<TrayIcon> {
    let menu = Menu::new();
    let show = MenuItem::with_id("aiu-show", "Show AIU", true, None);
    let refresh = MenuItem::with_id("aiu-refresh", "Refresh usage", true, None);
    let quit = MenuItem::with_id("aiu-quit", "Quit AIU", true, None);
    menu.append_items(&[&show, &refresh, &PredefinedMenuItem::separator(), &quit])?;
    Ok(TrayIconBuilder::new()
        .with_id(format!("aiu-tray-{}", std::process::id()))
        .with_tooltip("AIU account usage")
        .with_menu(Box::new(menu))
        .with_icon(icon()?)
        .with_icon_as_template(cfg!(target_os = "macos"))
        .with_menu_on_left_click(cfg!(target_os = "macos"))
        .build()?)
}
#[cfg(target_os = "linux")]
fn ensure_host() -> Result<()> {
    use gio::prelude::DBusProxyExt;
    use glib::prelude::ToVariant;
    use gtk::{gio, glib};
    let proxy = gio::DBusProxy::for_bus_sync(
        gio::BusType::Session,
        gio::DBusProxyFlags::DO_NOT_AUTO_START,
        None,
        "org.kde.StatusNotifierWatcher",
        "/StatusNotifierWatcher",
        "org.freedesktop.DBus.Properties",
        None::<&gio::Cancellable>,
    )
    .map_err(|e| anyhow!("StatusNotifier host unavailable: {e}"))?;
    let result = proxy
        .call_sync(
            "Get",
            Some(
                &(
                    "org.kde.StatusNotifierWatcher",
                    "IsStatusNotifierHostRegistered",
                )
                    .to_variant(),
            ),
            gio::DBusCallFlags::NONE,
            1000,
            None::<&gio::Cancellable>,
        )
        .map_err(|e| anyhow!("StatusNotifier host unavailable: {e}"))?;
    if result
        .child_value(0)
        .get::<glib::Variant>()
        .and_then(|v| v.get::<bool>())
        .unwrap_or(false)
    {
        Ok(())
    } else {
        Err(anyhow!("no StatusNotifier host is registered"))
    }
}
fn icon() -> Result<Icon> {
    let mut pixels = vec![0; 32 * 32 * 4];
    for (left, top, bottom) in [(4, 18, 28), (13, 10, 28), (22, 4, 28)] {
        for y in top..bottom {
            for x in left..left + 6 {
                pixels[(y * 32 + x) * 4..(y * 32 + x) * 4 + 4]
                    .copy_from_slice(&[133, 203, 193, 255]);
            }
        }
    }
    Ok(Icon::from_rgba(pixels, 32, 32)?)
}
