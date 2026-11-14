//! The app: owns the backend and everything the views show; routes backend
//! events into that state, and commands, view actions, and dialog results
//! to the backend.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use egui::{Frame, Margin, Ui};
use laankit_egui::about::{AboutAction, AboutIcon, AboutModal};
use laankit_egui::commands::{Recents, Registry};
use laankit_egui::diagnostics::{self, Screenshot};
use laankit_egui::dialog::{self, Confirm};
use laankit_egui::palette::{CommandPalette, PaletteOutcome};
use laankit_egui::theme::{self, ThemeState};
use laankit_egui::titlebar::{self, TitleBar, TitleBarAction};
use laankit_egui::toast::Toasts;
use laankit_egui::{fonts, icons, window};
use printliner_core::history::Sample;
use printliner_core::{Device, PrinterStatus, Task};
use serde_json::json;

use crate::backend::{self, Account, Backend, Connection, Event, Request, SignInStep};
use crate::commands::{self, Act, View};
use crate::config::{Paths, Settings, TokenStorage, TrendRange};
use crate::detail::{self, DetailAction, DetailProps};
use crate::model::Printer;
use crate::model::Tone;
use crate::overlays::settings::{AccountProps, PrinterRow, SettingsAction, SettingsState};
use crate::overlays::signin::{SignInAction, SignInState, Step};
use crate::overlays::{self, Overlay};
use crate::views::{self, GridProps, StatusProps, ToolbarProps};

const THEME_KEY: &str = "printliner-theme";
const RECENTS_KEY: &str = "printliner-recents";
const DESCRIPTION: &str =
    "A dashboard for your Bambu Lab 3D printers, through the Bambu Lab cloud. Built on the laankit framework.";
const REPO_URL: &str = "https://github.com/NathanLaan/laankit";
/// Warn this many days before the sign-in expires.
const EXPIRY_WARNING_DAYS: i64 = 7;
/// How often to redraw while signed in, so "No signal" appears on time.
const STALE_CHECK_SECS: f32 = 5.0;

/// How the app was started.
#[derive(Clone, Debug, Default)]
pub struct Launch {
    pub demo: bool,
    /// Tests: a mock REST server and a plain-TCP broker.
    pub api_base: Option<String>,
    pub mqtt_override: Option<(String, u16)>,
}

pub struct PrintLinerApp {
    paths: Paths,
    settings: Settings,
    theme: ThemeState,
    /// The scale changed from a command: apply it next frame.
    theme_dirty: bool,
    custom_titlebar: bool,
    status_bar_visible: bool,
    trend_range: TrendRange,
    initialized: bool,
    backend: Backend,
    account: Account,
    connection: Connection,
    devices: Vec<Device>,
    statuses: HashMap<String, (PrinterStatus, DateTime<Utc>)>,
    tasks: HashMap<String, Vec<Task>>,
    covers: HashMap<String, Arc<[u8]>>,
    /// Covers asked for (by task id), so each is asked for once.
    asked_covers: HashSet<String>,
    history: HashMap<String, Vec<Sample>>,
    /// The printer whose detail is showing.
    selected: Option<String>,
    overlay: Option<Overlay>,
    palette: Option<CommandPalette>,
    registry: Registry<Act, View>,
    toasts: Toasts,
    screenshot: Option<Screenshot>,
    open_on_start: Option<String>,
    /// `LAANKIT_OPEN=printer:<serial>`: open it when the printers arrive.
    pending_select: Option<String>,
}

