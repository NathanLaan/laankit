//! The app: owns the backend, the data the panes show and the open dialog;
//! routes backend events to whoever is waiting for them, and commands, pane
//! actions and dialog results to the backend. The equivalent of
//! `App.svelte` plus the `stores/app.js` and `stores/sync.js` actions.

use egui::{Frame, Ui};
use marina_egui::about::{AboutAction, AboutIcon, AboutModal, UpdateStatus};
use marina_egui::commands::{Recents, Registry};
use marina_egui::diagnostics::{self, Screenshot};
use marina_egui::dialog::Confirm;
use marina_egui::palette::{CommandPalette, ItemKind, PaletteItem, PaletteOutcome};
use marina_egui::theme::{self, ThemeId, ThemeState};
use marina_egui::titlebar::{self, TitleBar, TitleBarAction};
use marina_egui::toast::Toasts;
use marina_egui::toolbar::{self, ToolbarButton};
use marina_egui::{fonts, icons, window};
use serde_json::{json, Value};
use threadliner_core::sync::LogEntry;

use crate::backend::{Backend, Event, Op, Settings};
use crate::commands::{self, Act, View};
use crate::config::Paths;
use crate::model::{Entry, Feed, Tag};
use crate::overlays::settings::{SettingsAction, SettingsState, SyncView};
use crate::overlays::sync::{SyncAction, SyncState};
use crate::overlays::{self, FeedForm, FormAction, Overlay, TagSub, TagsAction, TagsState};
use crate::views::{self, Article, SetupForm, ViewAction};

/// eframe storage keys (the Svelte build's `threadliner-theme` /
/// `threadliner-scale` localStorage keys).
const THEME_KEY: &str = "threadliner-theme";
const RECENTS_KEY: &str = "threadliner-recents";
/// `pollSyncStatus` runs every 2 s.
const SYNC_POLL_SECS: f64 = 2.0;
/// The sync log keeps the last 200 entries, like the engine.
const LOG_CAP: usize = 200;
/// Ctrl+wheel zoom steps at most once per 200 ms (`ZOOM_COOLDOWN_MS`).
const ZOOM_COOLDOWN_SECS: f64 = 0.2;
const DESCRIPTION: &str = "A desktop RSS reader built with Rust, egui, and Git-synced JSON.";
const REPO_URL: &str = "https://github.com/NathanLaan/marina/";

/// `stores/sync.js`: the auto-sync engine's state, polled every 2 s.
#[derive(Default)]
struct SyncInfo {
    status: String,
    last_sync_time: Option<String>,
    log: Vec<LogEntry>,
    last_log_id: u64,
    next_poll: f64,
}

/// `MARINA_OPEN` / `MARINA_FEED` / `MARINA_ENTRY`: what to open once the
/// data is in, for screenshots.
#[derive(Default)]
struct Diagnostics {
    open: Option<String>,
    feed: Option<usize>,
    entry: Option<usize>,
}

pub struct ThreadLinerApp {
    backend: Backend,
    theme: ThemeState,
    /// Whether `theme` came from local storage; if not, the synced settings
    /// supply it (`hydrateFromSettings`).
    theme_saved: bool,
    hydrated: bool,
    /// Whether the window was built undecorated, with Marina's title bar.
    custom_titlebar: bool,
    toolbar_visible: bool,
    initialized: bool,
    loading: bool,
    setup_complete: bool,
    setup: SetupForm,
    feeds: Vec<Feed>,
    tags: Vec<Tag>,
    entries: Vec<Entry>,
    entries_loading: bool,
    selected_feed: Option<String>,
    selected_entry: Option<String>,
    article: Option<Article>,
    settings: Settings,
    sync: SyncInfo,
    overlay: Option<Overlay>,
    palette: Option<CommandPalette>,
    registry: Registry<Act, View>,
    toasts: Toasts,
    update: UpdateStatus,
    last_wheel_zoom: f64,
    screenshot: Option<Screenshot>,
    diagnostics: Diagnostics,
}

impl ThreadLinerApp {
    /// `storage` is eframe's (theme, scale, palette recents); `None` in tests.
    pub fn new(storage: Option<&dyn eframe::Storage>, paths: Paths) -> Self {
        let custom_titlebar = paths.custom_titlebar();
        let saved: Option<ThemeState> = storage.and_then(|s| eframe::get_value(s, THEME_KEY));
        let theme_saved = saved.is_some();
        let mut theme = saved.unwrap_or_default();
        diagnostics::theme_overrides(&mut theme);
        let mut registry = commands::registry();
        if let Some(recents) = storage.and_then(|s| eframe::get_value::<Recents>(s, RECENTS_KEY)) {
            registry.load_recents(recents);
        }
        let env = |k: &str| std::env::var(k).ok();
        Self {
            backend: Backend::new(paths),
            theme,
            theme_saved,
            hydrated: false,
            custom_titlebar,
            toolbar_visible: true,
            initialized: false,
            loading: true,
            setup_complete: false,
            setup: SetupForm::default(),
            feeds: Vec::new(),
            tags: Vec::new(),
            entries: Vec::new(),
            entries_loading: false,
            selected_feed: None,
            selected_entry: None,
            article: None,
            settings: Settings::default(),
            sync: SyncInfo::default(),
            overlay: None,
            palette: None,
            registry,
            toasts: Toasts::default(),
            update: UpdateStatus::Idle,
            last_wheel_zoom: f64::NEG_INFINITY,
            screenshot: Screenshot::from_env(),
            diagnostics: Diagnostics {
                open: env("MARINA_OPEN"),
                feed: env("MARINA_FEED").and_then(|v| v.parse().ok()),
                entry: env("MARINA_ENTRY").and_then(|v| v.parse().ok()),
            },
        }
    }

