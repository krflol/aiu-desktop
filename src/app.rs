use crate::{
    process::{Message, Operation},
    protocol::Account,
    tray::{Command, Tray},
};
use anyhow::{Result, anyhow};
use eframe::egui::{self, Color32, RichText};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

pub fn run(fixture: Option<PathBuf>) -> Result<()> {
    let backend = std::env::current_exe()
        .ok()
        .and_then(|p| {
            p.parent()
                .map(|d| d.join(if cfg!(windows) { "aiu.exe" } else { "aiu" }))
        })
        .filter(|p| p.is_file());
    let initial = fixture
        .as_ref()
        .map(read_fixture)
        .transpose()?
        .unwrap_or_default();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("AIU — account usage")
            .with_inner_size([700.0, 700.0])
            .with_min_inner_size([450.0, 360.0]),
        ..Default::default()
    };
    eframe::run_native(
        "AIU",
        options,
        Box::new(move |cc| {
            Ok(Box::new(Panel::new(
                cc.egui_ctx.clone(),
                backend,
                fixture,
                initial,
            )))
        }),
    )
    .map_err(|e| anyhow!("desktop panel: {e}"))
}

fn read_fixture(path: &PathBuf) -> Result<Vec<Account>> {
    Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
}
enum Action {
    Status,
    Add(String, String),
    Login(String, String),
    Switch(String),
    Remove(String),
    Sync,
}
struct Panel {
    backend: Option<PathBuf>,
    fixture: Option<PathBuf>,
    accounts: Vec<Account>,
    pending: Option<Operation>,
    message: String,
    last_poll: Instant,
    provider: String,
    label: String,
    tray: Option<Tray>,
    tray_error: Option<String>,
    tray_initialized: bool,
    quitting: bool,
    selected: Option<String>,
}
impl Panel {
    fn new(
        ctx: egui::Context,
        backend: Option<PathBuf>,
        fixture: Option<PathBuf>,
        accounts: Vec<Account>,
    ) -> Self {
        let mut panel = Self {
            backend,
            fixture,
            accounts,
            pending: None,
            message: String::new(),
            last_poll: Instant::now(),
            provider: "claude".into(),
            label: String::new(),
            tray: None,
            tray_error: None,
            tray_initialized: false,
            quitting: false,
            selected: None,
        };
        if panel.fixture.is_some() {
            panel.message = "Sample data — backend actions are disabled".into();
        } else if panel.backend.is_some() {
            panel.start(Action::Status, ctx);
        } else {
            panel.message = "AIU backend executable is unavailable beside aiu-desktop".into();
        }
        panel
    }
    fn start(&mut self, action: Action, ctx: egui::Context) {
        if self.pending.is_some() || self.quitting {
            return;
        }
        if self.fixture.is_some() {
            self.message = "Sample data — backend actions are disabled".into();
            return;
        }
        let Some(backend) = self.backend.clone() else {
            return;
        };
        let (name, args) = match action {
            Action::Status => ("status", vec![]),
            Action::Sync => ("sync", vec![]),
            Action::Add(p, l) => ("add", vec!["--provider".into(), p, "--label".into(), l]),
            Action::Login(p, l) => ("login", vec!["--provider".into(), p, "--label".into(), l]),
            Action::Switch(k) => selector_args("switch", k),
            Action::Remove(k) => selector_args("remove", k),
        };
        self.message = "Working…".into();
        match Operation::start(backend, name, &args) {
            Ok(operation) => self.pending = Some(operation),
            Err(error) => {
                self.message = format!("Could not start AIU: {error}");
                return;
            }
        }
        ctx.request_repaint();
    }
    fn finish(&mut self, message: Message) {
        match message {
            Message::Progress(m) => self.message = m,
            Message::Result {
                ok,
                cancelled,
                message,
                accounts,
                error,
            } => {
                if ok && let Some(accounts) = accounts {
                    self.accounts = accounts;
                }
                self.message = if cancelled {
                    "Cancelled".into()
                } else if ok {
                    message
                } else {
                    error.unwrap_or_else(|| "AIU operation failed".into())
                };
                self.last_poll = Instant::now();
                self.pending = None;
            }
            Message::Failed(e) => {
                self.message = e;
                self.pending = None;
            }
        }
    }
    fn quit(&mut self) {
        self.quitting = true;
        if let Some(op) = &self.pending {
            op.cancel();
        }
        self.message = "Finishing current operation…".into();
    }
}
impl eframe::App for Panel {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // Once the child has settled, accept the native close event. Re-entering
        // close-to-tray handling here would cancel our own Quit indefinitely.
        if self.quitting && self.pending.is_none() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if !self.tray_initialized {
            self.tray_initialized = true;
            match Tray::new(ctx.clone(), frame) {
                Ok(tray) => self.tray = Some(tray),
                Err(_) => {
                    self.tray_error =
                        Some("System tray unavailable; closing the window will quit AIU".into())
                }
            }
        }
        let commands: Vec<_> = self.tray.iter().flat_map(|t| t.commands()).collect();
        for command in commands {
            match command {
                Command::Show => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                Command::Refresh => self.start(Action::Status, ctx.clone()),
                Command::Quit => self.quit(),
            }
        }
        if ctx.input(|i| i.viewport().close_requested()) {
            if self.tray.is_some() && !self.quitting {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            } else {
                self.quit();
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            }
        }
        #[cfg(feature = "screenshot")]
        if self.fixture.is_some() {
            if ctx.cumulative_frame_nr() == 10 {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            }
            let captured = ctx.input(|input| {
                input.events.iter().find_map(|event| match event {
                    egui::Event::Screenshot { image, .. } => Some(image.clone()),
                    _ => None,
                })
            });
            if let (Some(captured), Some(path)) = (captured, std::env::var_os("AIU_SCREENSHOT_TO"))
            {
                let pixels = captured
                    .pixels
                    .iter()
                    .flat_map(|pixel| pixel.to_array())
                    .collect();
                if let Some(bitmap) = image::RgbaImage::from_raw(
                    captured.size[0] as u32,
                    captured.size[1] as u32,
                    pixels,
                ) {
                    let _ = bitmap.save(path);
                }
                self.quitting = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            ctx.request_repaint_after(Duration::from_millis(20));
        }
        if let Some(op) = &self.pending
            && let Some(event) = op.try_event()
        {
            self.finish(event);
        }
        if self.quitting && self.pending.is_none() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if self.pending.is_none()
            && self.fixture.is_none()
            && self.last_poll.elapsed() >= Duration::from_secs(60)
        {
            self.start(Action::Status, ctx.clone());
        }
        let busy = self.pending.is_some();
        let editable = !busy && self.fixture.is_none();
        let mut action = None;
        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading(
                    RichText::new("AIU")
                        .size(28.0)
                        .color(Color32::from_rgb(133, 203, 193)),
                );
                ui.label("Your account usage");
                if ui
                    .add_enabled(!busy, egui::Button::new("Refresh"))
                    .clicked()
                {
                    action = Some(Action::Status);
                }
            });
            ui.horizontal(|ui| {
                ui.label(if busy { "Working…" } else { "Ready" });
            });
        });
        egui::TopBottomPanel::bottom("footer").show(ctx, |ui| {
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("provider")
                    .selected_text(&self.provider)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.provider, "claude".into(), "Claude");
                        ui.selectable_value(&mut self.provider, "codex".into(), "Codex");
                    });
                ui.add(
                    egui::TextEdit::singleline(&mut self.label)
                        .hint_text("Label (optional)")
                        .desired_width(150.0),
                );
                if ui.add_enabled(editable, egui::Button::new("Add")).clicked() {
                    action = Some(Action::Add(self.provider.clone(), self.label.clone()));
                }
                if ui
                    .add_enabled(editable, egui::Button::new("Sign in"))
                    .clicked()
                {
                    action = Some(Action::Login(self.provider.clone(), self.label.clone()));
                }
                if ui
                    .add_enabled(editable, egui::Button::new("Sync"))
                    .clicked()
                {
                    action = Some(Action::Sync);
                }
            });
            ui.label(&self.message);
            if busy && ui.button("Cancel").clicked() {
                if let Some(operation) = &self.pending {
                    operation.cancel();
                }
                self.message = "Cancelling…".into();
            }
            if let Some(error) = &self.tray_error {
                ui.colored_label(Color32::from_rgb(240, 184, 98), error);
            }
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                for account in &self.accounts {
                    let key = selector(account);
                    let selected = self.selected.as_deref() == Some(key.as_str());
                    egui::Frame::group(ui.style())
                        .inner_margin(12.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                if ui.selectable_label(selected, &account.label).clicked() {
                                    self.selected = Some(key.clone());
                                }
                                ui.weak(format!("{} · {}", account.provider, account.tier));
                                if account.active {
                                    ui.colored_label(Color32::from_rgb(133, 203, 193), "Active");
                                }
                                if account.can_switch {
                                    ui.label("Switchable");
                                }
                            });
                            if !account.email.is_empty() {
                                ui.small(&account.email);
                            }
                            if !account.org_name.is_empty() {
                                ui.small(&account.org_name);
                            }
                            if !account.login.state.is_empty() && account.login.state != "ok" {
                                ui.small(format!("Login: {}", account.login.state));
                                if !account.login.message.is_empty() {
                                    ui.small(&account.login.message);
                                }
                            }
                            if !account.stale.is_empty() {
                                ui.small(format!("Cached: {}", account.stale));
                            }
                            if account.recommended {
                                ui.colored_label(
                                    Color32::from_rgb(133, 203, 193),
                                    if account.all_spent {
                                        "Next reset"
                                    } else {
                                        "Recommended"
                                    },
                                );
                                if !account.why.is_empty() {
                                    ui.small(&account.why);
                                }
                            }
                            if !account.error.is_empty() {
                                ui.colored_label(Color32::from_rgb(241, 129, 129), &account.error);
                            }
                            for window in &account.windows {
                                ui.horizontal(|ui| {
                                    ui.label(if window.label.is_empty() {
                                        &window.key
                                    } else {
                                        &window.label
                                    });
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(if window.severity == "locked" {
                                                "Locked".to_string()
                                            } else if !window.known {
                                                "Unknown".to_string()
                                            } else {
                                                format!("{:.0}% used", window.percent)
                                            });
                                        },
                                    );
                                });
                                if window.known {
                                    ui.add(egui::ProgressBar::new(
                                        (window.percent / 100.0).clamp(0.0, 1.0) as f32,
                                    ));
                                }
                                if !window.resets_at.is_empty() {
                                    ui.small(format!("Resets {}", window.resets_at));
                                }
                            }
                            if selected {
                                ui.horizontal(|ui| {
                                    if ui
                                        .add_enabled(
                                            editable && account.can_switch,
                                            egui::Button::new("Switch"),
                                        )
                                        .clicked()
                                    {
                                        action = Some(Action::Switch(key.clone()));
                                    }
                                    if ui
                                        .add_enabled(editable, egui::Button::new("Remove"))
                                        .clicked()
                                    {
                                        action = Some(Action::Remove(key.clone()));
                                    }
                                });
                            }
                        });
                }
            });
        });
        if let Some(a) = action {
            self.start(a, ctx.clone());
        }
        ctx.request_repaint_after(Duration::from_secs(1));
    }
}
fn selector(a: &Account) -> String {
    format!("{}:{}#{}", a.provider, a.email, a.org)
}
fn selector_args(action: &'static str, key: String) -> (&'static str, Vec<String>) {
    let mut parts = key.splitn(2, ':');
    let provider = parts.next().unwrap_or_default().to_string();
    let selector = parts.next().unwrap_or_default().to_string();
    (action, vec!["--provider".into(), provider, selector])
}
