use crate::{
    process::{Message, Operation},
    protocol::{Account, PendingResetRequest},
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
    Resets(String),
    Reset(String, Option<String>),
    AutoReset(String, Option<bool>, Option<u8>),
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
    reset_confirmation: Option<(String, Option<String>)>,
    reset_intents: std::collections::HashMap<String, PendingResetRequest>,
    active_reset_key: Option<String>,
    auto_threshold_drafts: std::collections::HashMap<String, u8>,
    active_auto_threshold: Option<(String, u8)>,
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
            reset_confirmation: None,
            reset_intents: std::collections::HashMap::new(),
            active_reset_key: None,
            auto_threshold_drafts: std::collections::HashMap::new(),
            active_auto_threshold: None,
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
        let active_reset_key = match &action {
            Action::Reset(key, _) => Some(key.clone()),
            _ => None,
        };
        let active_auto_threshold = match &action {
            Action::AutoReset(key, _, Some(threshold)) => Some((key.clone(), *threshold)),
            _ => None,
        };
        let (name, args) = match action {
            Action::Status => ("status", vec![]),
            Action::Sync => ("sync", vec![]),
            Action::Add(p, l) => ("add", vec!["--provider".into(), p, "--label".into(), l]),
            Action::Login(p, l) => ("login", vec!["--provider".into(), p, "--label".into(), l]),
            Action::Switch(k) => selector_args("switch", k),
            Action::Remove(k) => selector_args("remove", k),
            Action::Resets(k) => selector_args("resets", k),
            Action::Reset(k, credit_id) => {
                let from_backend = self
                    .accounts
                    .iter()
                    .find(|account| selector(account) == k)
                    .and_then(|account| account.banked_resets.as_ref())
                    .and_then(|state| state.pending_request.clone());
                let intent = get_or_create_reset_intent(
                    &mut self.reset_intents,
                    &k,
                    from_backend,
                    credit_id,
                );
                // A user choice is attached to the ID only when the intent is first created.
                let mut args = vec!["--provider".into(), "codex".into()];
                args.extend(k.split_once(':').map(|(_, selector)| selector.to_string()));
                args.extend([
                    "--yes".into(),
                    "--request-id".into(),
                    intent.request_id.clone(),
                ]);
                if let Some(id) = &intent.credit_id {
                    args.extend(["--credit-id".into(), id.clone()]);
                }
                ("reset", args)
            }
            Action::AutoReset(k, enabled, threshold) => {
                ("auto-reset", auto_reset_args(&k, enabled, threshold))
            }
        };
        self.message = "Working…".into();
        self.active_reset_key = active_reset_key;
        self.active_auto_threshold = active_auto_threshold;
        match Operation::start(backend, name, &args) {
            Ok(operation) => self.pending = Some(operation),
            Err(error) => {
                self.active_reset_key = None;
                self.active_auto_threshold = None;
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
                let completed_reset = self.active_reset_key.take();
                let completed_auto_threshold = self.active_auto_threshold.take();
                let acknowledged_threshold = completed_auto_threshold.filter(|(key, threshold)| {
                    threshold_saved_in_snapshot(accounts.as_deref(), key, *threshold)
                });
                if ok && let Some(accounts) = accounts {
                    self.accounts = accounts;
                }
                if ok
                    && !cancelled
                    && let Some((key, _)) = acknowledged_threshold
                {
                    self.auto_threshold_drafts.remove(&key);
                }
                if ok && !cancelled {
                    // A terminal provider response ends the local intent. Errors keep it
                    // available for an exact-ID retry after an ambiguous result.
                    if let Some(key) = completed_reset {
                        self.reset_intents.remove(&key);
                    }
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
                self.active_reset_key = None;
                self.active_auto_threshold = None;
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
                    let reset_pending = self.reset_intents.contains_key(&key)
                        || account
                            .banked_resets
                            .as_ref()
                            .and_then(|resets| resets.pending_request.as_ref())
                            .is_some();
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
                            if account.provider == "codex"
                                && let Some(resets) = &account.banked_resets
                            {
                                    ui.separator();
                                    ui.horizontal_wrapped(|ui| {
                                        ui.strong(match resets.available_count {
                                            Some(count) => format!("Banked resets: {count}"),
                                            None => "Banked resets: unknown".into(),
                                        });
                                        if ui.add_enabled(editable, egui::Button::new("Details / refresh")).clicked() {
                                            action = Some(Action::Resets(key.clone()));
                                        }
                                        if let Some(pending) = self.reset_intents.get(&key)
                                            .or(resets.pending_request.as_ref()) {
                                            ui.colored_label(Color32::from_rgb(240, 184, 98), "Reset pending or uncertain");
                                            if ui.add_enabled(editable, egui::Button::new("Retry same request")).clicked() {
                                                action = Some(Action::Reset(key.clone(), pending.credit_id.clone()));
                                            }
                                        } else if resets.can_redeem && let Some(credits) = &resets.credits {
                                            if let Some(credit) = credits.iter().find(|credit| credit.can_redeem) {
                                                let title = if credit.title.is_empty() { "Use a reset" } else { &credit.title };
                                                if ui.add_enabled(editable, egui::Button::new(title)).clicked() {
                                                    self.reset_confirmation = Some((key.clone(), Some(credit.id.clone())));
                                                }
                                            } else if ui.add_enabled(editable, egui::Button::new("Use next reset")).clicked() {
                                                self.reset_confirmation = Some((key.clone(), None));
                                            }
                                        } else if resets.can_redeem && ui.add_enabled(editable, egui::Button::new("Use next reset")).clicked() {
                                            self.reset_confirmation = Some((key.clone(), None));
                                        }
                                    });
                                    if !resets.stale.is_empty() { ui.small(format!("Reset data cached: {}", resets.stale)); }
                                    if !resets.error.is_empty() { ui.colored_label(Color32::from_rgb(241, 129, 129), &resets.error); }
                                    if let Some(credits) = &resets.credits {
                                        egui::CollapsingHeader::new(format!("Reset details ({} shown)", credits.len()))
                                            .id_salt(("reset-details", &key)).show(ui, |ui| {
                                                for credit in credits {
                                                    ui.horizontal_wrapped(|ui| {
                                                        ui.strong(if credit.title.is_empty() { &credit.reset_type } else { &credit.title });
                                                        ui.weak(&credit.status);
                                                        if !credit.expires_at.is_empty() { ui.label(format!("Expires {}", credit.expires_at)); }
                                                        if !reset_pending && resets.can_redeem && credit.can_redeem && ui.add_enabled(editable, egui::Button::new("Use")).clicked() {
                                                            self.reset_confirmation = Some((key.clone(), Some(credit.id.clone())));
                                                        }
                                                    });
                                                    if !credit.description.is_empty() { ui.small(&credit.description); }
                                                }
                                            });
                                    } else {
                                        ui.small("Reset details have not been loaded.");
                                    }
                                    let saved_threshold = resets.auto_reset_threshold_percent;
                                    let mut threshold_draft = current_threshold_draft(
                                        &mut self.auto_threshold_drafts,
                                        &key,
                                        saved_threshold,
                                    );
                                    ui.horizontal_wrapped(|ui| {
                                        ui.label(format!("Saved auto reset threshold: {saved_threshold}% remaining"));
                                        let mut enabled = resets.auto_reset;
                                        let toggle = ui.add_enabled(editable, egui::Checkbox::new(&mut enabled, "Enabled"));
                                        if toggle.changed() {
                                            action = Some(Action::AutoReset(key.clone(), Some(enabled), Some(threshold_draft)));
                                        }
                                    });
                                    ui.horizontal_wrapped(|ui| {
                                        ui.label("Reset at:");
                                        let response = ui.add_enabled(editable,
                                            egui::DragValue::new(&mut threshold_draft)
                                                .range(0..=99)
                                                .suffix("% remaining"),
                                        ).on_hover_text("0% remaining means the usage window is fully exhausted. The default is 1% remaining.");
                                        if response.changed() {
                                            threshold_draft = threshold_draft.min(99);
                                            if threshold_draft == saved_threshold {
                                                self.auto_threshold_drafts.remove(&key);
                                            } else {
                                                self.auto_threshold_drafts.insert(key.clone(), threshold_draft);
                                            }
                                        }
                                        if ui.add_enabled(
                                            editable && threshold_draft != saved_threshold,
                                            egui::Button::new("Save threshold"),
                                        ).clicked() {
                                            action = Some(Action::AutoReset(key.clone(), None, Some(threshold_draft)));
                                        }
                                    });
                                    ui.small(format!("Off by default. At {saved_threshold}% or less remaining in an eligible 5-hour or weekly window, this may reset both usage windows and move the weekly reset date."));
                                    if !resets.auto_reset_status.is_empty() { ui.small(format!("Auto reset: {}", resets.auto_reset_status)); }
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
        if let Some((key, credit_id)) = self.reset_confirmation.clone() {
            let account = self
                .accounts
                .iter()
                .find(|account| selector(account) == key);
            let identity = account
                .map(|account| {
                    let workspace = if account.org_name.is_empty() {
                        &account.org
                    } else {
                        &account.org_name
                    };
                    format!("{} ({})", account.email, workspace)
                })
                .unwrap_or_else(|| key.clone());
            let pending_now = self.reset_intents.contains_key(&key)
                || account
                    .and_then(|account| account.banked_resets.as_ref())
                    .and_then(|resets| resets.pending_request.as_ref())
                    .is_some();
            let credit_still_available = account
                .and_then(|account| account.banked_resets.as_ref())
                .is_some_and(|resets| {
                    resets.can_redeem
                        && credit_id.as_ref().is_none_or(|id| {
                            resets.credits.as_ref().is_some_and(|credits| {
                                credits
                                    .iter()
                                    .any(|credit| credit.id == *id && credit.can_redeem)
                            })
                        })
                });
            egui::Window::new("Confirm banked reset")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(format!("Use a banked reset for {identity}?"));
                    ui.colored_label(
                        Color32::from_rgb(240, 184, 98),
                        "This can refresh eligible 5-hour and weekly usage windows and move the weekly reset date.",
                    );
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                editable && !pending_now && credit_still_available,
                                egui::Button::new("Confirm reset"),
                            )
                            .clicked()
                        {
                            self.reset_confirmation = None;
                            self.start(Action::Reset(key, credit_id), ctx.clone());
                        }
                        if ui.button("Cancel").clicked() {
                            self.reset_confirmation = None;
                        }
                    });
                });
        }
        ctx.request_repaint_after(Duration::from_secs(1));
    }
}
fn get_or_create_reset_intent<'a>(
    intents: &'a mut std::collections::HashMap<String, PendingResetRequest>,
    key: &str,
    backend_pending: Option<PendingResetRequest>,
    selected_credit: Option<String>,
) -> &'a PendingResetRequest {
    if let Some(pending) = backend_pending {
        // The persisted backend request is authoritative, including when a prior
        // local request never reached the backend or was replaced elsewhere.
        intents.insert(key.to_owned(), pending);
        return intents.get(key).expect("inserted backend reset intent");
    }
    intents
        .entry(key.to_owned())
        .or_insert_with(|| PendingResetRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            credit_id: selected_credit,
        })
}