    /// Fonts, image loaders and the theme, once per context.
    fn init(&mut self, ctx: &egui::Context) {
        if self.initialized {
            return;
        }
        self.initialized = true;
        fonts::install(ctx);
        egui_extras::install_image_loaders(ctx);
        self.theme.apply(ctx);
        self.backend.attach(ctx);
    }

    // --- Accessors for tests ------------------------------------------------

    #[cfg(test)]
    pub fn feeds(&self) -> &[Feed] {
        &self.feeds
    }

    #[cfg(test)]
    pub fn tags(&self) -> &[Tag] {
        &self.tags
    }

    #[cfg(test)]
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    #[cfg(test)]
    pub fn is_ready(&self) -> bool {
        !self.loading
    }

    #[cfg(test)]
    pub fn is_setup_complete(&self) -> bool {
        self.setup_complete
    }

    #[cfg(test)]
    pub fn selected_entry(&self) -> Option<&str> {
        self.selected_entry.as_deref()
    }

    #[cfg(test)]
    pub fn has_overlay(&self) -> bool {
        self.overlay.is_some()
    }

    #[cfg(test)]
    pub fn toasts(&self) -> &Toasts {
        &self.toasts
    }

    fn selected_feed(&self) -> Option<&Feed> {
        let id = self.selected_feed.as_deref()?;
        self.feeds.iter().find(|f| f.id == id)
    }

    fn view(&self) -> View {
        View {
            setup_complete: self.setup_complete,
            modal_open: self.overlay.is_some() || self.palette.is_some(),
            feed_selected: self.selected_feed.is_some(),
        }
    }

    fn top_inset(&self) -> f32 {
        if self.custom_titlebar {
            theme::TITLEBAR_HEIGHT
        } else {
            0.0
        }
    }

    // --- Backend events -------------------------------------------------------

    fn handle_events(&mut self, ctx: &egui::Context) {
        for event in self.backend.poll() {
            self.handle_event(ctx, event);
        }
    }

    fn handle_event(&mut self, ctx: &egui::Context, event: Event) {
        match event {
            Event::Ready { setup_complete } => {
                self.loading = false;
                self.setup_complete = setup_complete;
                if setup_complete {
                    self.open_from_env(ctx);
                }
            }
            Event::Feeds(feeds) => {
                self.feeds = feeds;
                if self
                    .selected_feed
                    .as_ref()
                    .is_some_and(|id| !self.feeds.iter().any(|f| &f.id == id))
                {
                    self.select_feed(None);
                }
                if let Some(i) = self.diagnostics.feed.take() {
                    if let Some(id) = self.feeds.get(i).map(|f| f.id.clone()) {
                        self.select_feed(Some(id));
                    }
                }
            }
            Event::Tags(tags) => self.tags = tags,
            Event::Entries { feed_id, entries } => {
                if self.selected_feed.as_deref() == Some(feed_id.as_str()) {
                    self.entries = entries;
                    self.entries_loading = false;
                    if self
                        .selected_entry
                        .as_ref()
                        .is_some_and(|id| !self.entries.iter().any(|e| &e.id == id))
                    {
                        self.selected_entry = None;
                        self.article = None;
                    }
                    if let Some(i) = self.diagnostics.entry.take() {
                        if let Some(id) = self.entries.get(i).map(|e| e.id.clone()) {
                            self.select_entry(id);
                        }
                    }
                }
            }
            Event::Settings(settings) => {
                self.settings = settings;
                self.hydrate_theme(ctx);
            }
            Event::Done { op, result } => self.op_done(op, result),
            Event::FeedsUpdated { feed_ids, .. } => {
                if let Some(id) = self.selected_feed.clone() {
                    if feed_ids.contains(&id) {
                        self.backend.load_entries(&id);
                    }
                }
            }
            Event::Remote { url, branch } => {
                if let Some(Overlay::Sync(s)) = &mut self.overlay {
                    s.loading = false;
                    s.branch = branch;
                    if let Some(url) = url {
                        s.remote_input = url.clone();
                        s.saved_url = url;
                        s.status = None;
                        self.backend.git_status();
                    }
                }
            }
            Event::GitStatus(status) => {
                if let Some(Overlay::Sync(s)) = &mut self.overlay {
                    s.status = Some(status);
                }
            }
            Event::Update(status) => self.update = status,
        }
    }