impl PrintLinerApp {
    /// `storage` is eframe's (theme, scale, palette recents); `None` in tests.
    pub fn new(storage: Option<&dyn eframe::Storage>, paths: Paths, launch: Launch) -> Self {
        let prefs = paths.prefs();
        let settings = paths.settings();
        let mut theme: ThemeState = storage
            .and_then(|s| eframe::get_value(s, THEME_KEY))
            .unwrap_or_default();
        diagnostics::theme_overrides(&mut theme);
        let mut registry = commands::registry();
        if let Some(recents) = storage.and_then(|s| eframe::get_value::<Recents>(s, RECENTS_KEY)) {
            registry.load_recents(recents);
        }
        let backend = Backend::new(backend::Config {
            region: settings.region,
            email: settings.email.clone(),
            token_path: paths.token_file(),
            token_in_file: settings.token_storage == TokenStorage::File,
            history_dir: paths.history_dir(),
            covers_dir: paths.covers_dir(),
            user_agent: format!("PrintLiner/{}", env!("CARGO_PKG_VERSION")),
            demo: launch.demo,
            api_base: launch.api_base,
            mqtt_override: launch.mqtt_override,
        });
        Self {
            paths,
            settings,
            theme,
            theme_dirty: false,
            custom_titlebar: prefs.custom_titlebar,
            status_bar_visible: prefs.status_bar_visible,
            trend_range: prefs.trend_range,
            initialized: false,
            backend,
            account: Account::Starting,
            connection: Connection::Idle,
            devices: Vec::new(),
            statuses: HashMap::new(),
            tasks: HashMap::new(),
            covers: HashMap::new(),
            asked_covers: HashSet::new(),
            history: HashMap::new(),
            selected: None,
            overlay: None,
            palette: None,
            registry,
            toasts: Toasts::default(),
            screenshot: Screenshot::from_env(),
            open_on_start: std::env::var("LAANKIT_OPEN").ok(),
            pending_select: None,
        }
    }

    fn init(&mut self, ctx: &egui::Context) {
        if self.initialized {
            return;
        }
        self.initialized = true;
        self.backend.attach(ctx);
        fonts::install(ctx);
        egui_extras::install_image_loaders(ctx);
        self.theme.apply(ctx);
        match self.open_on_start.take().as_deref() {
            Some("settings") => self.apply(Act::Settings),
            Some("about") => self.apply(Act::About),
            Some("palette") => self.apply(Act::Palette),
            Some("signin") => self.apply(Act::SignIn),
            Some(other) => {
                // `printer:<serial>`: that printer's detail, once it's listed.
                self.pending_select = other.strip_prefix("printer:").map(str::to_owned);
            }
            None => {}
        }
    }

    // --- Accessors for tests ------------------------------------------------

    #[cfg(test)]
    pub fn account(&self) -> &Account {
        &self.account
    }

    #[cfg(test)]
    pub fn printers_for_test(&self) -> Vec<Printer> {
        self.printers()
    }

    #[cfg(test)]
    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    #[cfg(test)]
    pub fn has_overlay(&self) -> bool {
        self.overlay.is_some()
    }

    #[cfg(test)]
    pub fn tasks_for(&self, serial: &str) -> Option<&Vec<Task>> {
        self.tasks.get(serial)
    }

    #[cfg(test)]
    pub fn run(&mut self, act: Act) {
        self.apply(act);
    }

    fn signed_in(&self) -> bool {
        matches!(self.account, Account::SignedIn { .. })
    }

    fn demo(&self) -> bool {
        matches!(self.account, Account::SignedIn { demo: true, .. })
    }

    fn view(&self) -> View {
        View {
            modal_open: self.overlay.is_some() || self.palette.is_some(),
            signed_in: self.signed_in(),
            demo: self.demo(),
            detail_open: self.selected.is_some(),
        }
    }

    fn top_inset(&self) -> f32 {
        if self.custom_titlebar {
            theme::TITLEBAR_HEIGHT
        } else {
            0.0
        }
    }

    /// The printers in the order Settings gives, then the rest by name.
    fn printers(&self) -> Vec<Printer> {
        let mut out: Vec<Printer> = self
            .devices
            .iter()
            .map(|d| {
                let status = self.statuses.get(&d.serial);
                Printer {
                    name: self
                        .settings
                        .printer_names
                        .get(&d.serial)
                        .cloned()
                        .unwrap_or_else(|| d.name.clone()),
                    device: d.clone(),
                    status: status.map(|s| s.0.clone()),
                    at: status.map(|s| s.1),
                }
            })
            .collect();
        let order = &self.settings.printer_order;
        out.sort_by(|a, b| {
            let pos = |p: &Printer| order.iter().position(|s| *s == p.device.serial);
            match (pos(a), pos(b)) {
                (Some(x), Some(y)) => x.cmp(&y),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            }
        });
        out
    }

    fn save_settings(&mut self) {
        if let Err(e) = self.paths.save_settings(&self.settings) {
            self.toasts.error(format!("Couldn't save settings: {e:#}"));
        }
    }