fn selector(a: &Account) -> String {
    format!("{}:{}#{}", a.provider, a.email, a.org)
}
fn threshold_saved_in_snapshot(accounts: Option<&[Account]>, key: &str, threshold: u8) -> bool {
    accounts.is_some_and(|accounts| {
        accounts.iter().any(|account| {
            selector(account) == key
                && account
                    .banked_resets
                    .as_ref()
                    .is_some_and(|resets| resets.auto_reset_threshold_percent == threshold)
        })
    })
}
fn current_threshold_draft(
    drafts: &mut std::collections::HashMap<String, u8>,
    key: &str,
    saved: u8,
) -> u8 {
    match drafts.get(key).copied() {
        Some(draft) if draft != saved => draft,
        Some(_) => {
            drafts.remove(key);
            saved
        }
        None => saved,
    }
}
fn selector_args(action: &'static str, key: String) -> (&'static str, Vec<String>) {
    let mut parts = key.splitn(2, ':');
    let provider = parts.next().unwrap_or_default().to_string();
    let selector = parts.next().unwrap_or_default().to_string();
    (action, vec!["--provider".into(), provider, selector])
}
fn auto_reset_args(key: &str, enabled: Option<bool>, threshold: Option<u8>) -> Vec<String> {
    let mut args = selector_args("auto-reset", key.to_owned()).1;
    if let Some(enabled) = enabled {
        args.extend(["--enabled".into(), enabled.to_string()]);
    }
    if let Some(threshold) = threshold {
        args.extend(["--threshold".into(), threshold.to_string()]);
    }
    args
}

