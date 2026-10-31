//! The app: owns the backend, the library, the open reader and the open
//! dialog; routes backend events, commands, view actions and reader actions.
//! The equivalent of `App.svelte` plus `stores/library.svelte.js`.

use std::collections::HashMap;
use std::path::PathBuf;

use egui::{Frame, Ui};
use laankit_egui::about::{AboutAction, AboutIcon, AboutModal, UpdateStatus};
use laankit_egui::commands::{Recents, Registry};
use laankit_egui::diagnostics::{self, Screenshot};
use laankit_egui::dialog::Confirm;
use laankit_egui::file_dialog::FileDialogs;
use laankit_egui::palette::{CommandPalette, ItemKind, PaletteItem, PaletteOutcome};
use laankit_egui::panes::PaneLayout;
use laankit_egui::theme::{self, ThemeState};
use laankit_egui::titlebar::{self, TitleBar, TitleBarAction};
use laankit_egui::toast::Toasts;
use laankit_egui::{fonts, icons, window};
use serde_json::json;

use crate::backend::{Backend, Event, GitOp, OpenedBook};
use crate::commands::{self, Act, View};
use crate::config::Paths;
use crate::model::{Book, Counts, FormatFilter, SortMode};
use crate::overlays::settings::{SettingsAction, SettingsState};
use crate::overlays::sync::{SyncAction, SyncState};
use crate::overlays::{self, Overlay};
use crate::reader::epub::EpubReader;
use crate::reader::pdf::PdfReader;
use crate::reader::{Reader, ReaderAction};
use crate::views::{self, Cover, GridProps, ViewAction};

/// eframe storage keys (the Svelte build's `pageliner-theme` /
/// `pageliner-scale` localStorage keys, the palette's recents and the reader
/// sidebar's pane layout).
const THEME_KEY: &str = "pageliner-theme";
const RECENTS_KEY: &str = "pageliner-recents";
const PANES_KEY: &str = "pageliner-reader-panes";
/// Ctrl+wheel zoom steps at most once per 200 ms (`ZOOM_COOLDOWN_MS`).
const ZOOM_COOLDOWN_SECS: f64 = 0.2;
const DESCRIPTION: &str = "A desktop e-reader and document library built on the laankit framework.";
const REPO_URL: &str = "https://github.com/NathanLaan/laankit";

/// `LAANKIT_OPEN` / `LAANKIT_BOOK`: what to open once the library is in, for
/// screenshots.
#[derive(Default)]
struct Diagnostics {
    open: Option<String>,
    /// The nth book in the grid's order, or a book id.
    book: Option<String>,
}

pub struct PageLinerApp {
    backend: Backend,
    theme: ThemeState,
    /// Whether the window was built undecorated, with LaanKit's title bar.
    custom_titlebar: bool,
    sidebar_visible: bool,
    status_bar_visible: bool,
    initialized: bool,
    loading: bool,
    books: Vec<Book>,
    sort: SortMode,
    filter: FormatFilter,
    /// The open book (`selectedId`).
    selected: Option<String>,
    reader: Option<Reader>,
    /// Waiting for the backend to read the selected book.
    opening: bool,
    reading_status: Option<String>,
    covers: HashMap<String, Cover>,
    panes: PaneLayout,
    overlay: Option<Overlay>,
    palette: Option<CommandPalette>,
    registry: Registry<Act, View>,
    toasts: Toasts,
    /// The Import dialog, off the UI thread where it would freeze it.
    imports: FileDialogs<Vec<PathBuf>>,
    update: UpdateStatus,
    last_wheel_zoom: f64,
    screenshot: Option<Screenshot>,
    diagnostics: Diagnostics,
}