    /// `hydrateFromSettings`: with nothing saved locally (a fresh install on
    /// a synced data folder), adopt the synced theme and scale.
    fn hydrate_theme(&mut self, ctx: &egui::Context) {
        if self.hydrated || self.theme_saved {
            return;
        }
        self.hydrated = true;
        let before = self.theme.clone();
        if let Some(t) = self.settings.theme.as_deref().and_then(ThemeId::from_id) {
            self.theme.theme = t;
        }
        if let Some(s) = self.settings.scale.as_deref() {
            self.theme.set_scale(s);
        }
        diagnostics::theme_overrides(&mut self.theme);
        if self.theme != before {
            self.theme.apply(ctx);
        }
    }

    fn op_done(&mut self, op: Op, result: Result<(), String>) {
        let failed = |what: &str| {
            result
                .as_ref()
                .err()
                .map(|e| format!("Failed to {what}: {e}"))
        };
        match op {
            Op::Setup => {
                self.setup.busy = false;
                match &result {
                    Ok(()) => {
                        self.setup_complete = true;
                        self.hydrated = false;
                    }
                    Err(e) => self.setup.error = Some(e.clone()),
                }
            }
            Op::AddFeed | Op::EditFeed => {
                let what = if op == Op::AddFeed {
                    "add feed"
                } else {
                    "edit feed"
                };
                if let Some(Overlay::AddFeed(form) | Overlay::EditFeed(form)) = &mut self.overlay {
                    form.busy = false;
                    match &result {
                        Ok(()) => self.overlay = None,
                        Err(e) => form.error = Some(e.clone()),
                    }
                }
                if let Some(msg) = failed(what) {
                    self.toasts.error(msg);
                }
            }
            Op::AddTag | Op::EditTag => {
                let what = if op == Op::AddTag {
                    "add tag"
                } else {
                    "edit tag"
                };
                if let Some(Overlay::Tags(state)) = &mut self.overlay {
                    if let Some(TagSub::Add(form) | TagSub::Edit(form)) = &mut state.sub {
                        form.busy = false;
                        match &result {
                            Ok(()) => state.sub = None,
                            Err(e) => form.error = Some(e.clone()),
                        }
                    }
                }
                if let Some(msg) = failed(what) {
                    self.toasts.error(msg);
                }
            }
            Op::RefreshFeed => {
                self.entries_loading = false;
                if let Some(msg) = failed("refresh feed") {
                    self.toasts.error(msg);
                }
            }
            Op::RemoveFeed | Op::RemoveTag | Op::AssignTag | Op::MarkRead | Op::SetSetting => {
                let what = match op {
                    Op::RemoveFeed => "remove feed",
                    Op::RemoveTag => "remove tag",
                    Op::AssignTag => "update tags",
                    Op::MarkRead => "update entries",
                    _ => "save setting",
                };
                if let Some(msg) = failed(what) {
                    self.toasts.error(msg);
                }
            }
            Op::PollNow | Op::ForcePush => {}
            Op::ForcePull => match &result {
                Ok(()) => self.backend.reload(),
                Err(e) => self.toasts.error(format!("Pull failed: {e}")),
            },
            Op::Connect | Op::Disconnect | Op::Pull | Op::PullRebase | Op::Push | Op::Reset => {
                let Some(Overlay::Sync(s)) = &mut self.overlay else {
                    return;
                };
                if !s.finish(op, &result) || result.is_err() {
                    return;
                }
                match op {
                    Op::Disconnect => {
                        s.saved_url.clear();
                        s.remote_input.clear();
                        s.status = None;
                    }
                    Op::Reset => {
                        // The selected feed may no longer exist after the reset.
                        s.status = None;
                        self.backend.git_status();
                        self.select_feed(None);
                    }
                    _ => {
                        s.status = None;
                        self.backend.git_status();
                    }
                }
            }
        }
    }

    // --- Selection (stores/app.js) ---------------------------------------------

    fn select_feed(&mut self, id: Option<String>) {
        self.selected_entry = None;
        self.article = None;
        self.entries.clear();
        self.entries_loading = id.is_some();
        if let Some(id) = &id {
            self.backend.load_entries(id);
        }
        self.selected_feed = id;
    }