#[cfg(test)]
mod tests {
    use super::{
        auto_reset_args, current_threshold_draft, get_or_create_reset_intent,
        threshold_saved_in_snapshot,
    };
    use crate::protocol::{Account, PendingResetRequest};
    use std::collections::HashMap;

    #[test]
    fn uncertain_reset_retry_reuses_the_confirmed_request_id_and_credit() {
        let mut intents = HashMap::new();
        let first =
            get_or_create_reset_intent(&mut intents, "codex:a#org", None, Some("credit-7".into()))
                .clone();
        // Simulate a failed or ambiguous operation; retrying must keep the same id and credit.
        let retry = get_or_create_reset_intent(&mut intents, "codex:a#org", None, None).clone();
        assert!(!first.request_id.is_empty());
        assert_eq!(first.request_id, retry.request_id);
        assert_eq!(retry.credit_id.as_deref(), Some("credit-7"));
    }

    #[test]
    fn backend_pending_request_wins_when_restarting_the_frontend() {
        let mut intents = HashMap::new();
        let pending = PendingResetRequest {
            request_id: "11111111-1111-4111-8111-111111111111".into(),
            credit_id: None,
        };
        let intent = get_or_create_reset_intent(&mut intents, "codex:a#org", Some(pending), None);
        assert_eq!(intent.request_id, "11111111-1111-4111-8111-111111111111");
    }