impl PageLinerApp {
    /// `storage` is eframe's (theme, scale, palette recents, pane layout);
    /// `None` in tests.
    pub fn new(storage: Option<&dyn eframe::Storage>, paths: Paths) -> Self {
        let prefs = paths.prefs();
        let mut theme: ThemeState = storage
            .and_then(|s| eframe::get_value(s, THEME_KEY))
            .unwrap_or_default();
        diagnostics::theme_overrides(&mut theme);
        let mut registry = commands::registry();
        if let Some(recents) = storage.and_then(|s| eframe::get_value::<Recents>(s, RECENTS_KEY)) {
            registry.load_recents(recents);
        }
        let panes = storage
            .and_then(|s| eframe::get_value(s, PANES_KEY))
            .unwrap_or_default();
        let env = |k: &str| std::env::var(k).ok();
        Self {
            backend: Backend::new(paths),
            theme,
            custom_titlebar: prefs.custom_titlebar,
            sidebar_visible: prefs.sidebar_visible,
            status_bar_visible: prefs.status_bar_visible,
            initialized: false,
            loading: true,
            books: Vec::new(),
            sort: SortMode::default(),
            filter: FormatFilter::default(),
            selected: None,
            reader: None,
            opening: false,
            reading_status: None,
            covers: HashMap::new(),
            panes,
            overlay: None,
            palette: None,
            registry,
            toasts: Toasts::default(),
            imports: FileDialogs::new(),
            update: UpdateStatus::Idle,
            last_wheel_zoom: f64::NEG_INFINITY,
            screenshot: Screenshot::from_env(),
            diagnostics: Diagnostics {
                open: env("LAANKIT_OPEN"),
                book: env("LAANKIT_BOOK"),
            },
        }
    }

    /// Fonts and the theme, once per context.
    fn init(&mut self, ctx: &egui::Context) {
        if self.initialized {
            return;
        }
        self.initialized = true;
        fonts::install(ctx);
        egui_extras::install_image_loaders(ctx);
        self.theme.apply(ctx);
        self.backend.attach(ctx);
        self.imports.attach(ctx);
    }

    // --- Accessors for tests ------------------------------------------------

    #[cfg(test)]
    pub fn books(&self) -> &[Book] {
        &self.books
    }

    #[cfg(test)]
    pub fn is_ready(&self) -> bool {
        !self.loading
    }

    #[cfg(test)]
    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    #[cfg(test)]
    pub fn pdf_reader(&mut self) -> Option<&mut PdfReader> {
        match self.reader.as_mut()? {
            Reader::Pdf(r) => Some(r),
            _ => None,
        }
    }

    #[cfg(test)]
    pub fn import_paths(&mut self, paths: Vec<PathBuf>) {
        self.backend.import(paths);
    }

    #[cfg(test)]
    pub fn has_overlay(&self) -> bool {
        self.overlay.is_some()
    }

    fn view(&self) -> View {
        View {
            book_open: self.selected.is_some(),
            modal_open: self.overlay.is_some() || self.palette.is_some(),
        }
    }

    fn top_inset(&self) -> f32 {
        if self.custom_titlebar {
            theme::TITLEBAR_HEIGHT
        } else {
            0.0
        }
    }

    fn selected_book(&self) -> Option<&Book> {
        let id = self.selected.as_deref()?;
        self.books.iter().find(|b| b.id == id)
    }

    // --- Backend events -------------------------------------------------------

    fn handle_events(&mut self, ctx: &egui::Context) {
        for event in self.backend.poll() {
            self.handle_event(ctx, event);
        }
    }

    fn handle_event(&mut self, ctx: &egui::Context, event: Event) {
        match event {
            Event::Books(books) => {
                self.books = books;
                self.loading = false;
                if let Some(id) = self.selected.clone() {
                    if !self.books.iter().any(|b| b.id == id) {
                        self.close_book();
                    }
                }
                self.open_from_env(ctx);
            }
            Event::Imported(added) => {
                let first = added.first().map(|b| b.id.clone());
                let n = added.len();
                self.books.extend(added);
                if n > 1 {
                    self.toasts.success(format!("Imported {n} books"));
                }
                // `importBooks` jumps to the first new book.
                if let Some(id) = first {
                    self.open_book(id);
                }
            }
            Event::ImportFailed(names) => {
                self.toasts
                    .error(format!("Couldn't import {}", names.join(", ")));
            }
            Event::Deleted(id) => {
                self.books.retain(|b| b.id != id);
                self.covers.remove(&id);
                if self.selected.as_deref() == Some(id.as_str()) {
                    self.reader = None;
                    self.selected = None;
                    self.reading_status = None;
                }
            }
            Event::Cover { id, image } => {
                let cover = match image {
                    Some(img) => Cover::Ready(ctx.load_texture(
                        format!("cover-{id}"),
                        img,
                        Default::default(),
                    )),
                    None => Cover::Missing,
                };
                self.covers.insert(id, cover);
            }
            Event::Opened(opened) => self.opened(ctx, opened),
            Event::OpenFailed { id, message } => {
                if self.selected.as_deref() == Some(id.as_str()) {
                    self.opening = false;
                    self.selected = None;
                    self.toasts
                        .error(format!("Couldn't open the book: {message}"));
                }
            }
            Event::Error(message) => self.toasts.error(message),
            Event::GitInfo(info) => {
                if let Some(Overlay::Sync(s)) = &mut self.overlay {
                    s.set_info(info);
                }
            }
            Event::GitDone { op, result } => {
                if let Some(Overlay::Sync(s)) = &mut self.overlay {
                    s.finish(op, result);
                } else if let Err(e) = result {
                    self.toasts.error(e);
                }
            }
            Event::Update(status) => self.update = status,
        }
    }

