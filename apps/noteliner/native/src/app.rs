//! The app: owns the backend, the open project, the editor, the panels and
//! the open dialog; routes backend events, commands and view actions. The
//! equivalent of `App.svelte` plus `stores/project.svelte.js` and
//! `stores/log.svelte.js`.

use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;

use egui::{Frame, Ui};
use marina_core::git::SyncStatus;
use marina_editor::attachment_markdown;
use marina_egui::about::{AboutAction, AboutIcon, AboutModal, UpdateStatus};
use marina_egui::commands::{Recents, Registry};
use marina_egui::diagnostics::{self, Screenshot};
use marina_egui::dialog::Confirm;
use marina_egui::palette::{CommandPalette, ItemKind, PaletteItem, PaletteOutcome};
use marina_egui::theme::{self, ThemeState};
use marina_egui::titlebar::{self, TitleBar, TitleBarAction};
use marina_egui::toast::Toasts;
use marina_egui::toolbar::{self, ToolbarButton};
use marina_egui::{fonts, icons, window};
use noteliner_slides::{
    normalize_presentation, Direction, PresentationSettings, PresenterAction, SettingsOutcome,
    SlideEdit,
};
use serde_json::{json, Value};

use crate::backend::{self, Backend, Event, ExportKind, McpConfirm, Op, Outcome};
use crate::commands::{self, Act, View};
use crate::config::{LastFilePosition, Layout, Paths, Prefs, Recent};
use crate::deck::{self, DeckState, SlideImages};
use crate::editor::{EditorAction, NoteEditor};
use crate::model::{self, Entry, SortMode};
use crate::overlays::settings::{SettingsAction, SettingsState};
use crate::overlays::sync::{SyncAction, SyncState};
use crate::overlays::{
    self, FormAction, MoveForm, NewFileForm, NewProjectForm, Overlay, ProjectSettingsAction,
    ProjectSettingsForm, SetupAction, SetupForm,
};
use crate::spell;
use crate::views::attachments::{self, AttachmentAction};
use crate::views::history::{self, HistoryAction, HistoryState};
use crate::views::panes::{PaneAction, PaneStates};
use crate::views::preview::{DeckProps, Preview, PreviewAction};
use crate::views::sidebar::{self, SidebarAction, SidebarProps};
use crate::views::tree::{ContextAction, TreeAction, TreeState};
use crate::views::{self, LogLine, OpenAction, StatusProps};

/// eframe storage keys (the Svelte build's `noteliner-theme` /
/// `noteliner-scale` localStorage keys).
const THEME_KEY: &str = "noteliner-theme";
/// Ctrl+wheel zoom steps at most once per 200 ms (`ZOOM_COOLDOWN_MS`).
const ZOOM_COOLDOWN_SECS: f64 = 0.2;
/// Window bounds are saved after a second of quiet (`bounds_saver`).
const BOUNDS_DEBOUNCE_SECS: f64 = 1.0;
/// eframe's own window-geometry key, which NoteLiner keeps blank (see `save`).
const EFRAME_WINDOW_KEY: &str = "window";
const DESCRIPTION: &str = "An outliner-style note-taking application built with Rust and egui.";
const REPO_URL: &str = "https://github.com/NathanLaan/marina/";

/// `MARINA_OPEN` / `MARINA_PROJECT` / `MARINA_FILE`: what to open at
/// launch, for screenshots and tests.
#[derive(Default)]
struct Diagnostics {
    open: Option<String>,
    project: Option<PathBuf>,
    /// The nth note in display order, or a note name.
    file: Option<String>,
}

/// The open project (`projectState` while `isOpen`).
struct Project {
    folder: PathBuf,
    /// `noteliner.json` as the backend gave it; edited in place and sent
    /// back whole (`project:saveIndex`).
    index: Value,
    entries: Vec<Entry>,
    selected: Option<String>,
    /// The selected note's frontmatter (`file:getFrontmatter`).
    frontmatter: Value,
    layout: Layout,
    /// The layout as last saved, to notice changes.
    saved_layout: Layout,
}

impl Project {
    fn refresh(&mut self) {
        self.entries = model::entries(&self.index);
    }

    fn entry(&self, id: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }

    fn selected_entry(&self) -> Option<&Entry> {
        self.entry(self.selected.as_deref()?)
    }

    /// `isDeck`: the open note has a `presentation:` block.
    fn is_deck(&self) -> bool {
        self.selected.is_some()
            && self
                .frontmatter
                .get("presentation")
                .is_some_and(|p| !p.is_null())
    }

    fn attachments_dir(&self) -> PathBuf {
        self.folder.join(noteliner_core::project::ATTACHMENTS_DIR)
    }
}