    fn set_prefs(&mut self, patch: serde_json::Value) {
        if let Err(e) = self.paths.set_prefs(&patch) {
            self.toasts
                .error(format!("Couldn't save preferences: {e:#}"));
        }
    }

    fn apply(&mut self, act: Act) {
        match act {
            Act::Printers => self.selected = None,
            Act::Refresh => {
                self.backend.send(Request::Refresh);
                self.toasts.info("Refreshing…");
            }
            Act::SignIn => {
                if !self.demo() {
                    self.overlay = Some(Overlay::SignIn(SignInState::new(
                        self.settings.region,
                        self.settings.email.clone(),
                    )));
                }
            }
            Act::SignOut => self.overlay = Some(Overlay::ConfirmSignOut),
            Act::ZoomIn => {
                self.theme.zoom_in();
                self.theme_dirty = true;
            }
            Act::ZoomOut => {
                self.theme.zoom_out();
                self.theme_dirty = true;
            }
            Act::ZoomReset => {
                self.theme.zoom_reset();
                self.theme_dirty = true;
            }
            Act::ToggleStatusBar => {
                self.status_bar_visible = !self.status_bar_visible;
                self.set_prefs(json!({ "statusBarVisible": self.status_bar_visible }));
            }
            Act::Settings => self.open_settings("account"),
            Act::About => self.overlay = Some(Overlay::About),
            Act::Palette => self.palette = Some(CommandPalette::new()),
        }
    }

    fn open_settings(&mut self, tab: &str) {
        let rows = self
            .printers()
            .into_iter()
            .map(|p| PrinterRow {
                serial: p.device.serial.clone(),
                account_name: p.device.name.clone(),
                name: self
                    .settings
                    .printer_names
                    .get(&p.device.serial)
                    .cloned()
                    .unwrap_or_default(),
            })
            .collect();
        self.overlay = Some(Overlay::Settings(SettingsState::new(
            tab,
            self.custom_titlebar,
            self.custom_titlebar,
            self.status_bar_visible,
            rows,
        )));
    }

    fn select(&mut self, serial: String) {
        self.selected = Some(serial.clone());
        self.load_history(&serial);
        if !self.tasks.contains_key(&serial) {
            self.backend.send(Request::Tasks(serial));
        }
    }

    fn load_history(&mut self, serial: &str) {
        self.history.remove(serial);
        self.backend.send(Request::History {
            serial: serial.to_string(),
            since: Utc::now() - self.trend_range.duration(),
        });
    }

    fn want_covers(&mut self, wanted: Vec<(String, String)>) {
        for (task_id, url) in wanted {
            if self.asked_covers.insert(task_id.clone()) {
                self.backend.send(Request::Cover { task_id, url });
            }
        }
    }

    // --- Backend events -----------------------------------------------------

    fn backend_events(&mut self) {
        for event in self.backend.poll() {
            match event {
                Event::Account(account) => {
                    if matches!(account, Account::SignInNeeded { .. })
                        && !matches!(
                            self.account,
                            Account::SignInNeeded { .. } | Account::Starting
                        )
                    {
                        self.toasts.error("Bambu Lab needs you to sign in again.");
                    }
                    if matches!(account, Account::SignedOut) {
                        self.selected = None;
                        self.statuses.clear();
                        self.tasks.clear();
                        self.history.clear();
                    }
                    self.account = account;
                }
                Event::SignIn(step) => self.sign_in_step(step),
                Event::KeychainUnavailable => self.overlay = Some(Overlay::KeychainFallback),
                Event::Devices(devices) => {
                    let serials: HashSet<&str> =
                        devices.iter().map(|d| d.serial.as_str()).collect();
                    self.statuses.retain(|s, _| serials.contains(s.as_str()));
                    if self
                        .selected
                        .as_deref()
                        .is_some_and(|s| !serials.contains(s))
                    {
                        self.selected = None;
                    }
                    self.devices = devices;
                    if let Some(serial) = self
                        .pending_select
                        .take_if(|s| self.devices.iter().any(|d| &d.serial == s))
                    {
                        self.select(serial);
                    }
                }
                Event::Connection(c) => self.connection = c,
                Event::Status { serial, status, at } => {
                    self.statuses.insert(serial, (*status, at));
                }
                Event::Tasks { serial, tasks } => {
                    self.tasks.insert(serial, tasks);
                }
                Event::Cover { task_id, bytes } => {
                    self.covers.insert(task_id, bytes);
                }
                Event::History { serial, samples } => {
                    self.history.insert(serial, samples);
                }
                Event::Error(message) => self.toasts.error(message),
            }
        }
    }