    fn opened(&mut self, ctx: &egui::Context, opened: OpenedBook) {
        if self.selected.as_deref() != Some(opened.id.as_str()) {
            return;
        }
        self.opening = false;
        let Some(book) = self.selected_book().cloned() else {
            return;
        };
        let reader = match (book.format.as_str(), opened.pdf) {
            ("pdf", Some(info)) => Reader::Pdf(Box::new(PdfReader::new(
                ctx,
                &book,
                opened.bytes,
                info,
                &opened.state,
            ))),
            ("epub", _) => Reader::Epub(Box::new(EpubReader::new(
                &book,
                opened.path,
                opened.bytes,
                opened.state,
                &self.backend.paths().read_prefs_map(),
            ))),
            (format, _) => Reader::Unsupported {
                title: book.title.clone(),
                format: format.to_string(),
            },
        };
        self.reader = Some(reader);
    }

    // --- Library (stores/library.svelte.js) -------------------------------------

    /// `select(id)`: open the book in its reader.
    fn open_book(&mut self, id: String) {
        if self.selected.as_deref() == Some(id.as_str()) {
            return;
        }
        self.close_book();
        self.selected = Some(id.clone());
        self.opening = true;
        self.backend.open(id);
    }

    /// `clearSelection`: back to the library, saving the reader's position.
    fn close_book(&mut self) {
        if let (Some(id), Some(reader)) = (self.selected.clone(), self.reader.as_mut()) {
            if let Some(patch) = reader.flush() {
                self.backend.set_state(id, patch);
            }
        }
        self.reader = None;
        self.selected = None;
        self.opening = false;
        self.reading_status = None;
    }

    /// `importBooks`: the file dialog.
    fn import_dialog(&mut self) {
        self.imports.start(|| {
            rfd::FileDialog::new()
                .set_title("Import Books")
                .add_filter("E-Books & Documents", &["epub", "pdf"])
                .add_filter("EPUB", &["epub"])
                .add_filter("PDF", &["pdf"])
                .pick_files()
                .filter(|p| !p.is_empty())
        });
    }

    /// Files dropped on the window are imported (a native addition).
    fn dropped_files(&mut self, ctx: &egui::Context) {
        let (dropped, hovering) = ctx.input(|i| {
            let paths: Vec<PathBuf> = i
                .raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .filter(|p| !p.as_os_str().is_empty())
                .collect();
            (paths, !i.raw.hovered_files.is_empty())
        });
        if !dropped.is_empty() {
            self.backend.import(dropped);
        }
        if hovering {
            views::drop_overlay(ctx);
        }
    }

    // --- Commands ------------------------------------------------------------

    fn apply(&mut self, ctx: &egui::Context, act: Act) {
        let zoom = matches!(act, Act::ZoomIn | Act::ZoomOut | Act::ZoomReset);
        match act {
            Act::Import => self.import_dialog(),
            Act::CloseBook => self.close_book(),
            Act::ToggleSidebar => {
                self.sidebar_visible = !self.sidebar_visible;
                self.backend
                    .set_prefs(json!({ "sidebarVisible": self.sidebar_visible }));
            }
            Act::ToggleStatusBar => {
                self.status_bar_visible = !self.status_bar_visible;
                self.backend
                    .set_prefs(json!({ "statusBarVisible": self.status_bar_visible }));
            }
            Act::Sync => {
                self.overlay = Some(Overlay::Sync(SyncState::default()));
                self.backend.git_info();
            }
            Act::Settings => self.open_settings("ui"),
            Act::About => self.overlay = Some(Overlay::About),
            Act::Palette => self.palette = Some(CommandPalette::new()),
            Act::ZoomIn => self.theme.zoom_in(),
            Act::ZoomOut => self.theme.zoom_out(),
            Act::ZoomReset => self.theme.zoom_reset(),
            Act::OpenBook(id) => self.open_book(id),
        }
        if zoom {
            self.theme.apply(ctx);
        }
    }