    /// `selectEntry`: open it and mark it read.
    fn select_entry(&mut self, id: String) {
        let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) else {
            return;
        };
        self.article = Some(Article::new(entry));
        if !entry.is_read {
            entry.is_read = true;
            self.backend
                .mark_read(id.clone(), entry.feed_id.clone(), true);
        }
        self.selected_entry = Some(id);
    }

    /// `handleMarkReadShortcut` / `handleMarkUnreadShortcut`: the selected
    /// entry, or the whole feed.
    fn mark(&mut self, read: bool) {
        let Some(feed_id) = self.selected_feed.clone() else {
            return;
        };
        match self.selected_entry.clone() {
            Some(entry_id) => {
                if let Some(e) = self.entries.iter_mut().find(|e| e.id == entry_id) {
                    e.is_read = read;
                }
                self.backend.mark_read(entry_id, feed_id, read);
            }
            None => {
                for e in &mut self.entries {
                    e.is_read = read;
                }
                self.backend.mark_all(feed_id, read);
            }
        }
    }

    // --- Commands ------------------------------------------------------------

    fn apply(&mut self, ctx: &egui::Context, act: Act) {
        match act {
            Act::AddFeed => self.overlay = Some(Overlay::AddFeed(FeedForm::default())),
            Act::EditFeed => {
                if let Some(f) = self.selected_feed() {
                    self.overlay = Some(Overlay::EditFeed(FeedForm {
                        id: Some(f.id.clone()),
                        title: f.title.clone(),
                        url: f.url.clone(),
                        ..Default::default()
                    }));
                }
            }
            Act::RemoveFeed => {
                if let Some(f) = self.selected_feed() {
                    let title = if f.title.is_empty() {
                        "this feed".to_string()
                    } else {
                        f.title.clone()
                    };
                    self.overlay = Some(Overlay::RemoveFeed {
                        id: f.id.clone(),
                        title,
                    });
                }
            }
            Act::RefreshFeed => {
                if let Some(id) = self.selected_feed.clone() {
                    self.entries_loading = true;
                    self.backend.refresh_feed(id);
                }
            }
            Act::MarkRead => self.mark(true),
            Act::MarkUnread => self.mark(false),
            Act::Tags => self.overlay = Some(Overlay::Tags(TagsState::default())),
            Act::Sync => {
                self.overlay = Some(Overlay::Sync(SyncState::new()));
                self.backend.load_remote();
            }
            Act::ToggleToolbar => self.toolbar_visible = !self.toolbar_visible,
            Act::Settings => self.open_settings("ui"),
            Act::About => self.overlay = Some(Overlay::About),
            Act::Help => self.overlay = Some(Overlay::Help),
            Act::ZoomIn | Act::ZoomOut | Act::ZoomReset => {
                let before = self.theme.clone();
                match act {
                    Act::ZoomIn => self.theme.zoom_in(),
                    Act::ZoomOut => self.theme.zoom_out(),
                    _ => self.theme.zoom_reset(),
                }
                self.theme.apply(ctx);
                self.theme_changed(&before);
            }
            Act::Palette => self.palette = Some(CommandPalette::new()),
            Act::PollNow => {
                self.toasts.info("Checking all feeds…");
                self.backend.poll_now();
            }
            Act::SyncNow => {
                self.toasts.info("Syncing…");
                self.backend.force_push();
            }
            Act::PullNow => {
                self.toasts.info("Pulling from remote…");
                self.backend.force_pull();
            }
            Act::OpenFeed(id) => self.select_feed(Some(id)),
        }
    }

    fn open_settings(&mut self, tab: &str) {
        self.sync.log = self.backend.sync_log();
        self.sync.last_log_id = self.sync.log.last().map_or(0, |e| e.id);
        let pref = self.backend.paths().custom_titlebar();
        self.overlay = Some(Overlay::Settings(SettingsState::new(
            tab,
            pref,
            self.custom_titlebar,
        )));
    }

    /// `themeState.set` / `setScale` mirror the choice to the synced settings.
    fn theme_changed(&mut self, before: &ThemeState) {
        self.theme_saved = true;
        if self.theme.theme != before.theme {
            self.settings.theme = Some(self.theme.theme.id().to_string());
            if self.setup_complete {
                self.backend
                    .set_setting("theme", json!(self.theme.theme.id()));
            }
        }
        if self.theme.scale != before.scale {
            self.settings.scale = Some(self.theme.scale.clone());
            if self.setup_complete {
                self.backend.set_setting("scale", json!(self.theme.scale));
            }
        }
    }

    fn open_from_env(&mut self, ctx: &egui::Context) {
        let Some(open) = self.diagnostics.open.take() else {
            return;
        };
        let (what, arg) = open.split_once(':').unwrap_or((open.as_str(), ""));
        match what {
            "add" => self.apply(ctx, Act::AddFeed),
            "tags" => self.apply(ctx, Act::Tags),
            "settings" => self.open_settings(if arg.is_empty() { "ui" } else { arg }),
            "sync" => self.apply(ctx, Act::Sync),
            "about" => self.apply(ctx, Act::About),
            "help" => self.apply(ctx, Act::Help),
            "palette" => self.apply(ctx, Act::Palette),
            _ => {}
        }
    }

    // --- Per-frame work --------------------------------------------------------

    /// `pollSyncStatus`, every 2 s while set up.
    fn tick_sync(&mut self, ctx: &egui::Context) {
        if !self.setup_complete {
            return;
        }
        let now = ctx.input(|i| i.time);
        if now < self.sync.next_poll {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(
                self.sync.next_poll - now,
            ));
            return;
        }
        self.sync.next_poll = now + SYNC_POLL_SECS;
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(SYNC_POLL_SECS));
        let since = (self.sync.last_log_id > 0).then_some(self.sync.last_log_id);
        let status = self.backend.sync_status(since);
        self.sync.status = status["status"].as_str().unwrap_or("idle").to_string();
        self.sync.last_sync_time = status["lastSyncTime"].as_str().map(str::to_string);
        if let Some(entries) = status["logEntries"].as_array() {
            for v in entries {
                if let Some(entry) = log_entry(v) {
                    self.sync.last_log_id = entry.id;
                    self.sync.log.push(entry);
                }
            }
            let excess = self.sync.log.len().saturating_sub(LOG_CAP);
            self.sync.log.drain(..excess);
        }
    }

    /// Commands first, so their keys don't reach other widgets. Nothing fires
    /// while a text field has focus (`isTypingTarget`).
    fn dispatch_keys(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let view = self.view();
        for act in self.registry.dispatch_keys(ctx, &view) {
            self.apply(ctx, act);
        }
    }

    /// Ctrl+wheel steps the UI scale, at most once per cooldown.
    fn wheel_zoom(&mut self, ctx: &egui::Context) {
        let (zoom, now) = ctx.input(|i| (i.zoom_delta(), i.time));
        if zoom == 1.0 || now - self.last_wheel_zoom < ZOOM_COOLDOWN_SECS {
            return;
        }
        self.last_wheel_zoom = now;
        self.apply(
            ctx,
            if zoom > 1.0 {
                Act::ZoomIn
            } else {
                Act::ZoomOut
            },
        );
    }

    /// The whole UI, once per frame.
    pub fn show(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.init(&ctx);
        self.handle_events(&ctx);
        self.tick_sync(&ctx);
        self.dispatch_keys(&ctx);
        self.wheel_zoom(&ctx);

        if self.custom_titlebar {
            self.titlebar(ui);
        }
        let p = theme::palette(&ctx);
        if self.loading {
            egui::CentralPanel::default()
                .frame(Frame::NONE.fill(p.bg_base))
                .show(ui, views::loading);
        } else if !self.setup_complete {
            let actions = egui::CentralPanel::default()
                .frame(Frame::NONE.fill(p.bg_base))
                .show(ui, |ui| views::setup(ui, &mut self.setup))
                .inner;
            for a in actions {
                self.view_action(&ctx, a);
            }
        } else {
            self.main_view(ui);
        }

        self.overlays(&ctx);
        self.palette(&ctx);
        self.toasts.show(&ctx);
        if self.custom_titlebar {
            window::window_edges(&ctx);
        }
        if let Some(shot) = &mut self.screenshot {
            shot.tick(&ctx);
        }
    }

    fn titlebar(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let bar = TitleBar {
            app_name: "ThreadLiner",
            toolbar_visible: self.toolbar_visible,
        };
        let ready = self.setup_complete && !self.loading;
        let feed = self.selected_feed.is_some();
        let mut act = None;
        let actions = titlebar::show(ui, &bar, &mut self.theme.clone(), |ui| {
            if !ready {
                return;
            }
            if titlebar::button(ui, icons::PLUS, "Add Feed", false)
                .on_hover_text("Add Feed (Ctrl+N)")
                .clicked()
            {
                act = Some(Act::AddFeed);
            }
            let refresh = titlebar::button(ui, icons::ARROWS_ROTATE, "Refresh Feed", false)
                .on_hover_text("Refresh Feed (F5)");
            if refresh.clicked() && feed {
                act = Some(Act::RefreshFeed);
            }
            if titlebar::button(ui, icons::TAGS, "Tags", false)
                .on_hover_text("Tags (Ctrl+Shift+T)")
                .clicked()
            {
                act = Some(Act::Tags);
            }
        });
        for a in actions {
            match a {
                TitleBarAction::ToggleToolbar => act = Some(Act::ToggleToolbar),
                TitleBarAction::ScaleChanged(scale) => {
                    let before = self.theme.clone();
                    self.theme.set_scale(&scale);
                    self.theme.apply(&ctx);
                    self.theme_changed(&before);
                }
            }
        }
        if let Some(a) = act {
            self.apply(&ctx, a);
        }
    }

    fn main_view(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let p = theme::palette(&ctx);
        let mut acts = Vec::new();
        if self.toolbar_visible {
            self.toolbar(ui, &mut acts);
        }
        let mut actions = Vec::new();
        let (min, default, max) = views::SIDEBAR_WIDTH;
        let selected = self.selected_feed().cloned();
        egui::Panel::left("threadliner-sidebar")
            .resizable(true)
            .default_size(default)
            .size_range(min..=max)
            .frame(Frame::NONE.fill(p.bg_surface))
            .show(ui, |ui| {
                actions.extend(views::sidebar(
                    ui,
                    &self.feeds,
                    &self.tags,
                    selected.as_ref(),
                ));
            });
        let (min, default, max) = views::ENTRY_LIST_WIDTH;
        egui::Panel::left("threadliner-entries")
            .resizable(true)
            .default_size(default)
            .size_range(min..=max)
            .frame(Frame::NONE.fill(p.bg_surface))
            .show(ui, |ui| {
                actions.extend(views::entry_list(
                    ui,
                    selected.as_ref(),
                    &self.entries,
                    self.entries_loading,
                    self.selected_entry.as_deref(),
                ));
            });
        let entry = self
            .selected_entry
            .as_ref()
            .and_then(|id| self.entries.iter().find(|e| &e.id == id))
            .cloned();
        egui::CentralPanel::default()
            .frame(Frame::NONE.fill(p.bg_base))
            .show(ui, |ui| {
                actions.extend(views::content(ui, entry.as_ref(), self.article.as_mut()));
            });
        for a in acts {
            self.apply(&ctx, a);
        }
        for a in actions {
            self.view_action(&ctx, a);
        }
    }

    /// `Toolbar.svelte`.
    fn toolbar(&mut self, ui: &mut Ui, acts: &mut Vec<Act>) {
        let feed = self.selected_feed.is_some();
        let open = |o: &Option<Overlay>, f: fn(&Overlay) -> bool| o.as_ref().is_some_and(f);
        let tags_open = open(&self.overlay, |o| matches!(o, Overlay::Tags(_)));
        let sync_open = open(&self.overlay, |o| matches!(o, Overlay::Sync(_)));
        let settings_open = open(&self.overlay, |o| matches!(o, Overlay::Settings(_)));
        let about_open = open(&self.overlay, |o| matches!(o, Overlay::About));
        let pending = matches!(self.sync.status.as_str(), "committing" | "waiting");
        let mut top = Vec::new();
        let mut bottom = Vec::new();
        toolbar::show(
            ui,
            |ui| {
                let mut b =
                    |ui: &mut Ui, glyph, title, label, disabled: bool, active: bool, act: Act| {
                        let resp = toolbar::button(
                            ui,
                            ToolbarButton::new(glyph, title)
                                .label(label)
                                .disabled(disabled)
                                .active(active),
                        );
                        if resp.clicked() {
                            top.push(act);
                        }
                    };
                b(
                    ui,
                    icons::PLUS,
                    "Add Feed (Ctrl+N)",
                    "Add Feed",
                    false,
                    false,
                    Act::AddFeed,
                );
                b(
                    ui,
                    icons::PEN,
                    "Edit Feed (Ctrl+E)",
                    "Edit Feed",
                    !feed,
                    false,
                    Act::EditFeed,
                );
                b(
                    ui,
                    icons::TRASH,
                    "Remove Feed (Ctrl+D)",
                    "Remove Feed",
                    !feed,
                    false,
                    Act::RemoveFeed,
                );
                b(
                    ui,
                    icons::ARROWS_ROTATE,
                    "Refresh Feed (F5)",
                    "Refresh Feed",
                    !feed,
                    false,
                    Act::RefreshFeed,
                );
                toolbar::divider(ui);
                b(
                    ui,
                    icons::CHECK_DOUBLE,
                    "Mark as Read (Ctrl+M)",
                    "Mark as Read",
                    !feed,
                    false,
                    Act::MarkRead,
                );
                b(
                    ui,
                    icons::ROTATE_LEFT,
                    "Mark as Unread (Ctrl+Shift+M)",
                    "Mark as Unread",
                    !feed,
                    false,
                    Act::MarkUnread,
                );
                toolbar::divider(ui);
                b(
                    ui,
                    icons::TAGS,
                    "Tags (Ctrl+Shift+T)",
                    "Tags",
                    false,
                    tags_open,
                    Act::Tags,
                );
            },
            |ui| {
                let sync = toolbar::button(
                    ui,
                    ToolbarButton::new(icons::CLOUD_ARROW_UP, "Remote Sync (Ctrl+Shift+S)")
                        .label("Sync")
                        .active(sync_open)
                        .attention(pending && !sync_open),
                );
                if sync.clicked() {
                    bottom.push(Act::Sync);
                }
                toolbar::divider(ui);
                let mut b = |ui: &mut Ui, glyph, title, label, active: bool, act: Act| {
                    if toolbar::button(
                        ui,
                        ToolbarButton::new(glyph, title).label(label).active(active),
                    )
                    .clicked()
                    {
                        bottom.push(act);
                    }
                };
                b(
                    ui,
                    icons::GEAR,
                    "Settings (Ctrl+,)",
                    "Settings",
                    settings_open,
                    Act::Settings,
                );
                b(
                    ui,
                    icons::CIRCLE_INFO,
                    "About (Ctrl+Shift+I)",
                    "About",
                    about_open,
                    Act::About,
                );
                b(
                    ui,
                    icons::CIRCLE_QUESTION,
                    "Help (F1)",
                    "Help",
                    false,
                    Act::Help,
                );
            },
        );
        acts.extend(top);
        acts.extend(bottom);
    }

    fn view_action(&mut self, ctx: &egui::Context, action: ViewAction) {
        match action {
            ViewAction::SelectFeed(id) => self.select_feed(Some(id)),
            ViewAction::SelectEntry(id) => self.select_entry(id),
            ViewAction::ToggleTag(tag_id) => {
                if let Some(feed) = self.selected_feed() {
                    let assigned = feed.tag_ids.contains(&tag_id);
                    let feed_id = feed.id.clone();
                    self.backend.assign_tag(feed_id, tag_id, !assigned);
                }
            }
            ViewAction::OpenUrl(url) => ctx.open_url(egui::OpenUrl::new_tab(url)),
            ViewAction::Browse => {
                let picked = rfd::FileDialog::new()
                    .set_title("Select Data Folder")
                    .pick_folder();
                if let Some(dir) = picked {
                    self.setup.data_dir = dir.display().to_string();
                }
            }
            ViewAction::Setup => {
                let dir = self.setup.data_dir.trim().to_string();
                if dir.is_empty() {
                    self.setup.error = Some("Please select a data folder.".into());
                    return;
                }
                let remote =
                    Some(self.setup.remote_url.trim().to_string()).filter(|r| !r.is_empty());
                self.setup.busy = true;
                self.setup.error = None;
                self.backend.setup(dir.into(), remote);
            }
        }
    }

    // --- Dialogs ---------------------------------------------------------------

    fn overlays(&mut self, ctx: &egui::Context) {
        let Some(mut overlay) = self.overlay.take() else {
            return;
        };
        let top_inset = self.top_inset();
        let mut keep = true;
        match &mut overlay {
            Overlay::AddFeed(form) | Overlay::EditFeed(form) => {
                match overlays::feed_form(ctx, form) {
                    Some(FormAction::Close) => keep = form.busy,
                    Some(FormAction::Submit) => {
                        form.busy = true;
                        form.error = None;
                        let url = form.url.trim().to_string();
                        match form.id.clone() {
                            None => self.backend.add_feed(url),
                            Some(id) => {
                                self.backend
                                    .edit_feed(id, form.title.trim().to_string(), url)
                            }
                        }
                    }
                    None => {}
                }
            }
            Overlay::RemoveFeed { id, title } => match overlays::remove_feed(ctx, title) {
                Some(Confirm::Yes) => {
                    keep = false;
                    if self.selected_feed.as_deref() == Some(id.as_str()) {
                        self.select_feed(None);
                    }
                    self.feeds.retain(|f| &f.id != id);
                    self.backend.remove_feed(id.clone());
                }
                Some(Confirm::No) => keep = false,
                None => {}
            },
            Overlay::Tags(state) => match overlays::tags(ctx, state, &self.tags) {
                Some(TagsAction::Close) => keep = false,
                Some(TagsAction::Add(name)) => {
                    if let Some(TagSub::Add(f)) = &mut state.sub {
                        f.busy = true;
                        f.error = None;
                    }
                    self.backend.add_tag(name);
                }
                Some(TagsAction::Edit { id, name }) => {
                    if let Some(TagSub::Edit(f)) = &mut state.sub {
                        f.busy = true;
                        f.error = None;
                    }
                    self.backend.edit_tag(id, name);
                }
                Some(TagsAction::Delete(id)) => {
                    self.tags.retain(|t| t.id != id);
                    self.backend.remove_tag(id);
                }
                None => {}
            },
            Overlay::Settings(state) => {
                let sync = SyncView {
                    last_sync_time: self.sync.last_sync_time.as_deref(),
                    log: &self.sync.log,
                };
                let before = self.theme.clone();
                let actions = crate::overlays::settings::show(
                    ctx,
                    state,
                    &mut self.theme,
                    &self.settings,
                    sync,
                    top_inset,
                );
                for a in actions {
                    match a {
                        SettingsAction::Close => keep = false,
                        SettingsAction::ThemeChanged => self.theme_changed(&before),
                        SettingsAction::SetSetting(key, value) => {
                            self.local_setting(key, &value);
                            self.backend.set_setting(key, value);
                        }
                        SettingsAction::SetCustomTitlebar(on) => {
                            if let Err(e) = self.backend.paths().set_custom_titlebar(on) {
                                self.toasts.error(format!("Failed to save preference: {e}"));
                            }
                        }
                        SettingsAction::Restart => restart(ctx),
                    }
                }
            }
            Overlay::Sync(state) => match crate::overlays::sync::show(ctx, state, top_inset) {
                Some(SyncAction::Close) => keep = false,
                Some(SyncAction::RefreshStatus) => {
                    state.status = None;
                    self.backend.git_status();
                }
                Some(SyncAction::Connect(url)) => {
                    state.start(Op::Connect, "Connecting...");
                    state.saved_url = url.clone();
                    self.backend.connect(url);
                }
                Some(SyncAction::Disconnect) => {
                    state.start(Op::Disconnect, "Disconnecting...");
                    self.backend.disconnect();
                }
                Some(SyncAction::Pull) => {
                    state.start(Op::Pull, "Pulling...");
                    self.backend.pull();
                }
                Some(SyncAction::PullRebase) => {
                    state.start(Op::PullRebase, "Pulling & rebasing...");
                    self.backend.pull_rebase();
                }
                Some(SyncAction::Push) => {
                    state.start(Op::Push, "Pushing...");
                    self.backend.push();
                }
                Some(SyncAction::Reset) => {
                    state.start(Op::Reset, "Resetting...");
                    self.backend.reset_to_remote();
                }
                None => {}
            },
            Overlay::Help => keep = !crate::overlays::help::show(ctx, top_inset),
            Overlay::About => {
                let modal = AboutModal::new("ThreadLiner", env!("CARGO_PKG_VERSION"))
                    .description(DESCRIPTION)
                    .repo(REPO_URL, Some("github.com/NathanLaan/marina"))
                    .icon(AboutIcon::Glyph(icons::RSS))
                    .update(&self.update);
                match modal.show(ctx) {
                    Some(AboutAction::Close) => keep = false,
                    Some(AboutAction::CheckForUpdates) => {
                        self.update = UpdateStatus::Checking;
                        self.backend.check_updates();
                    }
                    Some(AboutAction::OpenUrl(url)) => ctx.open_url(egui::OpenUrl::new_tab(url)),
                    Some(AboutAction::Download | AboutAction::Install) | None => {}
                }
            }
        }
        if keep && self.overlay.is_none() {
            self.overlay = Some(overlay);
        }
    }

    /// Settings shows the value at once, before the backend confirms it.
    fn local_setting(&mut self, key: &str, value: &Value) {
        let text = || value.as_str().unwrap_or_default().to_string();
        match key {
            "pollInterval" => self.settings.poll_interval = text(),
            "syncWaitTime" => self.settings.sync_wait_time = text(),
            "pollNotificationsEnabled" => {
                self.settings.notifications = value.as_bool().unwrap_or(true)
            }
            _ => {}
        }
    }

    /// The command palette, with the feeds as extra results.
    fn palette(&mut self, ctx: &egui::Context) {
        let Some(mut palette) = self.palette.take() else {
            return;
        };
        let view = View {
            modal_open: false,
            ..self.view()
        };
        let feeds = &self.feeds;
        let mut extra = |q: &str| -> Vec<PaletteItem<Act>> {
            if q.is_empty() {
                return Vec::new();
            }
            feeds
                .iter()
                .filter_map(|f| {
                    let score = marina_egui::commands::fuzzy_score(&f.title, q);
                    (score > 0).then(|| PaletteItem {
                        kind: ItemKind::Other,
                        id: format!("feed:{}", f.id),
                        label: f.title.clone(),
                        sub: if f.unread_count > 0 {
                            format!("{} unread", f.unread_count)
                        } else {
                            String::new()
                        },
                        section: "Feeds".into(),
                        score,
                        action: Act::OpenFeed(f.id.clone()),
                    })
                })
                .collect()
        };
        match palette.show(ctx, &mut self.registry, &view, &mut extra) {
            PaletteOutcome::Open => self.palette = Some(palette),
            PaletteOutcome::Closed => {}
            PaletteOutcome::Run(act) => self.apply(ctx, act),
        }
    }

    pub fn save_state(&self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, THEME_KEY, &self.theme);
        eframe::set_value(storage, RECENTS_KEY, &self.registry.recents());
    }
}

impl eframe::App for ThreadLinerApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.save_state(storage);
    }
}

/// A sync log entry from `sync:getStatus`'s `logEntries`.
fn log_entry(v: &Value) -> Option<LogEntry> {
    let level = match v["level"].as_str()? {
        "error" => "error",
        _ => "info",
    };
    Some(LogEntry {
        id: v["id"].as_u64()?,
        timestamp: v["timestamp"].as_i64().unwrap_or(0),
        level,
        message: v["message"].as_str().unwrap_or("").to_string(),
        detail: v["detail"].as_str().map(str::to_string),
    })
}

/// `app.relaunch()`: start a new copy with the same arguments, then close.
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