    #[test]
    fn backend_pending_request_replaces_a_local_intent_that_was_never_accepted() {
        let mut intents = HashMap::from([(
            "codex:a#org".into(),
            PendingResetRequest {
                request_id: "22222222-2222-4222-8222-222222222222".into(),
                credit_id: Some("stale-credit".into()),
            },
        )]);
        let backend = PendingResetRequest {
            request_id: "11111111-1111-4111-8111-111111111111".into(),
            credit_id: Some("authoritative-credit".into()),
        };
        let intent = get_or_create_reset_intent(&mut intents, "codex:a#org", Some(backend), None);
        assert_eq!(intent.request_id, "11111111-1111-4111-8111-111111111111");
        assert_eq!(intent.credit_id.as_deref(), Some("authoritative-credit"));
    }

    #[test]
    fn threshold_save_preserves_enabled_state_and_sends_only_the_threshold() {
        let args = auto_reset_args("codex:a#org", None, Some(0));
        assert_eq!(args, ["--provider", "codex", "a#org", "--threshold", "0"]);
        assert!(!args.iter().any(|arg| arg == "--enabled"));
    }

    #[test]
    fn enabling_auto_reset_atomically_sends_the_current_threshold_draft() {
        let args = auto_reset_args("codex:a#org", Some(true), Some(37));
        assert_eq!(
            args,
            [
                "--provider",
                "codex",
                "a#org",
                "--enabled",
                "true",
                "--threshold",
                "37"
            ]
        );
    }

    #[test]
    fn local_threshold_draft_requires_matching_authoritative_snapshot_to_clear() {
        let key = "codex:a#org";
        assert!(!threshold_saved_in_snapshot(None, key, 42));
        let accounts = vec![Account {
            provider: "codex".into(),
            email: "a".into(),
            org: "org".into(),
            banked_resets: Some(crate::protocol::BankedResets {
                auto_reset_threshold_percent: 41,
                ..Default::default()
            }),
            ..Default::default()
        }];
        assert!(!threshold_saved_in_snapshot(Some(&accounts), key, 42));
        assert!(threshold_saved_in_snapshot(Some(&accounts), key, 41));
    }

    #[test]
    fn external_threshold_updates_refresh_clean_controls_and_preserve_dirty_drafts() {
        let key = "codex:a#org";
        let mut drafts = HashMap::new();
        assert_eq!(current_threshold_draft(&mut drafts, key, 5), 5);
        assert_eq!(current_threshold_draft(&mut drafts, key, 12), 12);
        assert!(drafts.is_empty());

        drafts.insert(key.into(), 37);
        assert_eq!(current_threshold_draft(&mut drafts, key, 20), 37);
        assert_eq!(drafts.get(key), Some(&37));

        assert_eq!(current_threshold_draft(&mut drafts, key, 37), 37);
        assert!(drafts.is_empty());
    }
}