    fn open_settings(&mut self, tab: &str) {
        let pref = self.backend.paths().prefs().custom_titlebar;
        self.overlay = Some(Overlay::Settings(SettingsState::new(
            tab,
            pref,
            self.custom_titlebar,
        )));
    }

    fn open_from_env(&mut self, ctx: &egui::Context) {
        if let Some(book) = self.diagnostics.book.take() {
            let visible = crate::model::visible_books(&self.books, self.sort, self.filter);
            let id = match book.parse::<usize>() {
                Ok(i) => visible.get(i).map(|b| b.id.clone()),
                Err(_) => Some(book),
            };
            if let Some(id) = id {
                self.open_book(id);
            }
        }
        let Some(open) = self.diagnostics.open.take() else {
            return;
        };
        let (what, arg) = open.split_once(':').unwrap_or((open.as_str(), ""));
        match what {
            "settings" => self.open_settings(if arg.is_empty() { "ui" } else { arg }),
            "sync" => self.apply(ctx, Act::Sync),
            "about" => self.apply(ctx, Act::About),
            "palette" => self.apply(ctx, Act::Palette),
            "remove" => {
                if let Some(b) = self.books.first() {
                    self.overlay = Some(Overlay::RemoveBook {
                        id: b.id.clone(),
                        title: b.title.clone(),
                    });
                }
            }
            _ => {}
        }
    }

    // --- Per-frame work --------------------------------------------------------

    /// Commands first, so their keys don't reach other widgets. Nothing fires
    /// while a text field has focus.
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
        if let Some(paths) = self.imports.poll() {
            self.backend.import(paths);
        }
        self.dispatch_keys(&ctx);
        self.wheel_zoom(&ctx);