pub struct NoteLinerApp {
    backend: Backend,
    theme: ThemeState,
    prefs: Prefs,
    /// Whether the window was built undecorated, with Marina's title bar.
    custom_titlebar: bool,
    initialized: bool,
    project: Option<Project>,
    /// A project being opened (`project:open` in flight).
    opening: bool,
    /// The Files pane's sort and tag filter (session-only, as in Svelte).
    sort: SortMode,
    hidden_tags: HashSet<String>,
    /// SEARCH, BACKLINKS and TAGS view state, and the tag filter.
    panes: PaneStates,
    history: HistoryState,
    /// A line to put the caret on once this note has loaded (search,
    /// backlinks).
    pending_line: Option<(String, usize)>,
    /// Where to put the caret and scroll of the last-opened note once it has
    /// loaded (the project was just reopened).
    pending_position: Option<LastFilePosition>,
    /// The MCP confirm's "raw arguments" are shown.
    mcp_details: bool,
    /// The MCP decisions sent, for tests.
    #[cfg(test)]
    mcp_decisions: Vec<(u64, &'static str)>,
    editor: NoteEditor,
    /// The open deck and the slides widgets.
    deck: DeckState,
    /// A note being converted to a presentation: open its settings once done
    /// (`handleConvertToPresentation`).
    pending_convert: Option<String>,
    /// Open the presentation settings once this note has loaded.
    pending_settings: Option<String>,
    /// The spell checker, loading in the background.
    spell: spell::Loader,
    spell_checker: Option<Arc<spell::AppSpell>>,
    /// What the `[[` completion list was built from (note count, open note).
    link_key: Option<(usize, usize, Option<String>)>,
    /// The editor took this frame's dropped files as attachments.
    editor_took_drop: bool,
    preview: Preview,
    tree: TreeState,
    recent: Vec<Recent>,
    log: Vec<LogLine>,
    branch: Option<String>,
    sync: Option<SyncStatus>,
    mcp_running: bool,
    mcp_queue: VecDeque<McpConfirm>,
    overlay: Option<Overlay>,
    palette: Option<CommandPalette>,
    registry: Registry<Act, View>,
    toasts: Toasts,
    update: UpdateStatus,
    last_wheel_zoom: f64,
    /// The window bounds last seen, and when they changed.
    bounds: Option<(egui::Rect, bool)>,
    bounds_changed_at: Option<f64>,
    screenshot: Option<Screenshot>,
    diagnostics: Diagnostics,
}

impl NoteLinerApp {
    /// `storage` is eframe's (theme and scale); `None` in tests.
    pub fn new(storage: Option<&dyn eframe::Storage>, paths: Paths) -> Self {
        let prefs = paths.prefs();
        let mut theme: ThemeState = storage
            .and_then(|s| eframe::get_value(s, THEME_KEY))
            .unwrap_or_default();
        diagnostics::theme_overrides(&mut theme);
        let mut registry = commands::registry();
        registry.load_recents(Recents {
            ids: prefs.command_recents.clone(),
        });
        let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        let backend = Backend::new(paths);
        let recent = backend.recent();
        Self {
            sort: SortMode::from_id(&prefs.files_sort_mode),
            custom_titlebar: prefs.custom_titlebar,
            prefs,
            backend,
            theme,
            initialized: false,
            project: None,
            opening: false,
            hidden_tags: HashSet::new(),
            panes: PaneStates::default(),
            history: HistoryState::default(),
            pending_line: None,
            pending_position: None,
            mcp_details: false,
            #[cfg(test)]
            mcp_decisions: Vec::new(),
            editor: NoteEditor::default(),
            deck: DeckState::default(),
            pending_convert: None,
            pending_settings: None,
            spell: spell::Loader::default(),
            spell_checker: None,
            link_key: None,
            editor_took_drop: false,
            preview: Preview::default(),
            tree: TreeState::default(),
            recent,
            log: Vec::new(),
            branch: None,
            sync: None,
            mcp_running: false,
            mcp_queue: VecDeque::new(),
            overlay: None,
            palette: None,
            registry,
            toasts: Toasts::default(),
            update: UpdateStatus::Idle,
            last_wheel_zoom: f64::NEG_INFINITY,
            bounds: None,
            bounds_changed_at: None,
            screenshot: Screenshot::from_env(),
            diagnostics: Diagnostics {
                open: env("MARINA_OPEN"),
                project: env("MARINA_PROJECT").map(PathBuf::from),
                file: env("MARINA_FILE"),
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
        self.editor.attach(ctx);
        if self.prefs.spell_check {
            self.spell.start(&self.backend.paths().user_data);
        }
        if let Some(folder) = self.diagnostics.project.take() {
            self.open_folder(folder);
        }
        if self.diagnostics.open.is_some() && self.diagnostics.file.is_none() {
            self.open_from_env(ctx);
        }
    }

    // --- Accessors for tests ------------------------------------------------

    #[cfg(test)]
    pub fn backend(&self) -> &Backend {
        &self.backend
    }

    #[cfg(test)]
    pub fn project_index(&self) -> Option<&Value> {
        self.project.as_ref().map(|p| &p.index)
    }

    #[cfg(test)]
    pub fn entries(&self) -> Vec<Entry> {
        self.project
            .as_ref()
            .map(|p| p.entries.clone())
            .unwrap_or_default()
    }

    #[cfg(test)]
    pub fn selected(&self) -> Option<String> {
        self.project.as_ref()?.selected.clone()
    }

    #[cfg(test)]
    pub fn editor_file_id(&self) -> Option<String> {
        self.editor.file_id().map(str::to_string)
    }

    #[cfg(test)]
    pub fn has_overlay(&self) -> bool {
        self.overlay.is_some()
    }

    #[cfg(test)]
    pub fn open(&mut self, folder: PathBuf) {
        self.open_folder(folder);
    }

    #[cfg(test)]
    pub fn layout_mut(&mut self) -> Option<&mut Layout> {
        self.project.as_mut().map(|p| &mut p.layout)
    }

    #[cfg(test)]
    pub fn editor_cursor(&self) -> (usize, usize) {
        self.editor.cursor()
    }

    /// The open note's caret (anchor, head) and scroll offset.
    #[cfg(test)]
    pub fn editor_view(&self) -> Option<(usize, usize, egui::Vec2)> {
        self.editor.view().map(|(_, a, h, s)| (a, h, s))
    }

    /// Moves the caret and scroll as if the user had (for tests).
    #[cfg(test)]
    pub fn set_editor_view(&mut self, anchor: usize, head: usize, scroll: egui::Vec2) {
        self.editor.restore_view(anchor, head, scroll);
    }

    #[cfg(test)]
    pub fn apply_for_test(&mut self, ctx: &egui::Context, act: Act) {
        self.apply(ctx, act);
    }

    #[cfg(test)]
    pub fn editor_text(&self) -> String {
        self.editor.text().to_string()
    }

    #[cfg(test)]
    pub fn deck_slides(&self) -> Option<usize> {
        self.deck.deck().map(|d| d.slides.len())
    }

    #[cfg(test)]
    pub fn presenting(&self) -> bool {
        self.deck.presenter.is_some()
    }

    #[cfg(test)]
    pub fn speaker_view_open(&self) -> bool {
        self.deck
            .presenter
            .as_ref()
            .is_some_and(|p| p.speaker_view_open())
    }

    #[cfg(test)]
    pub fn presentation_settings_open(&self) -> bool {
        matches!(self.overlay, Some(Overlay::PresentationSettings { .. }))
    }

    #[cfg(test)]
    pub fn hidden_tags(&self) -> &HashSet<String> {
        &self.hidden_tags
    }

    #[cfg(test)]
    pub fn history(&self) -> &HistoryState {
        &self.history
    }

    #[cfg(test)]
    pub fn push_mcp_confirm(&mut self, req: McpConfirm) {
        self.mcp_queue.push_back(req);
    }

    #[cfg(test)]
    pub fn mcp_decisions(&self) -> &[(u64, &'static str)] {
        &self.mcp_decisions
    }

    fn view(&self) -> View {
        let p = self.project.as_ref();
        View {
            project_open: p.is_some(),
            has_selection: p.is_some_and(|p| p.selected.is_some()),
            is_deck: p.is_some_and(Project::is_deck),
            tags_visible: p.is_some_and(|p| p.layout.show_tags),
            modal_open: self.overlay.is_some()
                || self.palette.is_some()
                || !self.mcp_queue.is_empty(),
        }
    }

    fn top_inset(&self) -> f32 {
        if self.custom_titlebar {
            theme::TITLEBAR_HEIGHT
        } else {
            0.0
        }
    }

    // --- Backend events -----------------------------------------------------

    fn handle_events(&mut self, ctx: &egui::Context) {
        for event in self.backend.poll() {
            self.handle_event(ctx, event);
        }
    }

    fn handle_event(&mut self, ctx: &egui::Context, event: Event) {
        match event {
            Event::Log(message) => self.log_line(message),
            Event::ProjectOpened {
                folder,
                index,
                needs_git_config,
            } => self.project_opened(ctx, folder, index, needs_git_config),
            Event::NeedsSetup(folder) => {
                self.opening = false;
                self.overlay = Some(Overlay::Setup(SetupForm {
                    folder: folder.display().to_string(),
                    remote: false,
                    url: String::new(),
                }));
            }
            Event::OpenFailed(message) => {
                self.opening = false;
                self.toasts
                    .error(format!("Couldn't open the project: {message}"));
            }
            Event::ProjectClosed => {
                self.project = None;
                self.editor.clear();
                self.hidden_tags.clear();
                self.panes = PaneStates::default();
                self.history = HistoryState::default();
                self.pending_line = None;
                self.pending_position = None;
                self.branch = None;
                self.sync = None;
                self.recent = self.backend.recent();
                if matches!(self.overlay, Some(Overlay::Busy { .. })) {
                    self.overlay = None;
                }
                self.backend.mcp_status();
            }
            Event::FileLoaded {
                id,
                content,
                frontmatter,
            } => {
                let Some(p) = &mut self.project else { return };
                if p.selected.as_deref() != Some(id.as_str()) {
                    return;
                }
                p.frontmatter = frontmatter;
                if let Some(e) = p.entry(&id) {
                    self.editor.load(&id, &e.name, &e.filename, content);
                }
                if let Some((pending, line)) = self.pending_line.take() {
                    if pending == id {
                        self.editor.scroll_to_line(line);
                    }
                }
                if let Some(pos) = self.pending_position.take() {
                    if pos.file_id == id {
                        self.editor.restore_view(
                            pos.anchor,
                            pos.cursor,
                            egui::vec2(pos.scroll_x, pos.scroll),
                        );
                    }
                }
                if self.pending_settings.as_deref() == Some(id.as_str()) {
                    self.pending_settings = None;
                    self.open_presentation_settings();
                }
                self.open_from_env(ctx);
            }
            Event::Saved {
                id,
                content,
                outcome,
                entry,
            } => {
                let ok = outcome == Outcome::Ok(());
                self.editor.saved(&id, &content, ok);
                if let (Some(p), Some(entry)) = (&mut self.project, entry) {
                    model::update_file(
                        &mut p.index,
                        &id,
                        &json!({ "modifiedAt": entry["modifiedAt"], "deck": entry["deck"] }),
                    );
                    if entry.get("deck").is_none() {
                        remove_key(&mut p.index, &id, "deck");
                    }
                    p.refresh();
                }
                match outcome {
                    Outcome::Ok(()) => {}
                    Outcome::GitConfigRequired => self.git_config_required(),
                    Outcome::Err(e) => self.toasts.error(format!("Couldn't save: {e}")),
                }
            }
            Event::Created(entry) => self.created(entry),
            Event::Renamed { id, entry } => {
                let (Some(p), Some(entry)) = (&mut self.project, entry) else {
                    return;
                };
                model::update_file(
                    &mut p.index,
                    &id,
                    &json!({ "name": entry["name"], "filename": entry["filename"] }),
                );
                p.refresh();
                if let Some(e) = p.entry(&id) {
                    self.editor.renamed(&id, &e.name, &e.filename);
                }
            }
            Event::Deleted(id) => {
                let Some(p) = &mut self.project else { return };
                // Children move up to the deleted note's parent, as the backend did.
                let parent = p.entry(&id).and_then(|e| e.parent_id.clone());
                if let Some(files) = p.index["files"].as_array_mut() {
                    for f in files.iter_mut() {
                        if f["parentId"].as_str() == Some(id.as_str()) {
                            f["parentId"] = json!(parent);
                        }
                    }
                }
                model::remove_file(&mut p.index, &id);
                p.refresh();
                if p.selected.as_deref() == Some(id.as_str()) {
                    p.selected = None;
                    p.frontmatter = json!({});
                    self.editor.clear();
                }
            }
            Event::IndexSaved(outcome) => match outcome {
                Outcome::Ok(()) => {}
                Outcome::GitConfigRequired => self.git_config_required(),
                Outcome::Err(e) => self.toasts.error(format!("Couldn't save the index: {e}")),
            },
            Event::Presentation { id, outcome } => match outcome {
                Outcome::Ok(presentation) => {
                    let turned_off = presentation.is_null();
                    let converted =
                        !turned_off && self.pending_convert.as_deref() == Some(id.as_str());
                    let from_settings = matches!(
                        &self.overlay,
                        Some(Overlay::PresentationSettings { id: o, .. }) if *o == id
                    );
                    if from_settings {
                        self.overlay = None;
                    }
                    let Some(p) = &mut self.project else { return };
                    if presentation.is_null() {
                        remove_key(&mut p.index, &id, "deck");
                    } else {
                        model::update_file(&mut p.index, &id, &json!({ "deck": true }));
                    }
                    if p.selected.as_deref() == Some(id.as_str()) {
                        if !p.frontmatter.is_object() {
                            p.frontmatter = json!({});
                        }
                        if presentation.is_null() {
                            if let Some(m) = p.frontmatter.as_object_mut() {
                                m.remove("presentation");
                            }
                        } else {
                            p.frontmatter["presentation"] = presentation;
                        }
                    }
                    // `handleConvertToNote` hides the SLIDES pane.
                    if turned_off && p.selected.as_deref() == Some(id.as_str()) {
                        p.layout.show_slides = false;
                    }
                    p.refresh();
                    if converted {
                        // Land in the settings, with the SLIDES pane open.
                        self.pending_convert = None;
                        if let Some(p) = &mut self.project {
                            p.layout.show_slides = true;
                        }
                        self.open_presentation_settings();
                    }
                }
                Outcome::GitConfigRequired => {
                    self.pending_convert = None;
                    if let Some(Overlay::PresentationSettings { dialog, .. }) = &mut self.overlay {
                        dialog.saving = false;
                        dialog.error =
                            Some("Set your git name and email in Project Settings first.".into());
                    } else {
                        self.git_config_required();
                    }
                }
                Outcome::Err(e) => {
                    self.pending_convert = None;
                    if let Some(Overlay::PresentationSettings { dialog, .. }) = &mut self.overlay {
                        dialog.saving = false;
                        dialog.error = Some(e);
                    } else {
                        self.toasts.error(format!("Couldn't convert: {e}"));
                    }
                }
            },
            Event::Templates(list) => {
                if let Some(Overlay::NewFile(form)) = &mut self.overlay {
                    form.templates = list;
                }
            }
            Event::GitConfig { name, email } => {
                if let Some(Overlay::ProjectSettings(form)) = &mut self.overlay {
                    if form.name.is_empty() {
                        form.name = name;
                    }
                    if form.email.is_empty() {
                        form.email = email;
                    }
                }
            }
            Event::GitState {
                branch,
                remote,
                sync,
            } => {
                self.branch = branch.clone();
                self.sync = sync.clone();
                if let Some(Overlay::Sync(s)) = &mut self.overlay {
                    s.loading = false;
                    s.branch = branch;
                    let url = remote.unwrap_or_default();
                    if s.saved_url != url || s.remote_input.is_empty() {
                        s.remote_input = url.clone();
                    }
                    s.saved_url = url;
                    s.status = sync;
                }
            }
            Event::GitDone { op, result } => {
                if let Some(Overlay::Sync(s)) = &mut self.overlay {
                    s.finish(op, &result);
                    if result.is_ok() && op != Op::Disconnect {
                        s.status = None;
                    }
                } else if let Err(e) = result {
                    self.toasts.error(e);
                }
            }
            Event::IndexReloaded(index) => {
                // `projectState.load`: the selection resets.
                if let Some(p) = &mut self.project {
                    p.index = index;
                    model::normalize_index(&mut p.index);
                    p.refresh();
                    p.selected = None;
                    p.frontmatter = json!({});
                    self.editor.clear();
                }
            }
            Event::Mcp(status) => {
                self.mcp_running = status.running;
                if let Some(Overlay::Settings(s)) = &mut self.overlay {
                    s.mcp_enabled = status.enabled;
                    s.mcp_status = Some(status);
                }
            }
            Event::McpConfirm(req) => self.mcp_queue.push_back(req),
            Event::Update(status) => self.update = status,
            Event::Exported { kind, result } => match result {
                Ok(dir) => {
                    if let Err(e) = open::that_detached(&dir) {
                        self.toasts.info(format!("Saved to {}", dir.display()));
                        log::warn!("opening {}: {e}", dir.display());
                    }
                }
                Err(e) => {
                    let what = match kind {
                        ExportKind::Html => "HTML",
                        ExportKind::Markdown => "Markdown",
                        ExportKind::Pdf => "PDF",
                    };
                    self.toasts.error(format!("Save to {what} failed: {e}"));
                }
            },
            Event::Imported { name, result } => {
                if matches!(self.overlay, Some(Overlay::Busy { .. })) {
                    self.overlay = None;
                }
                self.imported(&name, result);
            }
            Event::AttachmentAdded {
                file_id,
                result,
                insert,
            } => match result {
                Ok(att) => {
                    // `processFiles`: the editor gets the Markdown reference.
                    if insert && self.editor.file_id() == Some(file_id.as_str()) {
                        let markdown = attachment_markdown(
                            att["originalName"].as_str().unwrap_or_default(),
                            att["filename"].as_str().unwrap_or_default(),
                        );
                        self.editor.insert(&markdown, ctx.input(|i| i.time));
                    }
                    if let Some(p) = &mut self.project {
                        if let Some(files) = p.index["files"].as_array_mut() {
                            if let Some(f) = files.iter_mut().find(|f| f["id"] == file_id.as_str())
                            {
                                if !f["attachments"].is_array() {
                                    f["attachments"] = json!([]);
                                }
                                if let Some(a) = f["attachments"].as_array_mut() {
                                    a.push(att);
                                }
                            }
                        }
                        p.refresh();
                    }
                }
                Err(e) => self.toasts.error(e),
            },
            Event::AttachmentRemoved {
                file_id,
                attachment_id,
            } => {
                if let Some(p) = &mut self.project {
                    if let Some(files) = p.index["files"].as_array_mut() {
                        if let Some(f) = files.iter_mut().find(|f| f["id"] == file_id.as_str()) {
                            if let Some(a) = f["attachments"].as_array_mut() {
                                a.retain(|x| x["id"] != attachment_id.as_str());
                            }
                        }
                    }
                    p.refresh();
                }
            }
            Event::SearchResults { query, results } => {
                self.panes.search.results_arrived(&query, &results)
            }
            Event::Backlinks { id, links } => self.panes.backlinks.arrived(&id, &links),
            // `[[` completion is built from the index (as `wikilinkCompletions` is).
            Event::NoteNames(_) => {}
            Event::History { filename, entries } => self.history.loaded(&filename, entries),
            Event::HistoryContent { hash, content } => self.history.content_loaded(&hash, content),
            Event::GitConfigRequired => self.git_config_required(),
            Event::Error(message) => self.toasts.error(message),
        }
    }

    fn log_line(&mut self, message: String) {
        self.log.push(LogLine {
            time: chrono::Local::now().format("%-I:%M:%S %p").to_string(),
            message,
        });
        if self.log.len() > views::LOG_CAP {
            let over = self.log.len() - views::LOG_CAP;
            self.log.drain(..over);
        }
    }

    /// `loadProject`: the index, the saved layout and window bounds.
    fn project_opened(
        &mut self,
        ctx: &egui::Context,
        folder: PathBuf,
        mut index: Value,
        needs: bool,
    ) {
        self.opening = false;
        if matches!(
            self.overlay,
            Some(Overlay::Setup(_) | Overlay::NewProject(_) | Overlay::Busy { .. })
        ) {
            self.overlay = None;
        }
        model::normalize_index(&mut index);
        let layout = Layout::from_saved(&self.backend.layout(&folder));
        self.backend.add_recent(&folder);
        self.recent = self.backend.recent();
        self.restore_bounds(ctx, &folder);
        let mut project = Project {
            folder,
            index,
            entries: Vec::new(),
            selected: None,
            frontmatter: json!({}),
            saved_layout: layout.clone(),
            layout,
        };
        project.refresh();
        let last = project
            .layout
            .last_file_position
            .clone()
            .filter(|pos| project.entry(&pos.file_id).is_some());
        self.project = Some(project);
        self.editor.clear();
        self.pending_position = None;
        // Reopen the note that was open, where it was left.
        if let Some(pos) = last {
            let id = pos.file_id.clone();
            self.pending_position = Some(pos);
            self.select(id);
        }
        self.backend.git_state();
        self.backend.mcp_status();
        if needs {
            self.git_config_required();
        }
        self.open_from_env(ctx);
    }

    /// `projectState.addFile` + `selectFile`.
    fn created(&mut self, entry: Value) {
        let Some(p) = &mut self.project else { return };
        let id = entry["id"].as_str().unwrap_or_default().to_string();
        if p.entry(&id).is_none() {
            model::add_file(&mut p.index, entry);
        } else {
            model::update_file(&mut p.index, &id, &entry);
        }
        p.refresh();
        self.select(id);
    }

    /// `handleImportDocument`'s result: the note, and the Log panel summary.
    fn imported(&mut self, source: &str, result: Result<Value, String>) {
        match result {
            Ok(v) => {
                let entry = v["entry"].clone();
                let stats = &v["stats"];
                let plural = |n: u64, w: &str| format!("{n} {w}{}", if n == 1 { "" } else { "s" });
                let mut parts = vec![format!("Imported {source}")];
                if let Some(n) = stats["slides"].as_u64().filter(|n| *n > 0) {
                    parts.push(plural(n, "slide"));
                }
                if let Some(n) = stats["images"].as_u64().filter(|n| *n > 0) {
                    parts.push(plural(n, "image"));
                }
                if let Some(n) = stats["tablesStripped"].as_u64().filter(|n| *n > 0) {
                    parts.push(format!("{} stripped", plural(n, "table")));
                }
                self.log_line(parts.join(" — "));
                for w in stats["warnings"].as_array().into_iter().flatten() {
                    self.log_line(format!(
                        "Import warning: {}",
                        w.as_str().unwrap_or_default()
                    ));
                }
                self.created(entry);
            }
            Err(e) if e == "git_config_required" => self.git_config_required(),
            Err(e) => self.log_line(format!("Import failed: {e}")),
        }
    }

    /// A commit needs a git identity: Project Settings, marked required.
    fn git_config_required(&mut self) {
        let Some(p) = &self.project else { return };
        self.overlay = Some(Overlay::ProjectSettings(ProjectSettingsForm {
            folder: p.folder.display().to_string(),
            required: true,
            ..Default::default()
        }));
        self.backend.git_config();
    }

    // --- Projects and notes -------------------------------------------------------

    fn open_folder(&mut self, folder: PathBuf) {
        self.opening = true;
        self.backend.open_project(folder);
    }

    /// `handleOpenFolder`.
    fn pick_folder(&mut self) {
        if let Some(folder) = rfd::FileDialog::new()
            .set_title("Open Folder")
            .pick_folder()
        {
            self.open_folder(folder);
        }
    }

    /// `selectFile`: flush the pending edit to its own note, then read this one.
    fn select(&mut self, id: String) {
        let Some(p) = &mut self.project else { return };
        let Some(entry) = p.entry(&id).cloned() else {
            return;
        };
        if p.selected.as_deref() == Some(id.as_str()) && self.editor.file_id() == Some(id.as_str())
        {
            return;
        }
        if let Some(save) = self.editor.flush() {
            self.backend
                .write_file(save.id, save.filename, save.content);
        }
        p.selected = Some(id.clone());
        p.frontmatter = json!({});
        self.editor.clear();
        self.backend.read_file(id, entry.filename);
    }

    /// `selectPrevFile` / `selectNextFile`.
    fn step(&mut self, delta: i32) {
        let Some(p) = &self.project else { return };
        let list = model::flat_list(&p.entries, self.sort, &self.hidden_tags);
        if list.is_empty() {
            return;
        }
        let next = match p
            .selected
            .as_deref()
            .and_then(|s| list.iter().position(|(e, _)| e.id == s))
        {
            None => Some(0),
            Some(i) if delta < 0 && i > 0 => Some(i - 1),
            Some(i) if delta > 0 && i + 1 < list.len() => Some(i + 1),
            _ => None,
        };
        if let Some(i) = next {
            let id = list[i].0.id.clone();
            self.select(id);
        }
    }

    /// Saves the index after a tree edit (`project:saveIndex`).
    fn index_changed(&mut self) {
        if let Some(p) = &mut self.project {
            p.refresh();
            self.backend.save_index(p.index.clone());
        }
    }

    /// `handleGoHome`: save the layout, flush, close.
    fn go_home(&mut self) {
        let Some(p) = &self.project else { return };
        self.backend.save_layout(&p.folder, p.layout.to_value());
        let pending = self.editor.flush();
        self.overlay = Some(Overlay::Busy {
            title: "Syncing",
            message: "Syncing changes to remote...".into(),
        });
        self.backend.close_project(pending);
    }

    fn export(&mut self, kind: ExportKind, id: Option<String>) {
        let Some(p) = &self.project else { return };
        let Some(e) = id
            .as_deref()
            .or(p.selected.as_deref())
            .and_then(|id| p.entry(id))
        else {
            return;
        };
        // The file on disk is what's exported: write a pending edit first.
        if let Some(save) = self.editor.flush() {
            self.backend
                .write_file(save.id, save.filename, save.content);
        }
        if kind == ExportKind::Pdf {
            self.toasts.info(format!("Saving {} as PDF…", e.name));
        }
        self.backend
            .export(kind, e.filename.clone(), e.name.clone());
    }

    fn set_presentation(&mut self, id: Option<String>, on: bool) {
        let Some(p) = &self.project else { return };
        let Some(e) = id
            .as_deref()
            .or(p.selected.as_deref())
            .and_then(|id| p.entry(id))
        else {
            return;
        };
        let (id, filename, deck) = (e.id.clone(), e.filename.clone(), e.deck);
        if on && deck {
            self.presentation_settings_for(id);
            return;
        }
        let value = if on {
            // `handleConvertToPresentation` selects the note first.
            if p.selected.as_deref() != Some(id.as_str()) {
                self.select(id.clone());
            }
            self.pending_convert = Some(id.clone());
            backend::default_presentation()
        } else {
            Value::Null
        };
        self.backend.set_presentation(id, filename, value);
    }

    /// "Presentation Settings" for note `id`: selects it first if needed.
    fn presentation_settings_for(&mut self, id: String) {
        let selected = self.project.as_ref().and_then(|p| p.selected.clone());
        let loaded = self.editor.file_id() == Some(id.as_str());
        if selected.as_deref() == Some(id.as_str()) && loaded {
            self.open_presentation_settings();
        } else {
            self.pending_settings = Some(id.clone());
            self.select(id);
        }
    }

    /// `PresentationSettingsModal` over the open deck.
    fn open_presentation_settings(&mut self) {
        let Some(p) = &self.project else { return };
        let Some(e) = p.selected_entry() else { return };
        let Some(config) = normalize_presentation(p.frontmatter.get("presentation")) else {
            return;
        };
        self.overlay = Some(Overlay::PresentationSettings {
            id: e.id.clone(),
            filename: e.filename.clone(),
            dialog: Box::new(PresentationSettings::new(&config)),
        });
    }

    // --- Slides (`applySlideEdit` and the deck commands) -----------------------

    /// Re-parses the open deck when its text or `presentation:` block changed.
    fn update_deck(&mut self) {
        let presentation = self
            .project
            .as_ref()
            .filter(|p| p.is_deck())
            .and_then(|p| p.frontmatter.get("presentation"));
        self.deck.update(
            self.editor.file_id(),
            self.editor.version(),
            self.editor.text(),
            presentation,
        );
    }

    /// Replaces the note with a slide edit and writes it at once.
    fn apply_slide_edit(&mut self, edit: noteliner_slides::EditResult) {
        if let Some(req) = self.editor.apply_slide_edit(edit.markdown, edit.caret_line) {
            self.backend.write_file(req.id, req.filename, req.content);
        }
        self.update_deck();
    }

    /// A deck command from the keyboard, the palette or the toolbar.
    fn slide_command(&mut self, act: Act) {
        self.update_deck();
        let Some(deck) = self.deck.deck().cloned() else {
            return;
        };
        let line = self.editor.cursor().0;
        let active = self.deck.active(line);
        let edit = match act {
            Act::NewSlide => {
                self.toggle(|l| l.show_slides = true);
                SlideEdit::Insert {
                    after: Some(active.unwrap_or(deck.slides.len())),
                    title: String::new(),
                    body: String::new(),
                }
            }
            Act::SplitSlide => SlideEdit::SplitAtLine(line),
            Act::MoveSlideUp | Act::MoveSlideDown => {
                let Some(active) = active else { return };
                let dir = if act == Act::MoveSlideUp {
                    Direction::Up
                } else {
                    Direction::Down
                };
                SlideEdit::Move(active, dir)
            }
            Act::ToggleSpeakerNotes => {
                let (from_line, to_line) = self.editor.selected_lines();
                SlideEdit::ToggleNotes { from_line, to_line }
            }
            _ => return,
        };
        let result = edit.apply(self.editor.text(), &deck);
        self.apply_slide_edit(result);
    }

    /// The presenter, over everything, while presenting.
    fn present(&mut self, ctx: &egui::Context) {
        let Some(mut presenter) = self.deck.presenter.take() else {
            return;
        };
        let Some(deck) = self.deck.deck().cloned() else {
            return;
        };
        let name = self
            .project
            .as_ref()
            .and_then(Project::selected_entry)
            .map(|e| e.name.clone())
            .unwrap_or_default();
        let images = SlideImages {
            ctx: ctx.clone(),
            dir: self.project.as_ref().map(Project::attachments_dir),
        };
        let mut keep = true;
        for a in presenter.show(ctx, &deck, &deck::vars(&name), &images) {
            match a {
                PresenterAction::Changed(i) => {
                    if let Some(s) = deck.slides.iter().find(|s| s.index == i) {
                        self.editor.scroll_to_line(s.source_range.from_line);
                    }
                }
                PresenterAction::Exit => keep = false,
            }
        }
        if keep {
            self.deck.presenter = Some(presenter);
        }
    }

    // --- Editor inputs -------------------------------------------------------------

    /// Spell checking, the checker once loaded, and the `[[` completion list.
    fn update_editor_inputs(&mut self) {
        self.editor.set_spell_check(self.prefs.spell_check);
        if self.spell_checker.is_none() {
            if let Some(Some(checker)) = self.spell.ready() {
                self.editor.set_spell_checker(Some(
                    checker.clone() as Arc<dyn marina_editor::SpellChecker>
                ));
                self.spell_checker = Some(checker);
            }
        }
        let Some(p) = &self.project else { return };
        let fingerprint = p.entries.iter().map(|e| e.name.len() + 1).sum::<usize>();
        let key = (
            p.entries.len(),
            fingerprint,
            self.editor.file_id().map(str::to_string),
        );
        if self.link_key.as_ref() != Some(&key) {
            let current = self.editor.file_id();
            let names = p
                .entries
                .iter()
                .filter(|e| Some(e.id.as_str()) != current)
                .map(|e| e.name.clone())
                .collect();
            self.editor.set_link_targets(names);
            self.link_key = Some(key);
        }
    }

    /// An editor link: a note (`[[name]]` or `./file.md`), an attachment, or the web.
    fn open_link(&mut self, ctx: &egui::Context, target: &str) {
        if target.starts_with("http://")
            || target.starts_with("https://")
            || target.starts_with("mailto:")
        {
            ctx.open_url(egui::OpenUrl::new_tab(target));
            return;
        }
        let Some(p) = &self.project else { return };
        if let Some(name) = crate::views::preview::attachment_name(target) {
            let path = p.attachments_dir().join(name);
            if let Err(e) = open::that_detached(&path) {
                self.toasts
                    .error(format!("Couldn't open the attachment: {e}"));
            }
            return;
        }
        let file = target.trim_start_matches("./");
        let id = p
            .entries
            .iter()
            .find(|e| e.filename == file || e.name == file)
            .map(|e| e.id.clone());
        match id {
            Some(id) => self.select(id),
            None => self
                .toasts
                .info(format!("No note or attachment at {target}")),
        }
    }

    /// A `[[name]]` link: open the note, or offer to create it.
    fn open_wikilink(&mut self, name: &str) {
        let id = self.project.as_ref().and_then(|p| {
            p.entries
                .iter()
                .find(|e| e.name.to_lowercase() == name.to_lowercase())
                .map(|e| e.id.clone())
        });
        match id {
            Some(id) => self.select(id),
            None => self.overlay = Some(Overlay::NewFile(NewFileForm::new(name, false))),
        }
    }

    /// A pasted image becomes `image.png` in `_attachments/`, then its Markdown.
    fn paste_image(&mut self, file_id: String, image: &egui::ColorImage) {
        let [w, h] = image.size;
        let pixels: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
        let Some(rgba) = image::RgbaImage::from_raw(w as u32, h as u32, pixels) else {
            return;
        };
        let mut png = std::io::Cursor::new(Vec::new());
        if let Err(e) = rgba.write_to(&mut png, image::ImageFormat::Png) {
            self.log_line(format!("Attachment failed: {e}"));
            return;
        }
        self.backend
            .add_attachment(file_id, png.into_inner(), "image.png".into(), true);
    }

    fn import_dialog(&mut self) {
        if self.project.is_none() {
            return;
        }
        let picked = rfd::FileDialog::new()
            .set_title("Import Document")
            .add_filter("Documents", &["docx", "pptx"])
            .add_filter("Word Document", &["docx"])
            .add_filter("PowerPoint Presentation", &["pptx"])
            .pick_file();
        if let Some(path) = picked {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            self.overlay = Some(Overlay::Busy {
                title: "Importing",
                message: format!("Importing {name}..."),
            });
            self.backend.import(path);
        }
    }

    // --- Commands --------------------------------------------------------------

    fn apply(&mut self, ctx: &egui::Context, act: Act) {
        let zoom = matches!(
            act,
            Act::ZoomIn | Act::ZoomOut | Act::ZoomReset | Act::SetTheme(_)
        );
        match act {
            Act::NewFile | Act::NewPresentation => {
                if self.project.is_some() {
                    self.overlay = Some(Overlay::NewFile(NewFileForm::new(
                        "",
                        act == Act::NewPresentation,
                    )));
                    self.backend.list_templates();
                }
            }
            Act::DeleteFile => {
                if let Some(e) = self.project.as_ref().and_then(Project::selected_entry) {
                    self.overlay = Some(Overlay::DeleteFile {
                        id: e.id.clone(),
                        name: e.name.clone(),
                    });
                }
            }
            Act::DuplicateFile => {
                if let Some(e) = self.project.as_ref().and_then(Project::selected_entry) {
                    self.overlay = Some(Overlay::NewFile(NewFileForm::duplicate(e)));
                }
            }
            Act::ImportDocument => self.import_dialog(),
            Act::SaveAsTemplate => {
                if let Some(e) = self.project.as_ref().and_then(Project::selected_entry) {
                    self.backend
                        .save_template(e.name.clone(), self.editor.text().to_string());
                }
            }
            Act::OpenFolder => self.pick_folder(),
            Act::PrevFile => self.step(-1),
            Act::NextFile => self.step(1),
            Act::ConvertToPresentation => self.set_presentation(None, true),
            Act::ConvertToNote => self.set_presentation(None, false),
            Act::PresentationSettings => {
                if let Some(id) = self.project.as_ref().and_then(|p| p.selected.clone()) {
                    self.presentation_settings_for(id);
                }
            }
            Act::ToggleSlides => self.toggle(|l| l.show_slides = !l.show_slides),
            Act::NewSlide
            | Act::SplitSlide
            | Act::MoveSlideUp
            | Act::MoveSlideDown
            | Act::ToggleSpeakerNotes => self.slide_command(act),
            Act::Present | Act::PresentWithSpeakerView => {
                self.update_deck();
                if self.deck.deck().is_some_and(|d| !d.slides.is_empty()) {
                    let start = self.deck.active(self.editor.cursor().0).unwrap_or(1);
                    let speaker = act == Act::PresentWithSpeakerView;
                    self.deck.presenter =
                        Some(noteliner_slides::Presenter::new(start).with_speaker_view(speaker));
                    self.palette = None;
                }
            }
            Act::AddTag => {
                // `triggerTagAction('add')`: the TAGS pane's input (shown first
                // if it's hidden).
                if let Some(p) = &mut self.project {
                    p.layout.show_tags = true;
                    self.panes.tags.start_add(p.selected.as_deref());
                }
            }
            Act::TogglePreview => self.toggle(|l| l.show_preview = !l.show_preview),
            Act::ToggleHistory => self.toggle(|l| l.show_history = !l.show_history),
            Act::ToggleSidebar => self.toggle(|l| l.show_sidebar = !l.show_sidebar),
            Act::ToggleToolbar => self.toggle(|l| l.show_toolbar = !l.show_toolbar),
            Act::ToggleOutline => self.toggle(|l| l.show_outline = !l.show_outline),
            Act::ToggleTags => self.toggle(|l| l.show_tags = !l.show_tags),
            Act::ToggleAttachments => self.toggle(|l| l.show_attachments = !l.show_attachments),
            Act::ToggleSearch => {
                self.toggle(|l| l.show_search = !l.show_search);
                // `searchFocusTs`: opening it focuses the input.
                if self.project.as_ref().is_some_and(|p| p.layout.show_search) {
                    self.panes.search.focus = true;
                }
            }
            Act::ToggleBacklinks => self.toggle(|l| l.show_backlinks = !l.show_backlinks),
            Act::ToggleLog => self.toggle(|l| l.show_log = !l.show_log),
            Act::ToggleStatusBar => self.toggle(|l| l.show_status_bar = !l.show_status_bar),
            Act::ToggleSpellCheck => {
                self.prefs.spell_check = !self.prefs.spell_check;
                if self.prefs.spell_check {
                    self.spell.start(&self.backend.paths().user_data);
                }
                self.backend
                    .set_prefs(json!({ "spellCheckEnabled": self.prefs.spell_check }));
            }
            Act::GoToLine => self.editor.go_to_line(),
            Act::ZoomIn => self.theme.zoom_in(),
            Act::ZoomOut => self.theme.zoom_out(),
            Act::ZoomReset => self.theme.zoom_reset(),
            Act::SetTheme(t) => self.theme.theme = t,
            Act::Sync => {
                if self.project.is_some() {
                    self.overlay = Some(Overlay::Sync(SyncState::new()));
                    self.backend.git_state();
                }
            }
            Act::ProjectSettings => {
                if let Some(p) = &self.project {
                    self.overlay = Some(Overlay::ProjectSettings(ProjectSettingsForm {
                        folder: p.folder.display().to_string(),
                        ..Default::default()
                    }));
                    self.backend.git_config();
                }
            }
            Act::GoHome => self.go_home(),
            Act::Settings => {
                self.overlay = Some(Overlay::Settings(Box::new(SettingsState::new(
                    "ui",
                    &self.backend.paths().prefs(),
                    self.custom_titlebar,
                ))));
                self.backend.mcp_status();
            }
            Act::About => self.overlay = Some(Overlay::About),
            Act::Help => self.overlay = Some(Overlay::Help(Box::default())),
            Act::Palette => {
                self.recent = self.backend.recent();
                self.palette =
                    Some(CommandPalette::new().with_placeholder("Type a command or note name..."));
            }
            Act::OpenNote(id) => self.select(id),
            Act::OpenRecent(path) => self.open_folder(PathBuf::from(path)),
        }
        if zoom {
            self.theme.apply(ctx);
        }
    }

    /// Changes the project's layout (layout toggles need an open project;
    /// the Svelte layout object is per project too).
    fn toggle(&mut self, f: impl FnOnce(&mut Layout)) {
        if let Some(p) = &mut self.project {
            f(&mut p.layout);
        }
    }

    fn context_action(&mut self, action: ContextAction, id: String) {
        match action {
            ContextAction::OpenInFileSystem => {
                if let Some(p) = &self.project {
                    if let Err(e) = open::that_detached(&p.folder) {
                        self.toasts.error(format!("Couldn't open the folder: {e}"));
                    }
                }
            }
            ContextAction::Preview => self.toggle(|l| l.show_preview = !l.show_preview),
            ContextAction::Export(kind) => self.export(kind, Some(id)),
            ContextAction::ConvertToPresentation => self.set_presentation(Some(id), true),
            ContextAction::ConvertToNote => self.set_presentation(Some(id), false),
            ContextAction::PresentationSettings => self.presentation_settings_for(id),
            ContextAction::Duplicate => {
                if let Some(e) = self.project.as_ref().and_then(|p| p.entry(&id)) {
                    self.overlay = Some(Overlay::NewFile(NewFileForm::duplicate(e)));
                }
            }
            ContextAction::Move => {
                if let Some(p) = &self.project {
                    if let Some(e) = p.entry(&id) {
                        self.overlay = Some(Overlay::MoveFile(MoveForm::new(e, &p.entries)));
                    }
                }
            }
            ContextAction::ClearTags => {
                if let Some(e) = self.project.as_ref().and_then(|p| p.entry(&id)) {
                    if !e.tags.is_empty() {
                        self.overlay = Some(Overlay::ClearTags {
                            id: e.id.clone(),
                            name: e.name.clone(),
                            count: e.tags.len(),
                        });
                    }
                }
            }
            ContextAction::Delete => {
                if let Some(e) = self.project.as_ref().and_then(|p| p.entry(&id)) {
                    self.overlay = Some(Overlay::DeleteFile {
                        id: e.id.clone(),
                        name: e.name.clone(),
                    });
                }
            }
        }
    }

    fn open_from_env(&mut self, ctx: &egui::Context) {
        if let (Some(p), Some(file)) = (&self.project, self.diagnostics.file.clone()) {
            let list = model::flat_list(&p.entries, self.sort, &self.hidden_tags);
            let id = match file.parse::<usize>() {
                Ok(i) => list.get(i).map(|(e, _)| e.id.clone()),
                Err(_) => p
                    .entries
                    .iter()
                    .find(|e| e.name == file)
                    .map(|e| e.id.clone()),
            };
            self.diagnostics.file = None;
            if let Some(id) = id {
                self.select(id);
                return;
            }
        }
        if self.diagnostics.file.is_some() {
            return;
        }
        let Some(open) = self.diagnostics.open.take() else {
            return;
        };
        let (what, arg) = open.split_once(':').unwrap_or((open.as_str(), ""));
        match what {
            "settings" => {
                self.apply(ctx, Act::Settings);
                if let Some(Overlay::Settings(s)) = &mut self.overlay {
                    if !arg.is_empty() {
                        s.tab = arg.to_string();
                    }
                }
            }
            "sync" => self.apply(ctx, Act::Sync),
            "about" => self.apply(ctx, Act::About),
            "help" => self.apply(ctx, Act::Help),
            "palette" => self.apply(ctx, Act::Palette),
            "new-file" => self.apply(ctx, Act::NewFile),
            "new-project" => {
                self.overlay = Some(Overlay::NewProject(NewProjectForm::with_defaults()))
            }
            "project-settings" => self.apply(ctx, Act::ProjectSettings),
            "move" => {
                if let Some(id) = self.project.as_ref().and_then(|p| p.selected.clone()) {
                    self.context_action(ContextAction::Move, id);
                }
            }
            "preview" => self.toggle(|l| l.show_preview = true),
            "slides" => self.toggle(|l| {
                l.show_slides = true;
                l.show_preview = true;
            }),
            "presentation-settings" => self.apply(ctx, Act::PresentationSettings),
            "present" => self.apply(ctx, Act::Present),
            "present-speaker" => self.apply(ctx, Act::PresentWithSpeakerView),
            "log" => self.toggle(|l| l.show_log = true),
            "search" => {
                self.toggle(|l| l.show_search = true);
                let now = ctx.input(|i| i.time);
                self.panes.search.set_query(arg, now - 1.0);
            }
            "tags" => self.toggle(|l| l.show_tags = true),
            "backlinks" => self.toggle(|l| l.show_backlinks = true),
            "history" => self.toggle(|l| l.show_history = true),
            "attachments" => self.toggle(|l| l.show_attachments = true),
            "tag-filter" => self.panes.filter_open = true,
            "tag-editor" => self.overlay = Some(Overlay::TagEditor(Default::default())),
            "mcp-walkthrough" => {
                self.apply(ctx, Act::Settings);
                if let Some(Overlay::Settings(s)) = &mut self.overlay {
                    s.tab = "mcp".into();
                    s.confirm_mcp = true;
                }
            }
            "mcp-confirm" => {
                // A sample request, to see the dialog.
                self.mcp_queue.push_back(McpConfirm {
                    id: u64::MAX,
                    tool: "update_note".into(),
                    summary: "Update “Welcome” (412 chars)".into(),
                    args: json!({ "fileId": "welcome", "body": "# Welcome\n\n".repeat(60) }),
                });
                self.mcp_details = true;
            }
            _ => {}
        }
    }

    // --- Per-frame work ------------------------------------------------------------

    /// Commands first, so their keys don't reach the editor (the Svelte
    /// keydown handler runs in the capture phase). Only the palette blocks
    /// them entirely; other dialogs leave zoom working (`when`).
    fn dispatch_keys(&mut self, ctx: &egui::Context) {
        if self.palette.is_some() {
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
        self.editor_took_drop = false;
        self.update_editor_inputs();
        self.update_deck();
        // While presenting, the presenter owns the keyboard.
        if self.deck.presenter.is_none() {
            self.dispatch_keys(&ctx);
        }
        self.wheel_zoom(&ctx);

        if self.custom_titlebar {
            self.titlebar(ui);
        }
        let show_toolbar = self.project.as_ref().is_none_or(|p| p.layout.show_toolbar);
        let show_status = self
            .project
            .as_ref()
            .is_none_or(|p| p.layout.show_status_bar);
        if show_status {
            self.status_bar(ui);
        }
        if show_toolbar {
            self.toolbar(ui);
        }
        self.main_view(ui);
        self.overlays(&ctx);
        self.mcp_confirms(&ctx);
        self.palette(&ctx);
        self.dropped_files(&ctx);
        self.present(&ctx);
        self.toasts.show(&ctx);
        if self.custom_titlebar {
            window::window_edges(&ctx);
        }
        self.track_position();
        self.persist_layout();
        self.track_bounds(&ctx);
        if let Some(shot) = &mut self.screenshot {
            shot.tick(&ctx);
        }
    }

    fn titlebar(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let open = self.project.is_some();
        let bar = TitleBar {
            app_name: "NoteLiner",
            toolbar_visible: self.project.as_ref().is_none_or(|p| p.layout.show_toolbar),
        };
        let mut act = None;
        let actions = titlebar::show(ui, &bar, &mut self.theme, |ui| {
            if titlebar::button(ui, icons::HOUSE, "Home", !open).clicked() && open {
                act = Some(Act::GoHome);
            }
            if titlebar::button(ui, icons::FOLDER_OPEN, "Open Folder (Ctrl+O)", false).clicked() {
                act = Some(Act::OpenFolder);
            }
            if open {
                if titlebar::button(ui, icons::FILE_CIRCLE_PLUS, "New File (Ctrl+N)", false)
                    .clicked()
                {
                    act = Some(Act::NewFile);
                }
                if titlebar::button(
                    ui,
                    icons::FILE_IMPORT,
                    "Import Document (Ctrl+Alt+I)",
                    false,
                )
                .clicked()
                {
                    act = Some(Act::ImportDocument);
                }
            }
        });
        for a in actions {
            match a {
                TitleBarAction::ToggleToolbar => act = Some(Act::ToggleToolbar),
                // Already applied by the title bar; saved with the theme.
                TitleBarAction::ScaleChanged(_) => {}
            }
        }
        if let Some(a) = act {
            self.apply(&ctx, a);
        }
    }

    /// `Toolbar.svelte`: the left column of buttons.
    fn toolbar(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let open = self.project.is_some();
        let custom = self.custom_titlebar;
        let l = self
            .project
            .as_ref()
            .map(|p| p.layout.clone())
            .unwrap_or_default();
        let is_deck = self.project.as_ref().is_some_and(Project::is_deck);
        let update_ready = matches!(self.update, UpdateStatus::Available { .. });
        let mut acts = Vec::new();
        let mut bottom_acts = Vec::new();
        let b = |ui: &mut Ui, glyph, title, active: bool, disabled: bool| {
            toolbar::button(
                ui,
                ToolbarButton::new(glyph, title)
                    .active(active)
                    .disabled(disabled),
            )
            .clicked()
        };
        toolbar::show(
            ui,
            |ui| {
                if !custom {
                    if b(ui, icons::HOUSE, "Home", !open, !open) {
                        acts.push(Act::GoHome);
                    }
                    if b(ui, icons::FOLDER_OPEN, "Open Folder (Ctrl+O)", false, false) {
                        acts.push(Act::OpenFolder);
                    }
                }
                if open {
                    if !custom {
                        toolbar::divider(ui);
                        if b(
                            ui,
                            icons::FILE_CIRCLE_PLUS,
                            "New File (Ctrl+N)",
                            false,
                            false,
                        ) {
                            acts.push(Act::NewFile);
                        }
                        if b(
                            ui,
                            icons::FILE_IMPORT,
                            "Import Document (Ctrl+Alt+I)",
                            false,
                            false,
                        ) {
                            acts.push(Act::ImportDocument);
                        }
                        toolbar::divider(ui);
                    }
                    if b(
                        ui,
                        icons::BARS_STAGGERED,
                        "Files (Ctrl+E)",
                        l.show_sidebar,
                        false,
                    ) {
                        acts.push(Act::ToggleSidebar);
                    }
                    if b(
                        ui,
                        icons::LIST_OL,
                        "Outline (Ctrl+Shift+O)",
                        l.show_outline,
                        false,
                    ) {
                        acts.push(Act::ToggleOutline);
                    }
                    if is_deck
                        && b(
                            ui,
                            icons::PERSON_CHALKBOARD,
                            "Slides (Ctrl+Shift+G)",
                            l.show_slides,
                            false,
                        )
                    {
                        acts.push(Act::ToggleSlides);
                    }
                    if is_deck && b(ui, icons::CIRCLE_PLAY, "Present (F5)", false, false) {
                        acts.push(Act::Present);
                    }
                    if is_deck
                        && b(
                            ui,
                            icons::PERSON_CHALKBOARD,
                            "Present with Speaker View (Alt+F5)",
                            false,
                            false,
                        )
                    {
                        acts.push(Act::PresentWithSpeakerView);
                    }
                    if b(ui, icons::TAG, "Tags (Ctrl+Shift+T)", l.show_tags, false) {
                        acts.push(Act::ToggleTags);
                    }
                    if b(
                        ui,
                        icons::PAPERCLIP,
                        "Attachments (Ctrl+Shift+A)",
                        l.show_attachments,
                        false,
                    ) {
                        acts.push(Act::ToggleAttachments);
                    }
                    toolbar::divider(ui);
                    if b(
                        ui,
                        icons::MAGNIFYING_GLASS,
                        "Search (Ctrl+F)",
                        l.show_search,
                        false,
                    ) {
                        acts.push(Act::ToggleSearch);
                    }
                    if b(
                        ui,
                        icons::LINK,
                        "Backlinks (Ctrl+Shift+B)",
                        l.show_backlinks,
                        false,
                    ) {
                        acts.push(Act::ToggleBacklinks);
                    }
                }
            },
            |ui| {
                if open
                    && b(
                        ui,
                        icons::CLOUD_ARROW_UP,
                        "Remote Sync (Ctrl+Shift+S)",
                        false,
                        false,
                    )
                {
                    bottom_acts.push(Act::Sync);
                }
                if b(ui, icons::TERMINAL, "Show Log (Ctrl+L)", l.show_log, !open) {
                    bottom_acts.push(Act::ToggleLog);
                }
                if b(
                    ui,
                    icons::WINDOW_MAXIMIZE,
                    "Status Bar (Ctrl+J)",
                    l.show_status_bar,
                    !open,
                ) {
                    bottom_acts.push(Act::ToggleStatusBar);
                }
                toolbar::divider(ui);
                if open
                    && b(
                        ui,
                        icons::SLIDERS,
                        "Project Settings (Ctrl+Shift+,)",
                        false,
                        false,
                    )
                {
                    bottom_acts.push(Act::ProjectSettings);
                }
                if b(ui, icons::GEAR, "Settings (Ctrl+,)", false, false) {
                    bottom_acts.push(Act::Settings);
                }
                let about = toolbar::button(
                    ui,
                    ToolbarButton::new(
                        icons::CIRCLE_INFO,
                        if update_ready {
                            "Update available — About (Ctrl+Shift+I)"
                        } else {
                            "About (Ctrl+Shift+I)"
                        },
                    )
                    .attention(update_ready),
                );
                if about.clicked() {
                    bottom_acts.push(Act::About);
                }
            },
        );
        for a in acts.into_iter().chain(bottom_acts) {
            self.apply(&ctx, a);
        }
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let mut open_sync = false;
        let p = self.project.as_ref();
        let note = p
            .and_then(Project::selected_entry)
            .map(|e| (e.name.clone(), self.editor.status()));
        let props = StatusProps {
            project_open: p.is_some(),
            note_count: p.map_or(0, |p| p.entries.len()),
            note: note.as_ref().map(|(n, s)| (n.as_str(), *s)),
            text: self.editor.text(),
            cursor: self.editor.cursor(),
            selection_len: self.editor.selection_len(),
            spell_check: self.prefs.spell_check,
            deck: if p.is_some_and(Project::is_deck) {
                self.deck.status(self.editor.cursor().0)
            } else {
                None
            },
            mcp_running: self.mcp_running,
            branch: self.branch.as_deref(),
            sync: self.sync.as_ref(),
        };
        egui::Panel::bottom("noteliner-status")
            .exact_size(views::STATUS_BAR_HEIGHT)
            .resizable(false)
            .show_separator_line(false)
            .frame(Frame::NONE)
            .show(ui, |ui| open_sync = views::status_bar(ui, &props));
        if open_sync {
            self.apply(&ctx, Act::Sync);
        }
    }

    fn main_view(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let pal = theme::palette(&ctx);
        if self.project.is_none() {
            let mut action = None;
            egui::CentralPanel::default()
                .frame(Frame::NONE.fill(pal.bg_base))
                .show(ui, |ui| {
                    if self.opening {
                        ui.add_space(48.0);
                        ui.horizontal(|ui| {
                            ui.add_space(48.0);
                            ui.spinner();
                            ui.label(egui::RichText::new("Opening…").color(pal.text_muted));
                        });
                        return;
                    }
                    action = views::open_screen(ui, &self.recent);
                });
            match action {
                Some(OpenAction::NewProject) => {
                    self.overlay = Some(Overlay::NewProject(NewProjectForm::with_defaults()))
                }
                Some(OpenAction::OpenFolder) => self.pick_folder(),
                Some(OpenAction::OpenRecent(path)) => self.open_folder(PathBuf::from(path)),
                Some(OpenAction::RemoveRecent(r)) => self.overlay = Some(Overlay::RemoveRecent(r)),
                None => {}
            }
            return;
        }

        let mut sidebar_actions = Vec::new();
        let mut preview_actions = Vec::new();
        let mut editor_actions = Vec::new();
        let mut history_actions = Vec::new();
        let mut attachment_actions = Vec::new();
        let mut log_action = None;
        // A note dragged from FILES: captured before the tree forgets it on release.
        let tree_drag = self.tree.dragging().map(str::to_string);
        let slide_images = SlideImages {
            ctx: ctx.clone(),
            dir: self.project.as_ref().map(Project::attachments_dir),
        };
        let caret_line = self.editor.cursor().0;
        let active_slide = self.deck.active(caret_line);
        let Some(p) = self.project.as_mut() else {
            return;
        };
        let is_deck = p.is_deck();
        let note_name = p
            .selected_entry()
            .map(|e| e.name.clone())
            .unwrap_or_default();

        // BACKLINKS and HISTORY follow the selection while they're shown.
        if p.layout.show_backlinks {
            let files = p.entries.len();
            if let Some(id) = self.panes.backlinks.stale(p.selected.as_deref(), files) {
                self.backend.backlinks(id);
            }
        } else {
            // Reopening the pane loads afresh, as remounting it does.
            self.panes.backlinks.invalidate();
        }
        if p.layout.show_history {
            let filename = p.selected_entry().map(|e| e.filename.clone());
            if let Some(HistoryAction::Load(f)) = self.history.follow(filename.as_deref()) {
                self.backend.history(f);
            }
        } else {
            self.history.invalidate();
        }

        if p.layout.show_log {
            let resp = egui::Panel::bottom("noteliner-log")
                .resizable(true)
                .default_size(p.layout.log_panel_height)
                .size_range(20.0..=500.0)
                .frame(Frame::NONE.fill(pal.bg_overlay))
                .show(ui, |ui| views::log_panel(ui, &self.log));
            log_action = resp.inner;
            remember_size(&mut p.layout.log_panel_height, resp.response.rect.height());
        }

        let any_pane = crate::config::PANE_KEYS
            .iter()
            .any(|k| p.layout.pane_visible(k) && (*k != "slides" || is_deck));
        if any_pane {
            let resp = egui::Panel::left("noteliner-sidebar")
                .resizable(true)
                .default_size(p.layout.sidebar_width)
                .size_range(180.0..=500.0)
                .frame(Frame::NONE.fill(pal.bg_surface))
                .show(ui, |ui| {
                    let (parsed, pane, _) = self.deck.parts();
                    let props = SidebarProps {
                        index: &p.index,
                        entries: &p.entries,
                        sort: self.sort,
                        hidden_tags: &self.hidden_tags,
                        selected: p.selected.as_deref(),
                        text: self.editor.text(),
                        cursor_line: caret_line,
                        is_deck,
                        deck: parsed,
                        active_slide,
                        images: &slide_images,
                    };
                    sidebar::show(
                        ui,
                        &mut p.layout,
                        &mut self.tree,
                        &mut self.panes,
                        pane,
                        &props,
                    )
                });
            sidebar_actions = resp.inner;
            remember_size(&mut p.layout.sidebar_width, resp.response.rect.width());
        }

        if p.layout.show_attachments {
            let list = p
                .selected
                .as_deref()
                .map(|id| attachments::of_note(&p.index, id))
                .unwrap_or_default();
            let dir = p.attachments_dir();
            let selected = p.selected.is_some();
            let resp = egui::Panel::right("noteliner-attachments")
                .resizable(true)
                .default_size(p.layout.attachment_panel_width)
                .size_range(160.0..=400.0)
                .frame(Frame::NONE.fill(pal.bg_surface))
                .show(ui, |ui| attachments::show(ui, selected, &list, &dir));
            attachment_actions = resp.inner;
            remember_size(
                &mut p.layout.attachment_panel_width,
                resp.response.rect.width(),
            );
        }
        if p.layout.show_preview {
            let attachments = p.attachments_dir();
            let resp = egui::Panel::right("noteliner-preview")
                .resizable(true)
                .default_size(p.layout.preview_width)
                .size_range(200.0..=1600.0)
                .frame(Frame::NONE.fill(pal.bg_surface))
                .show(ui, |ui| {
                    let vars = deck::vars(&note_name);
                    let (parsed, _, preview) = self.deck.parts();
                    let deck = is_deck.then_some(DeckProps {
                        deck: parsed,
                        active: active_slide,
                        vars: &vars,
                        images: &slide_images,
                        preview,
                    });
                    self.preview.show(
                        ui,
                        self.editor.file_id(),
                        self.editor.text(),
                        deck,
                        Some(attachments),
                    )
                });
            preview_actions = resp.inner;
            remember_size(&mut p.layout.preview_width, resp.response.rect.width());
        }
        if p.layout.show_history {
            let dir = p.attachments_dir();
            history_actions = egui::Panel::right("noteliner-history")
                .resizable(true)
                .default_size(320.0)
                .frame(Frame::NONE.fill(pal.bg_surface))
                .show(ui, |ui| history::show(ui, &mut self.history, Some(dir)))
                .inner;
        }
        let (preview_on, history_on) = (p.layout.show_preview, p.layout.show_history);
        egui::CentralPanel::default()
            .frame(Frame::NONE.fill(pal.bg_base))
            .show(ui, |ui| {
                editor_actions = self.editor.show(ui, preview_on, history_on);
            });

        // A note dropped from FILES onto the text: a link at the drop point
        // (`interceptInternalDrops`).
        if let Some(dragged) = tree_drag {
            let (released, pointer, now) =
                ctx.input(|i| (i.pointer.any_released(), i.pointer.interact_pos(), i.time));
            if released {
                if let Some(at) = self.editor.drop_target(pointer) {
                    let link = self.project.as_ref().and_then(|p| {
                        p.entry(&dragged)
                            .map(|e| format!("[{}](./{})", e.name, e.filename))
                    });
                    if let Some(link) = link {
                        self.editor.insert_at(at, &link, now);
                    }
                }
            }
        }

        for a in sidebar_actions {
            self.sidebar_action(a);
        }

        for a in history_actions {
            match a {
                HistoryAction::Close => self.toggle(|l| l.show_history = false),
                HistoryAction::Load(f) => self.backend.history(f),
                HistoryAction::LoadContent { hash, filename } => {
                    self.backend.history_content(hash, filename)
                }
                HistoryAction::OpenUrl(url) => ctx.open_url(egui::OpenUrl::new_tab(url)),
            }
        }
        for a in attachment_actions {
            self.attachment_action(&ctx, a);
        }
        for a in preview_actions {
            self.preview_action(&ctx, a);
        }
        for a in editor_actions {
            match a {
                EditorAction::Save(req) => {
                    self.backend.write_file(req.id, req.filename, req.content)
                }
                EditorAction::TogglePreview => self.apply(&ctx, Act::TogglePreview),
                EditorAction::ToggleHistory => self.apply(&ctx, Act::ToggleHistory),
                EditorAction::Export(kind) => self.export(kind, None),
                EditorAction::PasteImage { file_id, image } => self.paste_image(file_id, &image),
                EditorAction::AttachFiles { file_id, paths } => {
                    self.editor_took_drop = true;
                    for path in paths {
                        self.attach_file(&file_id, &path, true);
                    }
                }
                EditorAction::OpenWikilink(name) => self.open_wikilink(&name),
                EditorAction::OpenLink(url) => self.open_link(&ctx, &url),
                EditorAction::AddToDictionary(word) => {
                    if let Some(checker) = &self.spell_checker {
                        checker.add_word(&word);
                    }
                    self.editor.refresh_spelling();
                }
            }
        }
        match log_action {
            Some(views::LogAction::Clear) => self.log.clear(),
            Some(views::LogAction::Close) => self.toggle(|l| l.show_log = false),
            None => {}
        }
    }

    fn sidebar_action(&mut self, action: SidebarAction) {
        match action {
            SidebarAction::Slides(a) => match a {
                noteliner_slides::PaneAction::Jump { line } => self.editor.scroll_to_line(line),
                noteliner_slides::PaneAction::Edit(result) => self.apply_slide_edit(result),
                noteliner_slides::PaneAction::OpenSettings => {
                    if let Some(id) = self.project.as_ref().and_then(|p| p.selected.clone()) {
                        self.presentation_settings_for(id);
                    }
                }
            },
            SidebarAction::Tree(t) => match t {
                TreeAction::Select(id) => self.select(id),
                TreeAction::Rename { id, name } => self.backend.rename_file(id, name),
                TreeAction::Drop {
                    dragged,
                    target,
                    pos,
                } => {
                    let changed = self
                        .project
                        .as_mut()
                        .is_some_and(|p| model::apply_drop(&mut p.index, &dragged, &target, pos));
                    if changed {
                        self.index_changed();
                    }
                }
                TreeAction::Context(a, id) => self.context_action(a, id),
                TreeAction::Step(d) => self.step(d),
            },
            SidebarAction::Sort(mode) => {
                self.sort = mode;
                self.backend
                    .set_prefs(json!({ "filesSortMode": mode.id() }));
            }
            SidebarAction::GoToLine(line) => self.editor.scroll_to_line(line),
            SidebarAction::ClosePane(id) => self.toggle(|l| l.close_pane(&id)),
            // The layout is saved by `persist_layout`.
            SidebarAction::LayoutChanged => {}
            SidebarAction::Pane(a) => self.pane_action(a),
        }
    }

    /// The SEARCH, BACKLINKS and TAGS panes and the tag filter.
    fn pane_action(&mut self, action: PaneAction) {
        match action {
            PaneAction::GoTo { id, line } => {
                // `selectFile` then `scrollToLine`: once the note has loaded.
                let open = self.editor.file_id() == Some(id.as_str());
                if open {
                    self.editor.scroll_to_line(line);
                } else {
                    self.pending_line = Some((id.clone(), line));
                    self.select(id);
                }
            }
            PaneAction::Search(q) => self.backend.search(q, false),
            PaneAction::ToggleTag(tag) => {
                let changed = self.project.as_mut().is_some_and(|p| {
                    let Some(id) = p.selected.clone() else {
                        return false;
                    };
                    let on = p.entry(&id).is_some_and(|e| e.tags.contains(&tag));
                    if on {
                        model::remove_tag(&mut p.index, &id, &tag)
                    } else {
                        model::add_tag(&mut p.index, &id, &tag)
                    }
                });
                if changed {
                    self.index_changed();
                }
            }
            PaneAction::AddTag(tag) => {
                let changed = self.project.as_mut().is_some_and(|p| {
                    p.selected
                        .clone()
                        .is_some_and(|id| model::add_tag(&mut p.index, &id, &tag))
                });
                if changed {
                    self.index_changed();
                }
            }
            PaneAction::TagDropped { file, tag } => {
                let changed = self
                    .project
                    .as_mut()
                    .is_some_and(|p| model::add_tag(&mut p.index, &file, &tag));
                if changed {
                    self.index_changed();
                }
            }
            PaneAction::StartAddTag => {
                let selected = self.project.as_ref().and_then(|p| p.selected.clone());
                self.panes.tags.start_add(selected.as_deref());
            }
            PaneAction::OpenTagEditor => {
                self.overlay = Some(Overlay::TagEditor(Default::default()));
            }
            PaneAction::FilterToggle(key) => {
                if !self.hidden_tags.remove(&key) {
                    self.hidden_tags.insert(key);
                }
            }
            PaneAction::FilterShowAll => self.hidden_tags.clear(),
            PaneAction::FilterHideAll => {
                if let Some(p) = &self.project {
                    self.hidden_tags.extend(model::all_tags(&p.index));
                    self.hidden_tags.insert(model::UNTAGGED_KEY.to_string());
                }
            }
        }
    }

    /// The Attachments panel (`AttachmentPanel.svelte`).
    fn attachment_action(&mut self, ctx: &egui::Context, action: AttachmentAction) {
        match action {
            AttachmentAction::Close => self.toggle(|l| l.show_attachments = false),
            AttachmentAction::Add => {
                let Some(id) = self.project.as_ref().and_then(|p| p.selected.clone()) else {
                    return;
                };
                let files = rfd::FileDialog::new()
                    .set_title("Add Attachments")
                    .pick_files()
                    .unwrap_or_default();
                for path in files {
                    self.attach_file(&id, &path, false);
                }
            }
            AttachmentAction::Open(path) => {
                if let Err(e) = open::that_detached(&path) {
                    self.toasts
                        .error(format!("Couldn't open the attachment: {e}"));
                }
            }
            AttachmentAction::Remove(attachment) => {
                if let Some(id) = self.project.as_ref().and_then(|p| p.selected.clone()) {
                    self.overlay = Some(Overlay::RemoveAttachment {
                        file_id: id,
                        attachment,
                    });
                }
            }
            AttachmentAction::Insert(markdown) => {
                let now = ctx.input(|i| i.time);
                if !self.editor.insert(&markdown, now) {
                    ctx.copy_text(markdown);
                    self.toasts.info("Copied the Markdown reference");
                }
            }
        }
    }

    /// `handleAddFiles` for one file: the 30 MB limit, then `file:addAttachment`.
    fn attach_file(&mut self, file_id: &str, path: &std::path::Path, insert: bool) {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        if size > attachments::MAX_SIZE {
            self.log_line(format!("Attachment rejected: {name} exceeds 30MB limit"));
            return;
        }
        match std::fs::read(path) {
            Ok(data) => self
                .backend
                .add_attachment(file_id.to_string(), data, name, insert),
            Err(e) => self.log_line(format!("Attachment failed: {e}")),
        }
    }

    fn preview_action(&mut self, ctx: &egui::Context, action: PreviewAction) {
        match action {
            PreviewAction::Close => self.toggle(|l| l.show_preview = false),
            PreviewAction::Wikilink(name) => self.open_wikilink(&name),
            PreviewAction::OpenUrl(url) => ctx.open_url(egui::OpenUrl::new_tab(url)),
            PreviewAction::JumpToLine(line) => self.editor.scroll_to_line(line),
        }
    }

    /// Remembers the open note's caret and scroll in the layout (saved with
    /// it, debounced). Waits until a pending restore has been applied, so
    /// reopening doesn't overwrite the saved place with the top of the note.
    fn track_position(&mut self) {
        let Some(p) = &mut self.project else { return };
        let Some((id, anchor, head, scroll)) = self.editor.view() else {
            return;
        };
        if p.selected.as_deref() != Some(id)
            || self
                .pending_position
                .as_ref()
                .is_some_and(|pos| pos.file_id == id)
        {
            return;
        }
        let pos = LastFilePosition {
            file_id: id.to_string(),
            anchor,
            cursor: head,
            scroll_x: scroll.x.round(),
            scroll: scroll.y.round(),
        };
        if p.layout.last_file_position.as_ref() != Some(&pos) {
            p.layout.last_file_position = Some(pos);
        }
    }

    /// `scheduleLayoutSave`: `window-state:saveLayout` when it changes
    /// (`WindowState` debounces the write by a second).
    fn persist_layout(&mut self) {
        let Some(p) = &mut self.project else { return };
        if p.layout != p.saved_layout {
            p.saved_layout = p.layout.clone();
            self.backend.save_layout(&p.folder, p.layout.to_value());
        }
    }

    /// The window bounds, saved per project like the Tauri host does.
    fn track_bounds(&mut self, ctx: &egui::Context) {
        let Some(p) = &self.project else { return };
        let (rect, inner, maximized, now) = ctx.input(|i| {
            let v = i.viewport();
            (
                v.outer_rect,
                v.inner_rect,
                v.maximized.unwrap_or(false),
                i.time,
            )
        });
        let (Some(outer), Some(inner)) = (rect, inner) else {
            return;
        };
        let bounds = egui::Rect::from_min_size(outer.min, inner.size());
        if self.bounds != Some((bounds, maximized)) {
            self.bounds = Some((bounds, maximized));
            self.bounds_changed_at = Some(now);
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(BOUNDS_DEBOUNCE_SECS));
            return;
        }
        if self
            .bounds_changed_at
            .is_some_and(|t| now - t >= BOUNDS_DEBOUNCE_SECS)
        {
            self.bounds_changed_at = None;
            if !maximized {
                let v = json!({
                    "x": bounds.min.x.round(),
                    "y": bounds.min.y.round(),
                    "width": bounds.width().round(),
                    "height": bounds.height().round(),
                });
                self.backend.save_bounds(&p.folder, v, false, false);
            } else if let Some((saved, _)) = self.backend.bounds(&p.folder) {
                // Maximized: keep the normal bounds, mark it maximized.
                self.backend.save_bounds(&p.folder, saved, true, false);
            }
        }
    }

    /// `window-state:restoreBounds`.
    fn restore_bounds(&mut self, ctx: &egui::Context, folder: &std::path::Path) {
        let Some((bounds, maximized)) = self.backend.bounds(folder) else {
            return;
        };
        let num = |k: &str| bounds[k].as_f64().map(|v| v as f32);
        if let (Some(x), Some(y), Some(w), Some(h)) =
            (num("x"), num("y"), num("width"), num("height"))
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(x, y)));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(w, h)));
        }
        if maximized {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
        }
    }

    /// Markdown files dropped on the window become notes (`handleExternalDrop`).
    fn dropped_files(&mut self, ctx: &egui::Context) {
        if self.project.is_none() {
            return;
        }
        if self.editor_took_drop {
            return;
        }
        let pointer = ctx.input(|i| i.pointer.hover_pos());
        let over_editor = pointer.is_some_and(|p| self.editor.contains(p));
        let (dropped, hovering) = ctx.input(|i| {
            let paths: Vec<PathBuf> = i
                .raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("md")))
                .collect();
            (paths, !i.raw.hovered_files.is_empty())
        });
        if !dropped.is_empty() {
            self.backend.import_markdown(dropped);
        }
        if hovering && over_editor {
            views::drop_overlay(ctx, "Drop files to attach them to the note");
        } else if hovering {
            views::drop_overlay(ctx, "Drop Markdown files to add them as notes");
        }
    }

    // --- Dialogs -----------------------------------------------------------------

    fn overlays(&mut self, ctx: &egui::Context) {
        let Some(mut overlay) = self.overlay.take() else {
            return;
        };
        let top_inset = self.top_inset();
        let mut keep = true;
        match &mut overlay {
            Overlay::NewProject(form) => {
                let mut browse = false;
                match overlays::new_project(ctx, form, &mut browse) {
                    Some(FormAction::Close) => keep = false,
                    Some(FormAction::Submit) => match form.validate() {
                        Some(e) => form.error = Some(e.into()),
                        None => {
                            keep = false;
                            self.opening = true;
                            self.backend.new_project(
                                PathBuf::from(form.location.trim()),
                                form.name.trim().to_string(),
                                form.email.trim().to_string(),
                            );
                        }
                    },
                    None => {}
                }
                if browse {
                    if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                        form.location = folder.display().to_string();
                    }
                }
            }
            Overlay::Setup(form) => match overlays::setup(ctx, form) {
                Some(SetupAction::Cancel) => keep = false,
                Some(SetupAction::Setup(url)) => {
                    keep = false;
                    self.opening = true;
                    self.backend.init_project(PathBuf::from(&form.folder), url);
                }
                None => {}
            },
            Overlay::NewFile(form) => {
                let (entries, tags, current) = match &self.project {
                    Some(p) => (
                        p.entries.clone(),
                        model::all_tags(&p.index),
                        p.selected_entry().cloned(),
                    ),
                    None => (Vec::new(), Vec::new(), None),
                };
                match overlays::new_file(ctx, form, &entries, &tags, current.as_ref()) {
                    Some(FormAction::Close) => keep = false,
                    Some(FormAction::Submit) => {
                        keep = false;
                        let tags: Vec<String> = form.tags.iter().cloned().collect();
                        let name = form.name.trim().to_string();
                        match &form.duplicate_of {
                            Some(source) => self.backend.duplicate_file(
                                source.clone(),
                                name,
                                tags,
                                form.parent.clone(),
                            ),
                            None => self.backend.create_file(
                                name,
                                tags,
                                form.template.clone(),
                                form.parent.clone(),
                                form.deck,
                            ),
                        }
                    }
                    None => {}
                }
            }
            Overlay::MoveFile(form) => {
                let entries = self
                    .project
                    .as_ref()
                    .map(|p| p.entries.clone())
                    .unwrap_or_default();
                match overlays::move_file(ctx, form, &entries) {
                    Some(FormAction::Close) => keep = false,
                    Some(FormAction::Submit) => {
                        keep = false;
                        let changed = self.project.as_mut().is_some_and(|p| {
                            model::move_to(&mut p.index, &form.id, form.parent.as_deref())
                        });
                        if changed {
                            self.index_changed();
                        }
                    }
                    None => {}
                }
            }
            Overlay::DeleteFile { id, name } => match overlays::delete_file(ctx, name) {
                Some(Confirm::Yes) => {
                    keep = false;
                    // A pending edit to a note being deleted is dropped.
                    if self.editor.file_id() == Some(id.as_str()) {
                        let _ = self.editor.flush();
                    }
                    self.backend.delete_file(id.clone());
                }
                Some(Confirm::No) => keep = false,
                None => {}
            },
            Overlay::ClearTags { id, name, count } => match overlays::clear_tags(ctx, name, *count)
            {
                Some(Confirm::Yes) => {
                    keep = false;
                    if self
                        .project
                        .as_mut()
                        .is_some_and(|p| model::clear_tags(&mut p.index, id))
                    {
                        self.index_changed();
                    }
                }
                Some(Confirm::No) => keep = false,
                None => {}
            },
            Overlay::RemoveRecent(r) => match overlays::remove_recent(ctx, r) {
                Some(Confirm::Yes) => {
                    keep = false;
                    self.backend.remove_recent(&r.path);
                    self.recent = self.backend.recent();
                }
                Some(Confirm::No) => keep = false,
                None => {}
            },
            Overlay::ProjectSettings(form) => {
                match overlays::project_settings(ctx, form, top_inset) {
                    Some(ProjectSettingsAction::Close) => keep = false,
                    Some(ProjectSettingsAction::Save) => {
                        if form.validate() {
                            keep = false;
                            self.backend.set_git_config(
                                form.name.trim().to_string(),
                                form.email.trim().to_string(),
                            );
                        }
                    }
                    Some(ProjectSettingsAction::OpenFolder) => {
                        if let Err(e) = open::that_detached(&form.folder) {
                            self.toasts.error(format!("Couldn't open the folder: {e}"));
                        }
                    }
                    None => {}
                }
            }
            Overlay::Settings(state) => {
                for a in crate::overlays::settings::show(ctx, state, &mut self.theme, top_inset) {
                    match a {
                        SettingsAction::Close => keep = false,
                        // Applied already; eframe saves it with the theme.
                        SettingsAction::ThemeChanged => {}
                        SettingsAction::SetPrefs(patch) => {
                            self.backend.set_prefs(patch);
                            self.prefs = self.backend.paths().prefs();
                        }
                        SettingsAction::RefreshMcp => self.backend.mcp_status(),
                        SettingsAction::Restart => restart(ctx),
                    }
                }
            }
            Overlay::Sync(state) => match crate::overlays::sync::show(ctx, state, top_inset) {
                Some(SyncAction::Close) => {
                    keep = false;
                    self.backend.git_state();
                }
                Some(SyncAction::RefreshStatus) => {
                    state.status = None;
                    self.backend.git_state();
                }
                Some(SyncAction::Connect(url)) => {
                    state.start(Op::Connect, "Connecting...");
                    self.backend.git_op(Op::Connect, Some(url));
                }
                Some(SyncAction::Disconnect) => {
                    state.start(Op::Disconnect, "Disconnecting...");
                    self.backend.git_op(Op::Disconnect, None);
                }
                Some(SyncAction::Pull) => {
                    state.start(Op::Pull, "Pulling...");
                    self.backend.git_op(Op::Pull, None);
                }
                Some(SyncAction::PullRebase) => {
                    state.start(Op::PullRebase, "Pulling & rebasing...");
                    self.backend.git_op(Op::PullRebase, None);
                }
                Some(SyncAction::Push) => {
                    state.start(Op::Push, "Pushing...");
                    self.backend.git_op(Op::Push, None);
                }
                Some(SyncAction::Reset) => {
                    state.start(Op::Reset, "Resetting...");
                    self.backend.git_op(Op::Reset, None);
                }
                None => {}
            },
            Overlay::Help(state) => {
                if crate::overlays::help::show(ctx, state, top_inset) {
                    keep = false;
                }
            }
            Overlay::About => {
                let modal = AboutModal::new("NoteLiner", env!("CARGO_PKG_VERSION"))
                    .description(DESCRIPTION)
                    .repo(REPO_URL, Some("github.com/NathanLaan/marina"))
                    .icon(AboutIcon::Glyph(icons::FILE_LINES))
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
            Overlay::Busy { title, message } => overlays::busy(ctx, title, message),
            Overlay::TagEditor(state) => {
                let index = self.project.as_ref().map(|p| p.index.clone());
                match index.and_then(|i| overlays::tags::show(ctx, state, &i, top_inset)) {
                    Some(overlays::tags::TagEditorAction::Close) => keep = false,
                    Some(overlays::tags::TagEditorAction::SetColor { tag, color }) => {
                        if let Some(p) = &mut self.project {
                            model::set_tag_color(&mut p.index, &tag, color.as_deref());
                        }
                        self.index_changed();
                    }
                    None => {
                        if self.project.is_none() {
                            keep = false;
                        }
                    }
                }
            }
            Overlay::PresentationSettings {
                id,
                filename,
                dialog,
            } => {
                let slides = self.deck.deck().map_or(0, |d| d.slides.len());
                match dialog.show(ctx, slides) {
                    SettingsOutcome::Open => {}
                    SettingsOutcome::Cancel => keep = false,
                    SettingsOutcome::Save(presentation) => {
                        dialog.error = None;
                        dialog.saving = true;
                        self.backend.set_presentation(
                            id.clone(),
                            filename.clone(),
                            presentation.to_json(),
                        );
                    }
                }
            }
            Overlay::RemoveAttachment {
                file_id,
                attachment,
            } => match overlays::remove_attachment(ctx, &attachment.original_name) {
                Some(Confirm::Yes) => {
                    keep = false;
                    self.backend
                        .remove_attachment(file_id.clone(), attachment.id.clone());
                }
                Some(Confirm::No) => keep = false,
                None => {}
            },
        }
        if keep && self.overlay.is_none() {
            self.overlay = Some(overlay);
        }
    }

    /// The head of the MCP confirm queue, over everything else.
    fn mcp_confirms(&mut self, ctx: &egui::Context) {
        let Some(req) = self.mcp_queue.front() else {
            return;
        };
        if let Some(decision) = overlays::mcp::confirm(ctx, req, &mut self.mcp_details) {
            self.backend.respond_confirm(req.id, decision);
            #[cfg(test)]
            self.mcp_decisions.push((req.id, decision));
            self.mcp_queue.pop_front();
            self.mcp_details = false;
        }
    }

    /// The command palette, with notes, tags and recent projects as extras
    /// (`paletteExtras`).
    fn palette(&mut self, ctx: &egui::Context) {
        let Some(mut palette) = self.palette.take() else {
            return;
        };
        let view = View {
            modal_open: false,
            ..self.view()
        };
        let project = self.project.as_ref();
        let recent = &self.recent;
        let mut extra = |q: &str| -> Vec<PaletteItem<Act>> {
            let mut out = Vec::new();
            if let (false, Some(p)) = (q.is_empty(), project) {
                for tag in model::all_tags(&p.index) {
                    let score = marina_egui::commands::fuzzy_score(&tag, q);
                    if score == 0 {
                        continue;
                    }
                    let mut files: Vec<&Entry> =
                        p.entries.iter().filter(|e| e.tags.contains(&tag)).collect();
                    files.sort_by(|a, b| a.name.cmp(&b.name));
                    let n = files.len();
                    let Some(first) = files.first() else { continue };
                    out.push(PaletteItem {
                        kind: ItemKind::Tag,
                        id: format!("tag:{tag}"),
                        label: format!("#{tag}"),
                        sub: format!("{n} note{}", if n == 1 { "" } else { "s" }),
                        section: "Tag".into(),
                        score,
                        action: Act::OpenNote(first.id.clone()),
                    });
                }
                for e in &p.entries {
                    let score = marina_egui::commands::fuzzy_score(&e.name, q);
                    if score == 0 {
                        continue;
                    }
                    out.push(PaletteItem {
                        kind: ItemKind::Note,
                        id: format!("note:{}", e.id),
                        label: e.name.clone(),
                        sub: e.filename.clone(),
                        section: "Note".into(),
                        score,
                        action: Act::OpenNote(e.id.clone()),
                    });
                }
            }
            if project.is_none() || !q.is_empty() {
                for r in recent {
                    let score = marina_egui::commands::fuzzy_score(&r.name, q);
                    if !q.is_empty() && score == 0 {
                        continue;
                    }
                    out.push(PaletteItem {
                        kind: ItemKind::Recent,
                        id: format!("rp:{}", r.path),
                        label: r.name.clone(),
                        sub: short_path(&r.path),
                        section: "Recent".into(),
                        score: score + if q.is_empty() { 200 } else { 0 },
                        action: Act::OpenRecent(r.path.clone()),
                    });
                }
            }
            out
        };
        match palette.show(ctx, &mut self.registry, &view, &mut extra) {
            PaletteOutcome::Open => self.palette = Some(palette),
            PaletteOutcome::Closed => self.save_recents(),
            PaletteOutcome::Run(act) => {
                self.save_recents();
                self.apply(ctx, act);
            }
        }
    }

    /// `commandRecents` in the UI prefs (shared with the Svelte builds).
    fn save_recents(&mut self) {
        let ids = self.registry.recents().ids;
        if ids != self.prefs.command_recents {
            self.prefs.command_recents = ids.clone();
            self.backend.set_prefs(json!({ "commandRecents": ids }));
        }
    }

    pub fn save_state(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, THEME_KEY, &self.theme);
    }

    /// On quit: write the pending edit and the layout, flush the push.
    pub fn shutdown(&mut self) {
        if let Some(save) = self.editor.flush() {
            if let Err(e) = self.backend.write_file_now(&save.filename, &save.content) {
                log::warn!("couldn't save {}: {e}", save.filename);
            }
        }
        if let Some(p) = &self.project {
            self.backend.save_layout(&p.folder, p.layout.to_value());
        }
        self.backend.shutdown();
    }
}