    fn sign_in_step(&mut self, step: SignInStep) {
        let state = match &mut self.overlay {
            Some(Overlay::SignIn(s)) => Some(s),
            _ => None,
        };
        match step {
            SignInStep::Done { region, email } => {
                self.settings.region = region;
                self.settings.email = Some(email);
                self.save_settings();
                if matches!(self.overlay, Some(Overlay::SignIn(_))) {
                    self.overlay = None;
                }
                self.toasts.success("Signed in.");
            }
            SignInStep::EmailCode => {
                if let Some(s) = state {
                    s.step = Step::Code;
                    s.busy = false;
                    s.error = None;
                    s.code.clear();
                }
            }
            SignInStep::Authenticator => {
                if let Some(s) = state {
                    s.step = Step::Authenticator;
                    s.busy = false;
                }
            }
            SignInStep::Failed(message) => match state {
                Some(s) => s.fail(message),
                None => self.toasts.error(message),
            },
        }
    }

    // --- Frame --------------------------------------------------------------

    pub fn show(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.init(&ctx);
        if std::mem::take(&mut self.theme_dirty) {
            self.theme.apply(&ctx);
        }
        self.backend_events();
        if ctx.egui_wants_keyboard_input() {
            // A text field has the keys.
        } else {
            let view = self.view();
            for act in self.registry.dispatch_keys(&ctx, &view) {
                self.apply(act);
            }
        }
        if self.custom_titlebar {
            self.titlebar(ui);
        }
        self.main_view(ui);
        self.overlays(&ctx);
        self.palette(&ctx);
        self.toasts.show(&ctx);
        if self.custom_titlebar {
            window::window_edges(&ctx);
        }
        if self.signed_in() {
            ctx.request_repaint_after(std::time::Duration::from_secs_f32(STALE_CHECK_SECS));
        }
        if let Some(shot) = &mut self.screenshot {
            shot.tick(&ctx);
        }
    }

    fn titlebar(&mut self, ui: &mut Ui) {
        let bar = TitleBar {
            app_name: "PrintLiner",
            toolbar_visible: true,
        };
        let mut act = None;
        let actions = titlebar::show(ui, &bar, &mut self.theme, |ui| {
            if titlebar::button(ui, icons::CIRCLE_INFO, "About", false).clicked() {
                act = Some(Act::About);
            }
            if titlebar::button(ui, icons::GEAR, "Settings", false).clicked() {
                act = Some(Act::Settings);
            }
        });
        for a in actions {
            match a {
                TitleBarAction::ToggleToolbar => {}
                TitleBarAction::ScaleChanged(_) => {}
            }
        }
        if let Some(a) = act {
            self.apply(a);
        }
    }