        if self.custom_titlebar {
            self.titlebar(ui);
        }
        self.main_view(ui);
        self.overlays(&ctx);
        self.palette(&ctx);
        self.dropped_files(&ctx);
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
            app_name: "PageLiner",
            toolbar_visible: self.sidebar_visible,
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
                TitleBarAction::ToggleToolbar => act = Some(Act::ToggleSidebar),
                // Already applied by the title bar; saved with the theme.
                TitleBarAction::ScaleChanged(_) => {}
            }
        }
        if let Some(a) = act {
            self.apply(&ctx, a);
        }
    }

    fn main_view(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let p = theme::palette(&ctx);
        let mut actions = Vec::new();
        if self.status_bar_visible {
            let book = self.selected_book().cloned();
            let status = self.reading_status.clone();
            egui::Panel::bottom("pageliner-status")
                .exact_size(views::STATUS_BAR_HEIGHT)
                .resizable(false)
                .show_separator_line(false)
                .frame(Frame::NONE)
                .show(ui, |ui| {
                    views::status_bar(
                        ui,
                        book.as_ref(),
                        self.books.len(),
                        status.as_deref(),
                        &self.theme.scale,
                    );
                });
        }
        if self.sidebar_visible {
            let counts = Counts::of(&self.books);
            egui::Panel::left("pageliner-sidebar")
                .exact_size(views::SIDEBAR_WIDTH)
                .resizable(false)
                .frame(Frame::NONE.fill(p.bg_surface))
                .show(ui, |ui| {
                    actions.extend(views::sidebar(ui, counts, self.filter));
                });
        }
        let keys = self.overlay.is_none() && self.palette.is_none();
        let mut reader_actions = Vec::new();
        egui::CentralPanel::default()
            .frame(Frame::NONE.fill(p.bg_base))
            .show(ui, |ui| {
                if let Some(reader) = &mut self.reader {
                    reader_actions = reader.show(ui, &mut self.panes, keys);
                } else if self.opening {
                    ui.vertical_centered(|ui| {
                        ui.add_space(40.0);
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(egui::RichText::new("Opening…").color(p.text_muted));
                        });
                    });
                } else {
                    let props = GridProps {
                        books: &self.books,
                        sort: self.sort,
                        filter: self.filter,
                        loading: self.loading,
                        selected: self.selected.as_deref(),
                        covers: &self.covers,
                    };
                    actions.extend(views::library(ui, &props));
                }
            });
        for a in reader_actions {
            self.reader_action(a);
        }
        for a in actions {
            self.view_action(&ctx, a);
        }
    }

    fn reader_action(&mut self, action: ReaderAction) {
        match action {
            ReaderAction::Close => self.close_book(),
            ReaderAction::SaveState(patch) => {
                if let Some(id) = self.selected.clone() {
                    self.backend.set_state(id, patch);
                }
            }
            ReaderAction::Status(status) => self.reading_status = status,
            ReaderAction::SavePrefs(patch) => self.backend.set_prefs(patch),
        }
    }

    fn view_action(&mut self, ctx: &egui::Context, action: ViewAction) {
        match action {
            // Clicking a Library section leaves reader mode.
            ViewAction::Filter(f) => {
                self.filter = f;
                self.close_book();
            }
            ViewAction::Sort(s) => self.sort = s,
            ViewAction::Open(id) => self.open_book(id),
            ViewAction::Delete(id) => {
                let title = self
                    .books
                    .iter()
                    .find(|b| b.id == id)
                    .map_or_else(|| "this book".to_string(), |b| b.title.clone());
                self.overlay = Some(Overlay::RemoveBook { id, title });
            }
            ViewAction::WantCover(id) => {
                self.covers.insert(id.clone(), Cover::Loading);
                self.backend.load_cover(id);
            }
            ViewAction::Act(act) => self.apply(ctx, act),
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
            Overlay::RemoveBook { id, title } => match overlays::remove_book(ctx, title) {
                Some(Confirm::Yes) => {
                    keep = false;
                    self.backend.delete(id.clone());
                }
                Some(Confirm::No) => keep = false,
                None => {}
            },
            Overlay::Settings(state) => {
                for a in crate::overlays::settings::show(ctx, state, &mut self.theme, top_inset) {
                    match a {
                        SettingsAction::Close => keep = false,
                        // Applied already; eframe saves it with the theme.
                        SettingsAction::ThemeChanged => {}
                        SettingsAction::SetCustomTitlebar(on) => {
                            self.backend.set_prefs(json!({ "customTitlebar": on }));
                        }
                        SettingsAction::Restart => restart(ctx),
                    }
                }
            }
            Overlay::Sync(state) => match crate::overlays::sync::show(ctx, state) {
                Some(SyncAction::Close) => keep = false,
                Some(SyncAction::SetEnabled(on)) => {
                    state.start(if on { GitOp::Enable } else { GitOp::Disable });
                    self.backend.set_sync_enabled(on);
                }
                Some(SyncAction::SaveRemote(url)) => {
                    state.start(GitOp::SetRemote);
                    self.backend.set_remote(url);
                }
                Some(SyncAction::SyncNow) => {
                    state.start(GitOp::SyncNow);
                    self.backend.sync_now();
                }
                Some(SyncAction::CheckStatus) => {
                    state.start(GitOp::Status);
                    self.backend.git_status();
                }
                None => {}
            },
            Overlay::About => {
                let modal = AboutModal::new("PageLiner", env!("CARGO_PKG_VERSION"))
                    .description(DESCRIPTION)
                    .repo(REPO_URL, Some("github.com/NathanLaan/laankit"))
                    .icon(AboutIcon::Glyph(icons::BOOK))
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

    /// The command palette, with the books as extra results.
    fn palette(&mut self, ctx: &egui::Context) {
        let Some(mut palette) = self.palette.take() else {
            return;
        };
        let view = View {
            modal_open: false,
            ..self.view()
        };
        let books = &self.books;
        let mut extra = |q: &str| -> Vec<PaletteItem<Act>> {
            if q.is_empty() {
                return Vec::new();
            }
            books
                .iter()
                .filter_map(|b| {
                    let score = laankit_egui::commands::fuzzy_score(&b.title, q);
                    (score > 0).then(|| PaletteItem {
                        kind: ItemKind::Other,
                        id: format!("book:{}", b.id),
                        label: b.title.clone(),
                        sub: b.author.clone().unwrap_or_default(),
                        section: "Books".into(),
                        score,
                        action: Act::OpenBook(b.id.clone()),
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

    pub fn save_state(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, THEME_KEY, &self.theme);
        eframe::set_value(storage, RECENTS_KEY, &self.registry.recents());
        eframe::set_value(storage, PANES_KEY, &self.panes);
    }
}

impl eframe::App for PageLinerApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.save_state(storage);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Write the reader's last position before the backend goes away.
        if let (Some(id), Some(reader)) = (self.selected.clone(), self.reader.as_mut()) {
            if let Some(patch) = reader.flush() {
                if let Err(e) = self.backend.set_state_now(&id, &patch) {
                    log::warn!("couldn't save the reading position: {e}");
                }
            }
        }
    }
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