impl eframe::App for NoteLinerApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.save_state(storage);
        // eframe 0.37 restores its saved window (`"window"`) even with
        // `persist_window: false`; it only stops writing it. Blank it so a
        // copy saved by an earlier build (e.g. full screen from presenting)
        // can't come back: an unparsable value loads as no settings.
        storage.set_string(EFRAME_WINDOW_KEY, String::new());
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.shutdown();
    }
}

/// Keeps a resizable panel's size in the layout once the drag settles.
fn remember_size(stored: &mut f32, actual: f32) {
    if actual > 0.0 && (actual - *stored).abs() > 0.5 {
        *stored = actual.round();
    }
}

fn remove_key(index: &mut Value, id: &str, key: &str) {
    if let Some(files) = index["files"].as_array_mut() {
        if let Some(f) = files.iter_mut().find(|f| f["id"] == id) {
            if let Some(m) = f.as_object_mut() {
                m.remove(key);
            }
        }
    }
}

/// `shortPath`: `.../parent/name` for long paths.
fn short_path(p: &str) -> String {
    let parts: Vec<&str> = p.split(['/', '\\']).filter(|s| !s.is_empty()).collect();
    if parts.len() <= 2 {
        return p.to_string();
    }
    format!(".../{}", parts[parts.len() - 2..].join("/"))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_paths() {
        assert_eq!(short_path("/home/u/docs/notes"), ".../docs/notes");
        assert_eq!(short_path("/notes"), "/notes");
    }
}