    fn main_view(&mut self, ui: &mut Ui) {
        let props = ToolbarProps {
            detail_open: self.selected.is_some(),
            can_refresh: self.signed_in(),
            settings_open: matches!(self.overlay, Some(Overlay::Settings(_))),
            about_open: matches!(self.overlay, Some(Overlay::About)),
        };
        let acts = views::toolbar(ui, &props);
        let printers = self.printers();
        if self.status_bar_visible {
            let status = StatusProps {
                connection: &self.connection,
                signed_in: self.signed_in(),
                demo: self.demo(),
                printers: printers.len(),
                printing: printers
                    .iter()
                    .filter(|p| p.status.as_ref().is_some_and(|s| s.state.is_active()))
                    .count(),
                token_in_file: matches!(
                    self.account,
                    Account::SignedIn {
                        token_in_file: Some(true),
                        demo: false,
                        ..
                    }
                ),
            };
            egui::Panel::bottom("printliner-status")
                .exact_size(views::STATUS_BAR_HEIGHT)
                .resizable(false)
                .show_separator_line(false)
                .frame(Frame::NONE)
                .show(ui, |ui| views::status_bar(ui, &status));
        }
        let p = theme::palette(ui.ctx());
        let mut sign_in = false;
        let mut clicked = None;
        let mut wanted = Vec::new();
        let mut detail_acts = Vec::new();
        let now = Utc::now();
        let connected = self.connection == Connection::Connected;
        egui::CentralPanel::default()
            .frame(Frame::NONE.fill(p.bg_base).inner_margin(Margin::same(20)))
            .show(ui, |ui| {
                match &self.account {
                    Account::Starting => return,
                    Account::SignedOut => {
                        sign_in = views::welcome(ui);
                        return;
                    }
                    Account::SignInNeeded { .. } => {
                        sign_in |= views::banner(
                            ui,
                            "Bambu Lab needs you to sign in again before PrintLiner can show your printers.",
                            "Sign In",
                            Tone::Bad,
                        );
                    }
                    Account::SignedIn {
                        expires_at: Some(at),
                        demo: false,
                        ..
                    } => {
                        let days = (*at - now).num_days();
                        if days < EXPIRY_WARNING_DAYS {
                            let message = match days {
                                d if d <= 0 => "Your Bambu Lab sign-in expires today.".to_string(),
                                1 => "Your Bambu Lab sign-in expires tomorrow.".to_string(),
                                d => format!("Your Bambu Lab sign-in expires in {d} days."),
                            };
                            sign_in |= views::banner(ui, &message, "Sign In Again", Tone::Warn);
                        }
                    }
                    Account::SignedIn { .. } => {}
                }
                if let Some(serial) = &self.selected {
                    if let Some(printer) = printers.iter().find(|p| &p.device.serial == serial) {
                        let props = DetailProps {
                            now,
                            connected,
                            tasks: self.tasks.get(serial),
                            covers: &self.covers,
                            samples: self.history.get(serial),
                            range: self.trend_range,
                        };
                        detail_acts = detail::show(ui, printer, &props);
                        return;
                    }
                }
                if printers.is_empty() {
                    if self.signed_in() {
                        views::empty(ui, "No printers on this Bambu Lab account yet.");
                    }
                    return;
                }
                let props = GridProps {
                    now,
                    connected,
                    tasks: &self.tasks,
                    covers: &self.covers,
                };
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let (c, w) = views::printer_grid(ui, &printers, &props);
                        clicked = c;
                        wanted = w;
                    });
            });
        for act in acts {
            self.apply(act);
        }
        if sign_in {
            self.apply(Act::SignIn);
        }
        if let Some(serial) = clicked {
            self.select(serial);
        }
        self.want_covers(wanted);
        for a in detail_acts {
            match a {
                DetailAction::Back => self.selected = None,
                DetailAction::Range(r) => {
                    self.trend_range = r;
                    self.set_prefs(json!({ "trendRange": r.id() }));
                    if let Some(serial) = self.selected.clone() {
                        self.load_history(&serial);
                    }
                }
                DetailAction::WantCovers(w) => self.want_covers(w),
                DetailAction::OpenUrl(url) => ui.ctx().open_url(egui::OpenUrl::new_tab(url)),
            }
        }
    }

    // --- Dialogs ------------------------------------------------------------

    fn overlays(&mut self, ctx: &egui::Context) {
        let Some(mut overlay) = self.overlay.take() else {
            return;
        };
        let top_inset = self.top_inset();
        let mut keep = true;
        match &mut overlay {
            Overlay::SignIn(state) => match overlays::signin::show(ctx, state) {
                Some(SignInAction::Close) => keep = false,
                Some(SignInAction::Submit {
                    region,
                    email,
                    password,
                }) => {
                    state.busy = true;
                    state.error = None;
                    self.backend.send(Request::SignIn {
                        region,
                        email,
                        password,
                    });
                    state.password.clear();
                }
                Some(SignInAction::CodeOnly { region, email }) => {
                    state.busy = true;
                    state.error = None;
                    state.password.clear();
                    self.backend.send(Request::EmailCodeOnly { region, email });
                }
                Some(SignInAction::SubmitCode(code)) => {
                    state.busy = true;
                    state.error = None;
                    self.backend.send(Request::SubmitCode(code));
                }
                Some(SignInAction::Resend) => {
                    state.error = None;
                    state.code.clear();
                    self.backend.send(Request::ResendCode);
                    self.toasts.info("A new code is on its way.");
                }
                None => {}
            },
            Overlay::Settings(state) => {
                let account = AccountProps {
                    account: &self.account,
                    region: self.settings.region,
                };
                let actions = overlays::settings::show(ctx, state, &mut self.theme, &account, top_inset);
                for a in actions {
                    match a {
                        SettingsAction::Close => keep = false,
                        SettingsAction::ThemeChanged => {}
                        SettingsAction::SetCustomTitlebar(on) => {
                            self.set_prefs(json!({ "customTitlebar": on }));
                        }
                        SettingsAction::Restart => restart(ctx),
                        SettingsAction::SetStatusBar(on) => {
                            self.status_bar_visible = on;
                            self.set_prefs(json!({ "statusBarVisible": on }));
                        }
                        SettingsAction::SignIn => {
                            keep = false;
                            self.apply(Act::SignIn);
                        }
                        SettingsAction::SignOut => {
                            keep = false;
                            self.apply(Act::SignOut);
                        }
                        SettingsAction::Rename(serial, name) => {
                            if name.is_empty() {
                                self.settings.printer_names.remove(&serial);
                            } else {
                                self.settings.printer_names.insert(serial, name);
                            }
                            self.save_settings();
                        }
                        SettingsAction::Reorder(order) => {
                            self.settings.printer_order = order;
                            self.save_settings();
                        }
                    }
                }
            }
            Overlay::About => {
                let modal = AboutModal::new("PrintLiner", env!("CARGO_PKG_VERSION"))
                    .description(DESCRIPTION)
                    .repo(REPO_URL, Some("github.com/NathanLaan/laankit"))
                    .icon(AboutIcon::Glyph(icons::LAYER_GROUP));
                match modal.show(ctx) {
                    Some(AboutAction::Close) => keep = false,
                    Some(AboutAction::OpenUrl(url)) => ctx.open_url(egui::OpenUrl::new_tab(url)),
                    Some(_) | None => {}
                }
            }
            Overlay::ConfirmSignOut => match dialog::confirm(
                ctx,
                "printliner-signout",
                "Sign Out",
                "Sign out of your Bambu Lab account?",
                Some("PrintLiner forgets the sign-in token. Your printers and their history on Bambu Lab aren't affected."),
                "Sign Out",
            ) {
                Some(Confirm::Yes) => {
                    self.backend.send(Request::SignOut);
                    keep = false;
                }
                Some(Confirm::No) => keep = false,
                None => {}
            },
            Overlay::KeychainFallback => match dialog::confirm(
                ctx,
                "printliner-keychain",
                "No System Keychain",
                "PrintLiner couldn't save your sign-in token to the system keychain. Save it in a file only your user account can read?",
                Some("If you choose Not Now, you stay signed in until PrintLiner closes, and sign in again next time."),
                "Save in File",
            ) {
                Some(Confirm::Yes) => {
                    self.backend.send(Request::StoreTokenInFile(true));
                    self.settings.token_storage = TokenStorage::File;
                    self.save_settings();
                    keep = false;
                }
                Some(Confirm::No) => {
                    self.backend.send(Request::StoreTokenInFile(false));
                    keep = false;
                }
                None => {}
            },
        }
        if keep && self.overlay.is_none() {
            self.overlay = Some(overlay);
        }
    }

    fn palette(&mut self, ctx: &egui::Context) {
        let Some(mut palette) = self.palette.take() else {
            return;
        };
        let view = View {
            modal_open: false,
            ..self.view()
        };
        match palette.show(ctx, &mut self.registry, &view, &mut |_| Vec::new()) {
            PaletteOutcome::Open => self.palette = Some(palette),
            PaletteOutcome::Closed => {}
            PaletteOutcome::Run(act) => self.apply(act),
        }
    }

    pub fn save_state(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, THEME_KEY, &self.theme);
        eframe::set_value(storage, RECENTS_KEY, &self.registry.recents());
    }
}

impl eframe::App for PrintLinerApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.save_state(storage);
    }
}

/// Starts a new copy with the same arguments, then closes this one.
fn restart(ctx: &egui::Context) {
    match std::env::current_exe() {
        Ok(exe) => {
            let args: Vec<String> = std::env::args().skip(1).collect();
            if let Err(e) = std::process::Command::new(exe).args(args).spawn() {
                log::error!("relaunch failed: {e}");
                return;
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        Err(e) => log::error!("relaunch failed: {e}"),
    }
}
