//! The app: owns the backend, the open project, the editor, the panels and
//! the open dialog; routes backend events, commands and view actions.
//!
//! A project is a folder. The tree in the Files pane is its sub-folders,
//! with the notes and diagrams in them, and the backend holds the index that
//! counts: the copy here is replaced whenever the folders change.

use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;

use egui::{Frame, Ui};
use laankit_editor::attachment_markdown;
use laankit_egui::about::{AboutAction, AboutIcon, AboutModal, UpdateStatus};
use laankit_egui::commands::{Recents, Registry};
use laankit_egui::diagnostics::{self, Screenshot};
use laankit_egui::dialog::Confirm;
use laankit_egui::file_dialog::FileDialogs;
use laankit_egui::palette::{CommandPalette, ItemKind, PaletteItem, PaletteOutcome};
use laankit_egui::tabs::{self, TabAction, TabItem};
use laankit_egui::theme::{self, ThemeState};
use laankit_egui::titlebar::{self, TitleBar, TitleBarAction};
use laankit_egui::toast::Toasts;
use laankit_egui::toolbar::{self, ToolbarButton};
use laankit_egui::{fonts, icons, window};
use noteliner_slides::{
    normalize_presentation, Direction, PresentationSettings, PresenterAction, SettingsOutcome,
    SlideEdit,
};
use serde_json::{json, Value};

use crate::backend::{self, Backend, DeleteTarget, Event, ExportKind, McpConfirm};
use crate::commands::{self, Act, View};
use crate::config::{LastFilePosition, Layout, Paths, Prefs, Recent};
use crate::deck::{self, DeckState, SlideImages};
use blockliner_egui::dialogs::colour::{self as colour_dialog, ColourAction};
use blockliner_egui::dialogs::export::{self as export_dialog, ExportAction, ExportState};
use blockliner_egui::export as diagram_export;
use blockliner_egui::properties::PropertiesProps;
use blockliner_egui::EditAct;

use crate::diagram::DiagramDoc;
use crate::editor::{EditorAction, NoteEditor};
use crate::model::{self, Entry, SortMode};
use crate::overlays::settings::{SettingsAction, SettingsState};
use crate::overlays::{
    self, ConflictAction, FormAction, MoveForm, MoveTarget, NewFileForm, NewFolderForm, Overlay,
};
use crate::spell;
use crate::tabs::{NoteStash, Stashed, Tabs};
use crate::views::attachments::{self, AttachmentAction};
use crate::views::diagram::{self as diagram_view, ToolbarAction as DiagramToolbarAction};
use crate::views::panes::{PaneAction, PaneStates};
use crate::views::preview::{DeckProps, Preview, PreviewAction};
use crate::views::sidebar::{self, SidebarAction, SidebarProps};
use crate::views::tree::{ContextAction, FolderAction, Item, TreeAction, TreeState};
use crate::views::{self, LogLine, OpenAction, StatusProps};

/// eframe storage keys (the Svelte build's `noteliner-theme` /
/// `noteliner-scale` localStorage keys).
const THEME_KEY: &str = "noteliner-theme";
/// Ctrl+wheel zoom steps at most once per 200 ms (`ZOOM_COOLDOWN_MS`).
const ZOOM_COOLDOWN_SECS: f64 = 0.2;
/// Window bounds are saved after a second of quiet (`bounds_saver`).
const BOUNDS_DEBOUNCE_SECS: f64 = 1.0;
/// How often the project folder is checked for changes made by other
/// programs, while the window has the focus.
const CHECK_SECS: f64 = 3.0;
/// eframe's own window-geometry key, which NoteLiner keeps blank (see `save`).
const EFRAME_WINDOW_KEY: &str = "window";
const DESCRIPTION: &str = "An outliner-style note-taking application built with Rust and egui.";
const REPO_URL: &str = "https://github.com/NathanLaan/laankit/";

/// `LAANKIT_OPEN` / `LAANKIT_PROJECT` / `LAANKIT_FILE`: what to open at
/// launch, for screenshots and tests.
#[derive(Default)]
struct Diagnostics {
    open: Option<String>,
    project: Option<PathBuf>,
    /// The nth note in display order, or a note name.
    file: Option<String>,
}

/// The open project.
struct Project {
    folder: PathBuf,
    /// `noteliner.json` as the backend gave it. Tag changes are made here
    /// and sent back; everything else comes from the backend.
    index: Value,
    entries: Vec<Entry>,
    /// Every folder under the project, by path.
    folders: Vec<String>,
    /// The folders that are open in the Files pane.
    open_folders: HashSet<String>,
    /// The open note or diagram: the active tab.
    selected: Option<String>,
    /// The folder selected in the tree. It takes the highlight from the open
    /// note, and new files and folders go in it.
    selected_folder: Option<String>,
    layout: Layout,
    /// The layout as last saved, to notice changes.
    saved_layout: Layout,
    /// Tag changes sent to the backend and not yet answered: until they
    /// are, the tags here are newer than the backend's.
    pending_index_saves: usize,
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

    /// The index entry of `id`, as the backend has it.
    fn entry_value(&self, id: &str) -> Option<&Value> {
        self.index["files"]
            .as_array()?
            .iter()
            .find(|f| f["id"].as_str() == Some(id))
    }

    /// The open note's attachments: `_attachments/` in its own folder.
    fn attachments_dir(&self) -> PathBuf {
        let folder = self.selected_entry().map_or("", |e| e.folder.as_str());
        self.folder
            .join(folder)
            .join(noteliner_core::project::ATTACHMENTS_DIR)
    }

    /// Where a new file or folder goes: the selected folder, or beside the
    /// selected note, or the top of the project with nothing selected.
    fn target_folder(&self) -> String {
        match &self.selected_folder {
            Some(folder) => folder.clone(),
            None => self
                .selected_entry()
                .map(|e| e.folder.clone())
                .unwrap_or_default(),
        }
    }

    /// Opens `folder` and the folders above it, so what is in it shows.
    fn reveal_folder(&mut self, folder: &str) {
        let mut path = folder;
        while !path.is_empty() {
            self.open_folders.insert(path.to_string());
            path = noteliner_core::tree::folder_of(path);
        }
    }

    /// Lists `folder` and the folders above it, until the backend's list
    /// arrives (a note was made in a folder that is new).
    fn note_folder(&mut self, folder: &str) {
        let mut path = folder;
        while !path.is_empty() {
            if !self.folders.iter().any(|f| f == path) {
                self.folders.push(path.to_string());
            }
            path = noteliner_core::tree::folder_of(path);
        }
        self.folders.sort();
    }
}

/// A file dialog's answer, and what it's for.
enum Picked {
    Folder(PathBuf),
    Import(PathBuf),
    /// Files to attach to the note `file_id`.
    Attachments {
        file_id: String,
        paths: Vec<PathBuf>,
    },
}

pub struct NoteLinerApp {
    backend: Backend,
    theme: ThemeState,
    prefs: Prefs,
    /// Whether the window was built undecorated, with LaanKit's title bar.
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
    /// The diagram editor: the active tab's diagram, when it is one.
    diagram: DiagramDoc,
    /// Open the Export dialog once this diagram has loaded.
    pending_diagram_export: Option<String>,
    /// Where Export Diagram writes, in tests (the Downloads folder otherwise).
    #[cfg(test)]
    diagram_export_dir: Option<PathBuf>,
    /// The open tabs; the active one is the project's selected file.
    tabs: Tabs,
    /// The open deck and the slides widgets.
    deck: DeckState,
    /// A deck picked with Present in the FILES pane that is still loading:
    /// its id, and whether the speaker view was asked for.
    present_when_open: Option<(String, bool)>,
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
    /// When the project folder was last checked for outside changes, and
    /// whether the window had the focus then.
    last_check: f64,
    was_focused: bool,
    /// An open note that changed on disk under unsaved edits, waiting for
    /// the dialog that is open to close: (id, the file on disk).
    pending_conflict: Option<(String, String)>,
    /// The time of the last frame, for edits made by tests.
    #[cfg(test)]
    test_time: f64,
    mcp_running: bool,
    mcp_queue: VecDeque<McpConfirm>,
    overlay: Option<Overlay>,
    palette: Option<CommandPalette>,
    registry: Registry<Act, View>,
    toasts: Toasts,
    /// The file and folder dialogs, off the UI thread where they would
    /// freeze it.
    dialogs: FileDialogs<Picked>,
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
        let diagram = DiagramDoc::new(&prefs);
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
            pending_line: None,
            pending_position: None,
            mcp_details: false,
            #[cfg(test)]
            mcp_decisions: Vec::new(),
            editor: NoteEditor::default(),
            diagram,
            pending_diagram_export: None,
            #[cfg(test)]
            diagram_export_dir: None,
            tabs: Tabs::default(),
            deck: DeckState::default(),
            present_when_open: None,
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
            last_check: 0.0,
            was_focused: true,
            pending_conflict: None,
            #[cfg(test)]
            test_time: 0.0,
            mcp_running: false,
            mcp_queue: VecDeque::new(),
            overlay: None,
            palette: None,
            registry,
            toasts: Toasts::default(),
            dialogs: FileDialogs::new(),
            update: UpdateStatus::Idle,
            last_wheel_zoom: f64::NEG_INFINITY,
            bounds: None,
            bounds_changed_at: None,
            screenshot: Screenshot::from_env(),
            diagnostics: Diagnostics {
                open: env("LAANKIT_OPEN"),
                project: env("LAANKIT_PROJECT").map(PathBuf::from),
                file: env("LAANKIT_FILE"),
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
        self.dialogs.attach(ctx);
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

    /// The project's folders, by path.
    #[cfg(test)]
    pub fn folders(&self) -> Vec<String> {
        self.project
            .as_ref()
            .map(|p| p.folders.clone())
            .unwrap_or_default()
    }

    /// The folder selected in the tree.
    #[cfg(test)]
    pub fn selected_folder(&self) -> Option<String> {
        self.project.as_ref()?.selected_folder.clone()
    }

    /// The folders that are open in the tree, sorted.
    #[cfg(test)]
    pub fn open_folders(&self) -> Vec<String> {
        let mut open: Vec<String> = self
            .project
            .as_ref()
            .map(|p| p.open_folders.iter().cloned().collect())
            .unwrap_or_default();
        open.sort();
        open
    }

    /// The open note's file as the editor holds it, frontmatter included.
    #[cfg(test)]
    pub fn editor_file_text(&self) -> String {
        self.editor.file_text().to_string()
    }

    #[cfg(test)]
    pub fn editor_status(&self) -> crate::editor::SaveStatus {
        self.editor.status()
    }

    /// Replaces the open note's whole file as if it had been typed: the
    /// note is unsaved, and the autosave follows.
    #[cfg(test)]
    pub fn edit_file_for_test(&mut self, text: &str) {
        let now = self.editor_time();
        self.editor.type_file(text, now);
    }

    #[cfg(test)]
    fn editor_time(&self) -> f64 {
        self.test_time
    }

    /// Looks for changes made outside the app now, as the timer would.
    #[cfg(test)]
    pub fn check_now(&mut self) {
        self.check(false);
    }

    /// The diagram on the canvas: the open diagram's id, if one is loaded.
    #[cfg(test)]
    pub fn diagram_file_id(&self) -> Option<String> {
        self.diagram.file_id().map(str::to_string)
    }

    #[cfg(test)]
    pub fn diagram_editor(&self) -> &blockliner_egui::DiagramEditor {
        &self.diagram.editor
    }

    /// Export Diagram writes here instead of the Downloads folder.
    #[cfg(test)]
    pub fn set_diagram_export_dir(&mut self, dir: PathBuf) {
        self.diagram_export_dir = Some(dir);
    }

    /// The open tabs' file ids, in tab order.
    #[cfg(test)]
    pub fn tab_ids(&self) -> Vec<String> {
        self.tabs.order().to_vec()
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

    /// Where character `c` of the open file is on screen.
    #[cfg(test)]
    pub fn editor_char_rect(&mut self, c: usize) -> Option<egui::Rect> {
        self.editor.char_rect(c)
    }

    /// The caret's line in the note's text, the frontmatter not counted.
    #[cfg(test)]
    pub fn editor_body_line(&self) -> usize {
        self.editor.body_line()
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

    /// The slide on screen while presenting (1-based).
    #[cfg(test)]
    pub fn presenting_slide(&self) -> Option<usize> {
        self.deck.presenter.as_ref().map(|p| p.index())
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
            folder_selected: p.is_some_and(|p| p.selected_folder.is_some()),
            is_deck: self.is_deck(),
            tags_visible: p.is_some_and(|p| p.layout.show_tags),
            tabs: self.tabs.len(),
            is_diagram: self.diagram_active(),
            diagram_has_content: self.diagram_has_content(),
            diagram: self.diagram.editor.view(),
            typing: false,
            modal_open: self.overlay.is_some()
                || self.palette.is_some()
                || !self.mcp_queue.is_empty(),
        }
    }

    /// The open note has a `presentation:` block in its frontmatter.
    fn is_deck(&self) -> bool {
        let selected = self.project.as_ref().and_then(|p| p.selected.as_deref());
        selected.is_some() && selected == self.editor.file_id() && self.editor.is_deck()
    }

    /// The active tab is a diagram, loaded on the canvas.
    fn diagram_active(&self) -> bool {
        let selected = self.project.as_ref().and_then(|p| p.selected.as_deref());
        selected.is_some() && selected == self.diagram.file_id()
    }

    fn diagram_has_content(&self) -> bool {
        let d = &self.diagram.editor.diagram;
        self.diagram_active() && !(d.shapes.is_empty() && d.connectors.is_empty())
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
                folders,
                truncated,
            } => self.project_opened(ctx, folder, index, folders, truncated),
            Event::OpenFailed(message) => {
                self.opening = false;
                self.toasts
                    .error(format!("Couldn't open the project: {message}"));
            }
            Event::ProjectClosed => {
                self.project = None;
                self.editor.clear();
                self.diagram.clear();
                self.tabs.clear();
                self.hidden_tags.clear();
                self.panes = PaneStates::default();
                self.pending_line = None;
                self.pending_position = None;
                self.pending_conflict = None;
                self.recent = self.backend.recent();
                if matches!(self.overlay, Some(Overlay::Busy { .. })) {
                    self.overlay = None;
                }
                self.backend.mcp_status();
            }
            Event::FileLoaded { id, content } => {
                let Some(p) = &mut self.project else { return };
                if p.selected.as_deref() != Some(id.as_str()) {
                    return;
                }
                // A diagram goes on the canvas; one that can't be read closes.
                if let Some(e) = p.entry(&id).filter(|e| e.is_diagram()) {
                    self.pending_line = None;
                    match self.diagram.load(&id, &e.name, &e.filename, &content) {
                        Ok(()) => {
                            if self.pending_diagram_export.as_deref() == Some(id.as_str()) {
                                self.pending_diagram_export = None;
                                self.open_diagram_export();
                            }
                        }
                        Err(message) => {
                            self.toasts
                                .error(format!("Couldn't open “{}”: {message}", e.name));
                            self.close_tab(&id);
                        }
                    }
                    self.open_from_env(ctx);
                    return;
                }
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
                result,
                entry,
            } => {
                let ok = result.is_ok();
                self.editor.saved(&id, &content, ok);
                for e in self.tabs.editors_mut() {
                    e.saved(&id, &content, ok);
                }
                self.diagram.saved(&id);
                for d in self.tabs.diagrams_mut() {
                    d.saved(&id);
                }
                // A field the save put back into the frontmatter shows in the
                // editor, unless the block was typed in since.
                if let Ok(written) = &result {
                    if let Some(rewritten) = &written.rewritten {
                        if self.editor.file_id() == Some(id.as_str()) {
                            self.editor.frontmatter_rewritten(&content, rewritten);
                        }
                        for e in self.tabs.editors_mut() {
                            if e.file_id() == Some(id.as_str()) {
                                e.frontmatter_rewritten(&content, rewritten);
                            }
                        }
                    }
                }
                if let (Some(p), Some(entry)) = (&mut self.project, entry) {
                    model::update_file(
                        &mut p.index,
                        &id,
                        &json!({ "modifiedAt": entry["modifiedAt"], "deck": entry["deck"] }),
                    );
                    if entry.get("deck").is_none() {
                        remove_key(&mut p.index, &id, "deck");
                    }
                    // Tags typed in the frontmatter are the note's tags. A tag
                    // change of the app's own that isn't saved yet is newer.
                    if p.pending_index_saves == 0 && entry["tags"].is_array() {
                        model::update_file(&mut p.index, &id, &json!({ "tags": entry["tags"] }));
                        for tag in entry["tags"].as_array().into_iter().flatten() {
                            if let Some(tag) = tag.as_str() {
                                model::add_to_palette(&mut p.index, tag);
                            }
                        }
                    }
                    p.refresh();
                }
                if let Err(e) = result {
                    self.toasts.error(format!("Couldn't save: {e}"));
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
                    for t in self.tabs.editors_mut() {
                        t.renamed(&id, &e.name, &e.filename);
                    }
                    self.diagram.renamed(&id, &e.name, &e.filename);
                    for d in self.tabs.diagrams_mut() {
                        d.renamed(&id, &e.name, &e.filename);
                    }
                }
            }
            Event::Deleted(id) => {
                let Some(p) = &mut self.project else { return };
                model::remove_file(&mut p.index, &id);
                p.refresh();
                self.forget_tab(&id);
            }
            Event::DeleteFailed { target, message } => {
                let name = self.project.as_ref().map(|p| match &target {
                    DeleteTarget::File(id) => {
                        p.entry(id).map(|e| e.name.clone()).unwrap_or_default()
                    }
                    DeleteTarget::Folder(path) => model::folder_name(path).to_string(),
                });
                if let Some(name) = name {
                    self.overlay = Some(Overlay::DeletePermanently {
                        target,
                        name,
                        message,
                    });
                }
            }
            Event::Tree { index, folders } => self.tree_changed(index, folders),
            Event::FolderCreated(path) => {
                if let Some(p) = &mut self.project {
                    p.reveal_folder(noteliner_core::tree::folder_of(&path));
                    p.selected_folder = Some(path);
                }
            }
            Event::FolderMoved { from, to } => {
                let Some(p) = &mut self.project else { return };
                let moved = |path: &str| format!("{to}{}", &path[from.len()..]);
                let under = |path: &str| noteliner_core::tree::is_under(path, &from);
                p.open_folders = p
                    .open_folders
                    .iter()
                    .map(|f| if under(f) { moved(f) } else { f.clone() })
                    .collect();
                if let Some(f) = p.selected_folder.as_ref().filter(|f| under(f)) {
                    p.selected_folder = Some(moved(f));
                }
                // The folder it was moved into opens, to show it.
                p.reveal_folder(noteliner_core::tree::folder_of(&to));
            }
            Event::FileChanged { id, content } => self.file_changed(id, content),
            Event::McpChanged => self.check(true),
            Event::IndexSaved(result) => {
                if let Some(p) = &mut self.project {
                    p.pending_index_saves = p.pending_index_saves.saturating_sub(1);
                }
                if let Err(e) = result {
                    self.toasts.error(format!("Couldn't save the tags: {e}"));
                }
            }
            Event::Presentation { id, outcome } => match outcome {
                Ok(presentation) => {
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
                    // The block in the open editor follows the file.
                    self.presentation_into_editors(&id, &presentation);
                    let Some(p) = &mut self.project else { return };
                    if presentation.is_null() {
                        remove_key(&mut p.index, &id, "deck");
                    } else {
                        model::update_file(&mut p.index, &id, &json!({ "deck": true }));
                    }
                    // Converting to a note hides the SLIDES pane.
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
                Err(e) => {
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
                        ExportKind::Mermaid => "Mermaid",
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

    /// A folder was opened: the index, the folders, the saved layout and
    /// window bounds.
    fn project_opened(
        &mut self,
        ctx: &egui::Context,
        folder: PathBuf,
        mut index: Value,
        folders: Vec<String>,
        truncated: bool,
    ) {
        self.opening = false;
        if matches!(self.overlay, Some(Overlay::Busy { .. })) {
            self.overlay = None;
        }
        model::normalize_index(&mut index);
        let layout = Layout::from_saved(&self.backend.layout(&folder));
        self.backend.add_recent(&folder);
        self.recent = self.backend.recent();
        self.restore_bounds(ctx, &folder);
        let open_folders = layout
            .open_folders
            .iter()
            .filter(|f| folders.contains(f))
            .cloned()
            .collect();
        let mut project = Project {
            folder,
            index,
            entries: Vec::new(),
            folders,
            open_folders,
            selected: None,
            selected_folder: None,
            saved_layout: layout.clone(),
            layout,
            pending_index_saves: 0,
        };
        project.refresh();
        // The tabs that were open, less the files that are gone.
        let open: Vec<String> = project
            .layout
            .open_tabs
            .iter()
            .filter(|id| project.entry(id).is_some())
            .cloned()
            .collect();
        let active = Some(project.layout.active_tab.clone())
            .filter(|id| open.contains(id))
            .or_else(|| open.first().cloned());
        let last = project
            .layout
            .last_file_position
            .clone()
            .filter(|pos| project.entry(&pos.file_id).is_some())
            // A layout saved before tabs has no tab list: the one open note.
            .filter(|pos| open.is_empty() || active.as_deref() == Some(pos.file_id.as_str()));
        self.project = Some(project);
        self.editor.clear();
        self.diagram.clear();
        self.tabs.restore(open);
        self.pending_position = None;
        self.pending_conflict = None;
        self.last_check = ctx.input(|i| i.time);
        // Reopen the note that was open, where it was left.
        if let Some(pos) = last {
            let id = pos.file_id.clone();
            self.pending_position = Some(pos);
            self.select(id);
        } else if let Some(id) = active {
            self.select(id);
        }
        self.backend.mcp_status();
        if truncated {
            let message = format!(
                "This folder has more than {} files. NoteLiner stopped reading it, so some notes are missing.",
                noteliner_core::tree::MAX_FILES
            );
            self.log_line(message.clone());
            self.toasts.error(message);
        }
        self.open_from_env(ctx);
    }

    /// The index and folders changed in the backend: a folder was added,
    /// renamed, moved or deleted, a note was moved, another program changed
    /// the folder, or an AI assistant wrote to the project. Open notes
    /// follow their files; a tab whose file is gone closes.
    fn tree_changed(&mut self, mut index: Value, folders: Vec<String>) {
        let Some(p) = &mut self.project else { return };
        model::normalize_index(&mut index);
        // Tag changes on their way to the backend are newer than its tags.
        if p.pending_index_saves > 0 {
            for key in ["tagMeta", "tagPalette"] {
                index[key] = p.index[key].clone();
            }
            if let Some(files) = index["files"].as_array_mut() {
                for f in files {
                    let mine = f["id"].as_str().and_then(|id| p.entry_value(id));
                    if let Some(mine) = mine {
                        f["tags"] = mine["tags"].clone();
                    }
                }
            }
        }
        let before = std::mem::replace(&mut p.index, index);
        p.folders = folders;
        p.refresh();
        let known = p.folders.clone();
        p.open_folders.retain(|f| known.contains(f));
        if p.selected_folder
            .as_ref()
            .is_some_and(|f| !known.contains(f))
        {
            p.selected_folder = None;
        }
        // Open tabs follow a renamed or moved file.
        let mut gone = Vec::new();
        for id in self.tabs.order().to_vec() {
            match p.entry(&id) {
                Some(e) => {
                    self.editor.renamed(&id, &e.name, &e.filename);
                    for t in self.tabs.editors_mut() {
                        t.renamed(&id, &e.name, &e.filename);
                    }
                    self.diagram.renamed(&id, &e.name, &e.filename);
                    for d in self.tabs.diagrams_mut() {
                        d.renamed(&id, &e.name, &e.filename);
                    }
                }
                None => gone.push(id),
            }
        }
        self.link_key = None;
        for id in gone {
            // A removed note with unsaved edits is written again, so nothing
            // typed is lost; any other tab of a removed file closes.
            let pending = if self.editor.file_id() == Some(id.as_str()) {
                self.editor.flush()
            } else if self.diagram.file_id() == Some(id.as_str()) {
                self.diagram.flush()
            } else {
                None
            };
            let entry = before["files"]
                .as_array()
                .and_then(|fs| fs.iter().find(|f| f["id"].as_str() == Some(id.as_str())))
                .cloned();
            match (pending, entry) {
                (Some(save), Some(entry)) => {
                    self.toasts.info(format!(
                        "“{}” was removed by another program. Your unsaved edits were saved to it again.",
                        entry["name"].as_str().unwrap_or_default()
                    ));
                    if let Some(p) = &mut self.project {
                        model::add_file(&mut p.index, entry.clone());
                        p.refresh();
                    }
                    self.backend.write_file_restoring(
                        save.id,
                        save.filename,
                        save.content,
                        Some(entry),
                    );
                }
                _ => self.forget_tab(&id),
            }
        }
    }

    /// Closes the tab of a file that is gone, without writing it. The active
    /// tab's neighbour is shown in its place.
    fn forget_tab(&mut self, id: &str) {
        let Some(p) = &mut self.project else { return };
        let active = p.selected.as_deref() == Some(id);
        if active {
            p.selected = None;
            self.editor.clear();
            self.diagram.clear();
        }
        let next = self.tabs.close(id);
        if let Some(next) = next.filter(|_| active) {
            self.select(next);
        }
    }

    /// An open file was changed on disk by another program (or by an AI
    /// assistant). With no unsaved edits it reloads. With unsaved edits they
    /// stay, and a dialog offers Reload and Keep Mine.
    fn file_changed(&mut self, id: String, content: String) {
        let Some(p) = &self.project else { return };
        let Some(entry) = p.entry(&id).cloned() else {
            return;
        };
        if self.diagram.file_id() == Some(id.as_str()) {
            // A diagram being edited keeps the user's version.
            if self.diagram.is_dirty() {
                self.log_line(format!(
                    "“{}” changed on disk while it was being edited. Your version was kept.",
                    entry.name
                ));
            } else if let Err(e) = self
                .diagram
                .load(&id, &entry.name, &entry.filename, &content)
            {
                self.toasts
                    .error(format!("Couldn't reload “{}”: {e}", entry.name));
            }
            return;
        }
        if self.editor.file_id() == Some(id.as_str()) {
            if self.editor.file_text() == content {
                return;
            }
            if !self.editor.is_dirty() {
                self.editor.replace(&id, content);
                return;
            }
            // Nothing is written over the change until the user has chosen.
            self.editor.hold(true);
            self.pending_conflict = Some((id, content));
            return;
        }
        // A background tab has no unsaved edits: a note takes the file, and
        // a diagram is read again when its tab is shown.
        let mut taken = false;
        for e in self.tabs.editors_mut() {
            if e.file_id() == Some(id.as_str()) {
                e.replace(&id, content.clone());
                taken = true;
            }
        }
        if !taken {
            self.tabs.take(&id);
        }
    }

    /// Looks for changes made by other programs: in the folders, and in the
    /// files of the open tabs.
    fn check(&mut self, tree: bool) {
        let Some(p) = &self.project else { return };
        let open: Vec<(String, String)> = self
            .tabs
            .order()
            .iter()
            .filter_map(|id| p.entry(id).map(|e| (e.id.clone(), e.filename.clone())))
            .collect();
        self.backend.check(open, tree);
    }

    /// The check runs while the window has the focus, every few seconds, and
    /// when the window regains the focus.
    fn watch(&mut self, ctx: &egui::Context) {
        if self.project.is_none() || self.opening {
            return;
        }
        let (focused, now) = ctx.input(|i| (i.focused, i.time));
        let regained = focused && !self.was_focused;
        self.was_focused = focused;
        if !focused {
            return;
        }
        if regained || now - self.last_check >= CHECK_SECS {
            self.last_check = now;
            self.check(false);
        }
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(CHECK_SECS));
    }

    /// Sets a note's `presentation:` block in the editor that has the note
    /// open, as the backend set it in the file.
    fn presentation_into_editors(&mut self, id: &str, presentation: &Value) {
        if self.editor.file_id() == Some(id) {
            self.editor.set_presentation(presentation);
        }
        for e in self.tabs.editors_mut() {
            if e.file_id() == Some(id) {
                e.set_presentation(presentation);
            }
        }
    }

    /// Writes the index's tags into the frontmatter of the notes that are
    /// open, as the backend writes them into the files. Not an undo step,
    /// and the note isn't marked as edited.
    fn tags_into_editors(&mut self) {
        if !self.prefs.write_frontmatter {
            return;
        }
        let Some(p) = &self.project else { return };
        if let Some(entry) = self.editor.file_id().and_then(|id| p.entry_value(id)) {
            let entry = entry.clone();
            self.editor.retag(&entry);
        }
        for e in self.tabs.editors_mut() {
            if let Some(entry) = e.file_id().and_then(|id| p.entry_value(id)) {
                let entry = entry.clone();
                e.retag(&entry);
            }
        }
    }

    /// A note or diagram was made: it joins the tree, its folder opens, and
    /// it is shown.
    fn created(&mut self, entry: Value) {
        let Some(p) = &mut self.project else { return };
        let id = entry["id"].as_str().unwrap_or_default().to_string();
        if p.entry(&id).is_none() {
            model::add_file(&mut p.index, entry);
        } else {
            model::update_file(&mut p.index, &id, &entry);
        }
        p.refresh();
        if let Some(folder) = p.entry(&id).map(|e| e.folder.clone()) {
            p.note_folder(&folder);
            p.reveal_folder(&folder);
        }
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
            Err(e) => self.log_line(format!("Import failed: {e}")),
        }
    }

    // --- Projects and notes -------------------------------------------------------

    fn open_folder(&mut self, folder: PathBuf) {
        self.opening = true;
        self.backend.open_project(folder);
    }

    /// `handleOpenFolder`.
    fn pick_folder(&mut self) {
        self.dialogs.start(|| {
            rfd::FileDialog::new()
                .set_title("Open Folder")
                .pick_folder()
                .map(Picked::Folder)
        });
    }

    /// A file dialog's answer.
    fn picked(&mut self) {
        let Some(picked) = self.dialogs.poll() else {
            return;
        };
        match picked {
            Picked::Folder(folder) => self.open_folder(folder),
            Picked::Import(path) => self.import(path),
            Picked::Attachments { file_id, paths } => {
                for path in paths {
                    self.attach_file(&file_id, &path, false);
                }
            }
        }
    }

    /// `selectFile`: shows the file in its tab, opening one at the end of the
    /// strip if it has none.
    fn select(&mut self, id: String) {
        self.show_file(id, false);
    }

    /// Makes `id` the active tab. The pending edit is flushed to its own
    /// note first and the tab being left keeps its state. `reuse` shows a
    /// file without a tab in the active tab's place (Previous File and Next
    /// File), so stepping through a project doesn't fill the strip.
    fn show_file(&mut self, id: String, reuse: bool) {
        let Some(p) = &self.project else { return };
        let Some(entry) = p.entry(&id).cloned() else {
            return;
        };
        let current = p.selected.clone();
        let loaded = self.editor.file_id() == Some(id.as_str())
            || self.diagram.file_id() == Some(id.as_str());
        if current.as_deref() == Some(id.as_str()) && loaded {
            return;
        }
        self.flush_active();
        let Some(p) = &mut self.project else { return };
        let replaced = self.tabs.open(&id, current.as_deref().filter(|_| reuse));
        // The tab being left keeps its editor, unless this file took its place.
        if let Some(left) = current.filter(|c| *c != id && replaced.as_ref() != Some(c)) {
            if self.diagram.file_id() == Some(left.as_str()) {
                if let Some(stash) = self.diagram.take() {
                    self.tabs.put(&left, Stashed::Diagram(Box::new(stash)));
                }
            } else if self.editor.file_id() == Some(left.as_str()) {
                if let Some((_, anchor, head, scroll)) = self.editor.view() {
                    // An empty editor with this one's settings takes its place.
                    let mut fresh = self.editor.sibling();
                    if let Some(checker) = &self.spell_checker {
                        fresh.set_spell_checker(Some(
                            checker.clone() as Arc<dyn laankit_editor::SpellChecker>
                        ));
                    }
                    let editor = std::mem::replace(&mut self.editor, fresh);
                    self.tabs.put(
                        &left,
                        Stashed::Note(Box::new(NoteStash {
                            editor,
                            anchor,
                            head,
                            scroll,
                        })),
                    );
                }
            }
        }
        p.selected = Some(id.clone());
        // The open note has the tree's highlight again.
        p.selected_folder = None;
        self.link_key = None;
        match self.tabs.take(&id) {
            Some(Stashed::Note(note)) => {
                let NoteStash {
                    editor,
                    anchor,
                    head,
                    scroll,
                } = *note;
                self.editor = editor;
                self.diagram.clear();
                self.editor.place_view(anchor, head, scroll);
            }
            Some(Stashed::Diagram(diagram)) => {
                self.editor.clear();
                self.diagram.restore(*diagram);
            }
            None => {
                self.editor.clear();
                self.diagram.clear();
                self.backend.read_file(id, entry.filename);
            }
        }
    }

    /// Writes the active tab's pending edit, a note's or a diagram's.
    fn flush_active(&mut self) {
        for save in [self.editor.flush(), self.diagram.flush()]
            .into_iter()
            .flatten()
        {
            self.backend
                .write_file(save.id, save.filename, save.content);
        }
    }

    /// Closes `id`'s tab, writing its pending edit. The active tab's
    /// neighbour is shown in its place.
    fn close_tab(&mut self, id: &str) {
        let active = self
            .project
            .as_ref()
            .is_some_and(|p| p.selected.as_deref() == Some(id));
        if active {
            self.flush_active();
        }
        let Some(p) = &mut self.project else { return };
        if active {
            p.selected = None;
            self.editor.clear();
            self.diagram.clear();
        }
        let next = self.tabs.close(id);
        if let Some(next) = next.filter(|_| active) {
            self.select(next);
        }
    }

    /// Next Tab and Previous Tab.
    fn step_tab(&mut self, delta: isize) {
        let active = self.project.as_ref().and_then(|p| p.selected.clone());
        if let Some(id) = self.tabs.step(active.as_deref(), delta) {
            self.select(id);
        }
    }

    /// `selectPrevFile` / `selectNextFile`.
    fn step(&mut self, delta: i32) {
        let Some(p) = &self.project else { return };
        let list = model::flat_list(&p.entries, &p.folders, self.sort, &self.hidden_tags);
        if list.is_empty() {
            return;
        }
        let next = match p
            .selected
            .as_deref()
            .and_then(|s| list.iter().position(|e| e.id == s))
        {
            None => Some(0),
            Some(i) if delta < 0 && i > 0 => Some(i - 1),
            Some(i) if delta > 0 && i + 1 < list.len() => Some(i + 1),
            _ => None,
        };
        if let Some(i) = next {
            let id = list[i].id.clone();
            self.show_file(id, true);
        }
    }

    /// Saves a tag change: the tags and their colours go to the backend, and
    /// the tags line of an open note's frontmatter is rewritten.
    fn index_changed(&mut self) {
        if let Some(p) = &mut self.project {
            p.refresh();
            p.pending_index_saves += 1;
            self.backend.save_index(p.index.clone());
        }
        self.tags_into_editors();
    }

    /// Closes the project: saves the layout, writes the pending edits.
    fn go_home(&mut self) {
        let Some(p) = &self.project else { return };
        self.backend.save_layout(&p.folder, p.layout.to_value());
        let pending = [self.editor.flush(), self.diagram.flush()]
            .into_iter()
            .flatten()
            .collect();
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
        let (name, filename) = (e.name.clone(), e.filename.clone());
        // The file on disk is what's exported: write a pending edit first.
        self.flush_active();
        if kind == ExportKind::Pdf {
            self.toasts.info(format!("Saving {name} as PDF…"));
        }
        self.backend.export(kind, filename, name);
    }

    // --- Diagrams ------------------------------------------------------------------

    /// Runs an edit of the active tab's diagram. A Custom… colour opens its
    /// dialog.
    fn edit_diagram(&mut self, act: EditAct) {
        if !self.diagram_active() {
            return;
        }
        if let Some(state) = self.diagram.editor.apply(act) {
            self.overlay = Some(Overlay::Colour(state));
        }
    }

    /// The Export dialog for the active tab's diagram.
    fn open_diagram_export(&mut self) {
        if self.diagram_has_content() {
            self.overlay = Some(Overlay::ExportDiagram(ExportState {
                options: self.prefs.diagram_export,
                selected_only: false,
                has_selection: self.diagram.editor.canvas.has_selection(),
            }));
        }
    }

    /// What Export Diagram writes: everything, or the selection.
    fn diagram_scope(&self, selected_only: bool) -> Option<blockliner_core::export::Scope> {
        use blockliner_core::export::{scope, Objects};
        let ed = &self.diagram.editor;
        let objects = if selected_only {
            Objects::Selected {
                shapes: &ed.canvas.selection,
                lines: &ed.canvas.lines,
            }
        } else {
            Objects::All
        };
        scope(&ed.diagram, objects)
    }

    /// Writes the diagram as a PNG or an SVG to the Downloads folder, as the
    /// note exports do, and opens the folder. `false` if it couldn't.
    fn export_diagram(&mut self, ctx: &egui::Context, state: &ExportState) -> bool {
        let Some(scope) = self.diagram_scope(state.selected_only) else {
            return false;
        };
        let o = state.options;
        let name = self.diagram.name().unwrap_or("diagram").to_string();
        #[cfg(test)]
        let dir = self
            .diagram_export_dir
            .clone()
            .unwrap_or_else(backend::downloads_dir);
        #[cfg(not(test))]
        let dir = backend::downloads_dir();
        let path = dir.join(format!(
            "{}.{}",
            noteliner_core::export::export_slug(&name),
            o.format.ext()
        ));
        let result = std::fs::create_dir_all(&dir)
            .map_err(anyhow::Error::from)
            .and_then(|()| diagram_export::render(ctx, &self.diagram.editor.diagram, &scope, &o))
            .and_then(|bytes| diagram_export::write(&path, &bytes));
        match result {
            Ok(()) => {
                self.prefs.diagram_export = o;
                self.backend.set_prefs(json!({
                    "diagramExportFormat": o.format.ext(),
                    "diagramExportGrid": o.grid,
                    "diagramExportScale": o.scale,
                }));
                #[cfg(not(test))]
                if let Err(e) = open::that_detached(&dir) {
                    log::warn!("opening {}: {e}", dir.display());
                }
                self.toasts.info(format!("Saved to {}", path.display()));
                true
            }
            Err(e) => {
                self.toasts.error(format!("Couldn't export: {e:#}"));
                false
            }
        }
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
            // Converting to a presentation selects the note first.
            if p.selected.as_deref() != Some(id.as_str()) {
                self.select(id.clone());
            }
            self.pending_convert = Some(id.clone());
            backend::default_presentation()
        } else {
            Value::Null
        };
        self.send_presentation(id, filename, value);
    }

    /// Sets a note's `presentation:` block: in the editor that has it open,
    /// so a save from there carries it, and in the file.
    fn send_presentation(&mut self, id: String, filename: String, presentation: Value) {
        self.flush_active();
        self.presentation_into_editors(&id, &presentation);
        self.backend.set_presentation(id, filename, presentation);
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
        if self.editor.file_id() != Some(e.id.as_str()) {
            return;
        }
        let Some(config) = normalize_presentation(self.editor.frontmatter().get("presentation"))
        else {
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
        let presentation = if self.is_deck() {
            self.editor.frontmatter().get("presentation")
        } else {
            None
        };
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
        let line = self.editor.body_line().max(1);
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

    /// Starts presenting the open deck at slide `start` (1-based), or at the
    /// caret's slide.
    fn start_presenting(&mut self, start: Option<usize>, speaker: bool) {
        self.present_when_open = None;
        self.update_deck();
        let Some(total) = self.deck.deck().map(|d| d.slides.len()) else {
            return;
        };
        if total == 0 {
            return;
        }
        let start = start
            .or_else(|| self.deck.active(self.editor.body_line().max(1)))
            .unwrap_or(1)
            .clamp(1, total);
        self.deck.presenter =
            Some(noteliner_slides::Presenter::new(start).with_speaker_view(speaker));
        self.palette = None;
    }

    /// Present from a deck's menu in the FILES pane: at once if it is the
    /// open note, or once it has loaded.
    fn present_file(&mut self, id: String, speaker: bool) {
        if self.editor.file_id() == Some(id.as_str()) && self.is_deck() {
            self.start_presenting(None, speaker);
        } else {
            self.select(id.clone());
            self.present_when_open = Some((id, speaker));
        }
    }

    /// Starts the presentation asked for from the FILES pane once its deck
    /// is in the editor; forgets it if another note was opened instead.
    fn present_pending(&mut self) {
        let Some((id, speaker)) = self.present_when_open.clone() else {
            return;
        };
        let selected = self.project.as_ref().and_then(|p| p.selected.as_deref());
        if selected != Some(id.as_str()) {
            self.present_when_open = None;
        } else if self.editor.file_id() == Some(id.as_str()) {
            // Loaded: a note that isn't a deck has nothing to present.
            if self.is_deck() {
                self.start_presenting(None, speaker);
            }
            self.present_when_open = None;
        }
    }

    /// The presenter, over everything, while presenting.
    fn present(&mut self, ctx: &egui::Context) {
        self.present_pending();
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
                    checker.clone() as Arc<dyn laankit_editor::SpellChecker>
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
        // A link to a note is a path from the open note's folder.
        let from = p.selected_entry().map_or("", |e| e.folder.as_str());
        let id = link_target(&p.entries, from, target).map(|e| e.id.clone());
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
            None => {
                let folder = self
                    .project
                    .as_ref()
                    .map(Project::target_folder)
                    .unwrap_or_default();
                self.overlay = Some(Overlay::NewFile(NewFileForm::new(name, false, &folder)));
            }
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
        self.dialogs.start(|| {
            rfd::FileDialog::new()
                .set_title("Import Document")
                .add_filter("Documents", &["docx", "pptx"])
                .add_filter("Word Document", &["docx"])
                .add_filter("PowerPoint Presentation", &["pptx"])
                .pick_file()
                .map(Picked::Import)
        });
    }

    /// Imports the document picked in [`Self::import_dialog`], as a note in
    /// the selected folder.
    fn import(&mut self, path: PathBuf) {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.overlay = Some(Overlay::Busy {
            title: "Importing",
            message: format!("Importing {name}..."),
        });
        let folder = self.project.as_ref().map(Project::target_folder);
        self.backend.import(path, folder);
    }

    // --- Commands --------------------------------------------------------------

    fn apply(&mut self, ctx: &egui::Context, act: Act) {
        let zoom = matches!(
            act,
            Act::ZoomIn | Act::ZoomOut | Act::ZoomReset | Act::SetTheme(_)
        );
        match act {
            Act::NewFile | Act::NewPresentation => {
                if let Some(p) = &self.project {
                    self.overlay = Some(Overlay::NewFile(NewFileForm::new(
                        "",
                        act == Act::NewPresentation,
                        &p.target_folder(),
                    )));
                    self.backend.list_templates();
                }
            }
            Act::NewFolder => {
                if let Some(p) = &self.project {
                    self.overlay = Some(Overlay::NewFolder(NewFolderForm::new(&p.target_folder())));
                }
            }
            Act::DeleteFile => {
                // A selected folder is what Delete deletes.
                let folder = self
                    .project
                    .as_ref()
                    .and_then(|p| p.selected_folder.clone());
                if let Some(path) = folder {
                    self.folder_action(FolderAction::Delete, path);
                } else if let Some(e) = self.project.as_ref().and_then(Project::selected_entry) {
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
            Act::CloseTab => {
                if let Some(id) = self.project.as_ref().and_then(|p| p.selected.clone()) {
                    self.close_tab(&id);
                }
            }
            Act::NextTab => self.step_tab(1),
            Act::PreviousTab => self.step_tab(-1),
            Act::NewDiagram => {
                if let Some(p) = &self.project {
                    self.overlay = Some(Overlay::NewFile(NewFileForm::diagram(
                        "",
                        &p.target_folder(),
                    )));
                    self.backend.list_templates();
                }
            }
            Act::Diagram(act) => self.edit_diagram(act),
            Act::ExportDiagram => self.open_diagram_export(),
            Act::ExportMermaid => {
                if self.diagram_active() {
                    self.export(ExportKind::Mermaid, None);
                }
            }
            Act::ToggleProperties => self.toggle(|l| l.show_properties = !l.show_properties),
            Act::ToggleGrid => {
                self.prefs.diagram_show_grid = !self.prefs.diagram_show_grid;
                self.diagram.set_prefs(&self.prefs);
                self.backend
                    .set_prefs(json!({ "diagramShowGrid": self.prefs.diagram_show_grid }));
            }
            Act::ToggleDimensions => {
                self.prefs.diagram_show_dimensions = !self.prefs.diagram_show_dimensions;
                self.diagram.set_prefs(&self.prefs);
                self.backend.set_prefs(
                    json!({ "diagramShowDimensions": self.prefs.diagram_show_dimensions }),
                );
            }
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
            Act::Present => self.start_presenting(None, false),
            Act::PresentWithSpeakerView => self.start_presenting(None, true),
            Act::PresentFromStart => self.start_presenting(Some(1), false),
            Act::AddTag => {
                // `triggerTagAction('add')`: the TAGS pane's input (shown first
                // if it's hidden).
                if let Some(p) = &mut self.project {
                    p.layout.show_tags = true;
                    self.panes.tags.start_add(p.selected.as_deref());
                }
            }
            Act::TogglePreview => self.toggle(|l| l.show_preview = !l.show_preview),
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
            Act::GoHome => self.go_home(),
            Act::Settings => {
                let mut state =
                    SettingsState::new("ui", &self.backend.paths().prefs(), self.custom_titlebar);
                state.project_folder = self
                    .project
                    .as_ref()
                    .map(|p| p.folder.display().to_string());
                self.overlay = Some(Overlay::Settings(Box::new(state)));
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
                // The folder the note is in.
                let folder = self
                    .project
                    .as_ref()
                    .and_then(|p| p.entry(&id).map(|e| e.folder.clone()));
                if let Some(folder) = folder {
                    self.folder_action(FolderAction::OpenInFileSystem, folder);
                }
            }
            ContextAction::Preview => self.toggle(|l| l.show_preview = !l.show_preview),
            ContextAction::Export(kind) => self.export(kind, Some(id)),
            ContextAction::ExportDiagram => {
                // The export draws the diagram on the canvas: show it first.
                if self.diagram.file_id() == Some(id.as_str()) && self.diagram_active() {
                    self.open_diagram_export();
                } else {
                    self.pending_diagram_export = Some(id.clone());
                    self.select(id.clone());
                    if self.diagram.file_id() == Some(id.as_str()) {
                        self.pending_diagram_export = None;
                        self.open_diagram_export();
                    }
                }
            }
            ContextAction::ConvertToPresentation => self.set_presentation(Some(id), true),
            ContextAction::ConvertToNote => self.set_presentation(Some(id), false),
            ContextAction::PresentationSettings => self.presentation_settings_for(id),
            ContextAction::Present => self.present_file(id, false),
            ContextAction::PresentWithSpeakerView => self.present_file(id, true),
            ContextAction::Duplicate => {
                if let Some(e) = self.project.as_ref().and_then(|p| p.entry(&id)) {
                    self.overlay = Some(Overlay::NewFile(NewFileForm::duplicate(e)));
                }
            }
            ContextAction::Move => {
                if let Some(e) = self.project.as_ref().and_then(|p| p.entry(&id)) {
                    self.overlay = Some(Overlay::Move(MoveForm::file(e)));
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

    /// A folder's menu, the menu of the empty part of the tree (`path` is
    /// empty: the project folder), and Delete with a folder selected.
    fn folder_action(&mut self, action: FolderAction, path: String) {
        let Some(p) = &self.project else { return };
        match action {
            FolderAction::NewFile => {
                self.overlay = Some(Overlay::NewFile(NewFileForm::new("", false, &path)));
                self.backend.list_templates();
            }
            FolderAction::NewDiagram => {
                self.overlay = Some(Overlay::NewFile(NewFileForm::diagram("", &path)));
                self.backend.list_templates();
            }
            FolderAction::NewFolder => {
                self.overlay = Some(Overlay::NewFolder(NewFolderForm::new(&path)));
            }
            FolderAction::Move => {
                if !path.is_empty() {
                    self.overlay = Some(Overlay::Move(MoveForm::folder(&path)));
                }
            }
            FolderAction::OpenInFileSystem => {
                if let Err(e) = open::that_detached(p.folder.join(&path)) {
                    self.toasts.error(format!("Couldn't open the folder: {e}"));
                }
            }
            FolderAction::Delete => {
                if !path.is_empty() {
                    // Asks first, with what is inside.
                    let counts = noteliner_core::project::folder_counts(&p.folder.join(&path));
                    self.overlay = Some(Overlay::DeleteFolder { path, counts });
                }
            }
        }
    }

    /// Moves a note, a diagram or a folder into `into`. Edits waiting to be
    /// saved are written first, as switching tabs does.
    fn move_item(&mut self, item: Item, into: String) {
        self.flush_active();
        match item {
            Item::File(id) => self.backend.move_file(id, into),
            Item::Folder(path) => self.backend.move_folder(path, into),
        }
    }

    /// The pending edits of the notes in `folder` are dropped: the folder is
    /// being deleted.
    fn drop_edits_under(&mut self, folder: &str) {
        let Some(p) = &self.project else { return };
        let inside = |id: Option<&str>| {
            id.and_then(|id| p.entry(id))
                .is_some_and(|e| noteliner_core::tree::is_under(&e.filename, folder))
        };
        if inside(self.editor.file_id()) {
            let _ = self.editor.flush();
        }
        if inside(self.diagram.file_id()) {
            let _ = self.diagram.flush();
        }
    }

    fn open_from_env(&mut self, ctx: &egui::Context) {
        if let (Some(p), Some(file)) = (&self.project, self.diagnostics.file.clone()) {
            let list = model::flat_list(&p.entries, &p.folders, self.sort, &self.hidden_tags);
            let id = match file.parse::<usize>() {
                Ok(i) => list.get(i).map(|e| e.id.clone()),
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
            "about" => self.apply(ctx, Act::About),
            "help" => self.apply(ctx, Act::Help),
            "palette" => self.apply(ctx, Act::Palette),
            "new-file" => self.apply(ctx, Act::NewFile),
            "new-folder" => self.apply(ctx, Act::NewFolder),
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
        let mut view = self.view();
        view.typing = ctx.egui_wants_keyboard_input();
        for act in self.registry.dispatch_keys(ctx, &view) {
            self.apply(ctx, act);
        }
        // A diagram's other keys (the arrows, copy, cut and paste), while it
        // has the keyboard.
        if view.is_diagram && !view.modal_open && !view.typing {
            self.diagram.editor.keys(ctx);
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
        #[cfg(test)]
        {
            self.test_time = ctx.input(|i| i.time);
        }
        self.handle_events(&ctx);
        self.watch(&ctx);
        self.picked();
        self.editor_took_drop = false;
        self.update_editor_inputs();
        self.update_deck();
        self.diagram.editor.begin_frame(&ctx);
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
        self.diagram.editor.end_frame(&ctx);
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
        let is_deck = self.is_deck();
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
                    // Context buttons: the ones only some documents have.
                    // They always come after the standard buttons above,
                    // which every document has, so those never move.
                    if is_deck {
                        let context: [(&str, &str, bool, Act); 3] = [
                            (
                                icons::PERSON_CHALKBOARD,
                                "Slides (Ctrl+Shift+G)",
                                l.show_slides,
                                Act::ToggleSlides,
                            ),
                            (icons::CIRCLE_PLAY, "Present (F5)", false, Act::Present),
                            (
                                icons::DISPLAY,
                                "Present with Speaker View (Alt+F5)",
                                false,
                                Act::PresentWithSpeakerView,
                            ),
                        ];
                        // The toolbar doesn't scroll: what doesn't fit above
                        // the bottom group is left out (the keys still work).
                        let room = (1..=context.len())
                            .rev()
                            .find(|n| toolbar::fits(ui, *n))
                            .unwrap_or(0);
                        if room > 0 {
                            toolbar::divider(ui);
                        }
                        for (glyph, title, active, act) in context.into_iter().take(room) {
                            if b(ui, glyph, title, active, false) {
                                acts.push(act);
                            }
                        }
                    }
                }
            },
            |ui| {
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
        let p = self.project.as_ref();
        let diagram_on = self.diagram_active();
        let note = p.and_then(Project::selected_entry).map(|e| {
            let status = if diagram_on {
                self.diagram.status()
            } else {
                self.editor.status()
            };
            (e.name.clone(), status)
        });
        let props = StatusProps {
            project_open: p.is_some(),
            note_count: p.map_or(0, |p| p.entries.len()),
            note: note.as_ref().map(|(n, s)| (n.as_str(), *s)),
            text: self.editor.text(),
            cursor: self.editor.cursor(),
            selection_len: self.editor.selection_len(),
            spell_check: self.prefs.spell_check,
            deck: if self.is_deck() {
                self.deck.status(self.editor.body_line().max(1))
            } else {
                None
            },
            diagram: diagram_on.then(|| {
                let canvas = &self.diagram.editor.canvas;
                (canvas.view.zoom, canvas.pointer)
            }),
            mcp_running: self.mcp_running,
            frontmatter_unreadable: !diagram_on && self.editor.frontmatter_unreadable(),
        };
        egui::Panel::bottom("noteliner-status")
            .exact_size(views::STATUS_BAR_HEIGHT)
            .resizable(false)
            .show_separator_line(false)
            .frame(Frame::NONE)
            .show(ui, |ui| views::status_bar(ui, &props));
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
                Some(OpenAction::OpenFolder) => self.pick_folder(),
                Some(OpenAction::OpenRecent(path)) => self.open_folder(PathBuf::from(path)),
                Some(OpenAction::RemoveRecent(r)) => self.overlay = Some(Overlay::RemoveRecent(r)),
                None => {}
            }
            return;
        }

        let modal_open =
            self.overlay.is_some() || self.palette.is_some() || !self.mcp_queue.is_empty();
        let mut sidebar_actions = Vec::new();
        let mut preview_actions = Vec::new();
        let mut editor_actions = Vec::new();
        let mut attachment_actions = Vec::new();
        let mut log_action = None;
        // A note dragged from FILES: captured before the tree forgets it on release.
        let tree_drag = self.tree.dragging().map(str::to_string);
        let slide_images = SlideImages {
            ctx: ctx.clone(),
            dir: self.project.as_ref().map(Project::attachments_dir),
        };
        // The caret's line in the note's text: 0 while it is in the
        // frontmatter, which is under no heading and on the first slide.
        let caret_line = self.editor.body_line();
        let active_slide = self.deck.active(caret_line.max(1));
        let is_deck = self.is_deck();
        let Some(p) = self.project.as_mut() else {
            return;
        };
        let note_name = p
            .selected_entry()
            .map(|e| e.name.clone())
            .unwrap_or_default();

        // BACKLINKS follows the selection while it's shown.
        if p.layout.show_backlinks {
            let files = p.entries.len();
            if let Some(id) = self.panes.backlinks.stale(p.selected.as_deref(), files) {
                self.backend.backlinks(id);
            }
        } else {
            // Reopening the pane loads afresh, as remounting it does.
            self.panes.backlinks.invalidate();
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
                        folders: &p.folders,
                        open_folders: &p.open_folders,
                        sort: self.sort,
                        hidden_tags: &self.hidden_tags,
                        selected: p.selected.as_deref(),
                        selected_folder: p.selected_folder.as_deref(),
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

        // The tabs, across the document and the panels to its right.
        let mut tab_action = None;
        let tab_ids: Vec<String> = self.tabs.order().to_vec();
        if !tab_ids.is_empty() {
            let items: Vec<TabItem> = tab_ids
                .iter()
                .map(|id| {
                    let e = p.entry(id);
                    TabItem {
                        name: e.map(|e| e.name.clone()).unwrap_or_default(),
                        path: e.map(|e| e.filename.clone()).unwrap_or_default(),
                        active: p.selected.as_deref() == Some(id.as_str()),
                        icon: Some(views::tree::file_icon(
                            e.is_some_and(Entry::is_diagram),
                            e.is_some_and(|e| e.deck),
                        )),
                    }
                })
                .collect();
            let reveal = std::mem::take(&mut self.tabs.reveal);
            tab_action = egui::Panel::top("noteliner-tabs")
                .exact_size(tabs::HEIGHT)
                .resizable(false)
                .show_separator_line(false)
                .frame(Frame::NONE)
                .show(ui, |ui| tabs::strip(ui, "noteliner-tabs", &items, reveal))
                .inner;
        }

        if p.layout.show_attachments {
            let list = p
                .selected
                .as_deref()
                .map(|id| attachments::of_note(&p.index, id))
                .unwrap_or_default();
            let dir = p.attachments_dir();
            let selected = p.selected.is_some();
            let insertable = p.selected_entry().is_some_and(|e| !e.is_diagram());
            let resp = egui::Panel::right("noteliner-attachments")
                .resizable(true)
                .default_size(p.layout.attachment_panel_width)
                .size_range(160.0..=400.0)
                .frame(Frame::NONE.fill(pal.bg_surface))
                .show(ui, |ui| {
                    attachments::show(ui, selected, insertable, &list, &dir)
                });
            attachment_actions = resp.inner;
            remember_size(
                &mut p.layout.attachment_panel_width,
                resp.response.rect.width(),
            );
        }
        // A diagram has the Properties panel where a note has its preview.
        let diagram_on = p.selected.is_some() && p.selected.as_deref() == self.diagram.file_id();
        let mut properties_action = None;
        if diagram_on && p.layout.show_properties {
            let ed = &self.diagram.editor;
            let props = PropertiesProps::new(
                &ed.diagram,
                &ed.canvas.selection,
                ed.canvas.selected_style(&ed.diagram),
                ed.canvas.selected_kinds(&ed.diagram),
            );
            let state = &mut self.diagram.properties;
            let resp = egui::Panel::right("noteliner-properties")
                .resizable(true)
                .default_size(p.layout.properties_width)
                .size_range(200.0..=420.0)
                .frame(Frame::NONE.fill(pal.bg_surface))
                .show(ui, |ui| diagram_view::properties_panel(ui, state, &props));
            let closed;
            (properties_action, closed) = resp.inner;
            remember_size(&mut p.layout.properties_width, resp.response.rect.width());
            if closed {
                p.layout.show_properties = false;
            }
        }
        if p.layout.show_preview && !diagram_on {
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
        let preview_on = p.layout.show_preview;
        let mut diagram_actions = Vec::new();
        let mut diagram_menu = None;
        if diagram_on {
            let ed = &self.diagram.editor;
            let view = ed.view();
            let props = diagram_view::ToolbarProps {
                file_name: &note_name,
                tool: ed.tool,
                more_shape: ed.more_shape,
                can_undo: view.can_undo,
                can_redo: view.can_redo,
                orderable: view.orderable,
                show_grid: ed.canvas.show_grid,
                show_dimensions: ed.canvas.show_dimensions,
                zoom: ed.canvas.view.zoom,
                show_properties: p.layout.show_properties,
                can_export: !(ed.diagram.shapes.is_empty() && ed.diagram.connectors.is_empty()),
            };
            // No dialog or text field has the keyboard: Space may pan.
            let keys = !modal_open && !ctx.egui_wants_keyboard_input();
            egui::CentralPanel::default()
                .frame(Frame::NONE.fill(pal.bg_base))
                .show(ui, |ui| {
                    diagram_actions = egui::Panel::top(ui.scope_id().with("diagram-toolbar"))
                        .frame(Frame::NONE)
                        .show_separator_line(false)
                        .show(ui, |ui| diagram_view::toolbar(ui, &props))
                        .inner;
                    egui::CentralPanel::default()
                        .frame(Frame::NONE)
                        .show(ui, |ui| {
                            diagram_menu = self.diagram.editor.show(ui, keys);
                        });
                });
        } else {
            egui::CentralPanel::default()
                .frame(Frame::NONE.fill(pal.bg_base))
                .show(ui, |ui| {
                    editor_actions = self.editor.show(ui, preview_on);
                });
        }

        // A note dropped from FILES onto the text: a link at the drop point
        // (`interceptInternalDrops`).
        if let Some(dragged) = tree_drag {
            let (released, pointer, now) =
                ctx.input(|i| (i.pointer.any_released(), i.pointer.interact_pos(), i.time));
            if released {
                if let Some(at) = self.editor.drop_target(pointer) {
                    // The link is a path from the open note's folder.
                    let link = self.project.as_ref().and_then(|p| {
                        let from = p.selected_entry().map_or("", |e| e.folder.as_str());
                        p.entry(&dragged)
                            .map(|e| format!("[{}]({})", e.name, relative_link(from, &e.filename)))
                    });
                    if let Some(link) = link {
                        self.editor.insert_at(at, &link, now);
                    }
                }
            }
        }

        // The diagram: its toolbar, its right-click menu, the Properties
        // panel, then one undo step per finished change and the autosave.
        for a in diagram_actions {
            match a {
                DiagramToolbarAction::Edit(act) => self.edit_diagram(act),
                DiagramToolbarAction::ToggleGrid => self.apply(&ctx, Act::ToggleGrid),
                DiagramToolbarAction::ToggleDimensions => self.apply(&ctx, Act::ToggleDimensions),
                DiagramToolbarAction::ToggleProperties => self.apply(&ctx, Act::ToggleProperties),
                DiagramToolbarAction::Export => self.apply(&ctx, Act::ExportDiagram),
            }
        }
        if let Some(act) = diagram_menu {
            self.edit_diagram(act);
        }
        if let Some(state) = properties_action.and_then(|a| self.diagram.editor.apply_properties(a))
        {
            self.overlay = Some(Overlay::Colour(state));
        }
        if diagram_on && self.diagram.editor.observe() {
            let now = ctx.input(|i| i.time);
            if let Some(save) = self.diagram.due(now) {
                self.backend
                    .write_file(save.id, save.filename, save.content);
            }
            // Keep the autosave timer ticking without input.
            if self.diagram.is_dirty() {
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(
                    crate::editor::SAVE_DELAY_SECS,
                ));
            }
        }

        match tab_action {
            Some(TabAction::Activate(i)) => {
                if let Some(id) = tab_ids.get(i) {
                    self.select(id.clone());
                }
            }
            Some(TabAction::Close(i)) => {
                if let Some(id) = tab_ids.get(i) {
                    self.close_tab(id);
                }
            }
            None => {}
        }
        for a in sidebar_actions {
            self.sidebar_action(a);
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
                noteliner_slides::PaneAction::Present { index } => {
                    self.start_presenting(Some(index), false);
                }
                noteliner_slides::PaneAction::OpenSettings => {
                    if let Some(id) = self.project.as_ref().and_then(|p| p.selected.clone()) {
                        self.presentation_settings_for(id);
                    }
                }
            },
            SidebarAction::Present => self.start_presenting(None, false),
            SidebarAction::Tree(t) => match t {
                TreeAction::Select(id) => {
                    // A click on the open note takes the highlight back from
                    // a selected folder.
                    if let Some(p) = &mut self.project {
                        p.selected_folder = None;
                    }
                    self.select(id);
                }
                TreeAction::SelectFolder(path) => {
                    if let Some(p) = &mut self.project {
                        p.selected_folder = Some(path);
                    }
                }
                TreeAction::ToggleFolder(path) => {
                    if let Some(p) = &mut self.project {
                        if !p.open_folders.remove(&path) {
                            p.open_folders.insert(path);
                        }
                    }
                }
                TreeAction::Rename { id, name } => self.backend.rename_file(id, name),
                TreeAction::RenameFolder { path, name } => {
                    // Edits waiting to be saved are written first.
                    self.flush_active();
                    self.backend.rename_folder(path, name);
                }
                TreeAction::Drop { dragged, into } => self.move_item(dragged, into),
                TreeAction::Context(a, id) => self.context_action(a, id),
                TreeAction::FolderContext(a, path) => self.folder_action(a, path),
                TreeAction::Step(d) => self.step(d),
            },
            SidebarAction::NewFolder => {
                if let Some(p) = &self.project {
                    self.overlay = Some(Overlay::NewFolder(NewFolderForm::new(&p.target_folder())));
                }
            }
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
                self.dialogs.start(move || {
                    let paths = rfd::FileDialog::new()
                        .set_title("Add Attachments")
                        .pick_files()?;
                    Some(Picked::Attachments { file_id: id, paths })
                });
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
        if p.layout.open_tabs != self.tabs.order() {
            p.layout.open_tabs = self.tabs.order().to_vec();
        }
        let active = p.selected.as_deref().unwrap_or_default();
        if p.layout.active_tab != active {
            p.layout.active_tab = active.to_string();
        }
        // The open folders, by path.
        if p.layout.open_folders.len() != p.open_folders.len()
            || p.layout
                .open_folders
                .iter()
                .any(|f| !p.open_folders.contains(f))
        {
            let mut open: Vec<String> = p.open_folders.iter().cloned().collect();
            open.sort();
            p.layout.open_folders = open;
        }
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
                .filter(|p| {
                    p.extension().is_some_and(|e| {
                        ["md", "mmd", "mermaid"]
                            .iter()
                            .any(|x| e.eq_ignore_ascii_case(x))
                    })
                })
                .collect();
            (paths, !i.raw.hovered_files.is_empty())
        });
        if !dropped.is_empty() {
            let folder = self.project.as_ref().map(Project::target_folder);
            self.backend.import_markdown(dropped, folder);
        }
        if hovering && over_editor {
            views::drop_overlay(ctx, "Drop files to attach them to the note");
        } else if hovering {
            views::drop_overlay(ctx, "Drop Markdown or Mermaid files to add them");
        }
    }

    // --- Dialogs -----------------------------------------------------------------

    fn overlays(&mut self, ctx: &egui::Context) {
        // A note that changed on disk under unsaved edits asks once the
        // dialog that was open has closed.
        if self.overlay.is_none() {
            if let Some((id, content)) = self.pending_conflict.take() {
                let name = self.editor.name().to_string();
                if self.editor.file_id() == Some(id.as_str()) {
                    self.overlay = Some(Overlay::Conflict { id, name, content });
                }
            }
        }
        let Some(mut overlay) = self.overlay.take() else {
            return;
        };
        let top_inset = self.top_inset();
        let mut keep = true;
        match &mut overlay {
            Overlay::NewFile(form) => {
                let (folders, tags) = match &self.project {
                    Some(p) => (p.folders.clone(), model::all_tags(&p.index)),
                    None => (Vec::new(), Vec::new()),
                };
                match overlays::new_file(ctx, form, &folders, &tags) {
                    Some(FormAction::Close) => keep = false,
                    Some(FormAction::Submit) => {
                        keep = false;
                        let tags: Vec<String> = form.tags.iter().cloned().collect();
                        let name = form.name.trim().to_string();
                        let folder = Some(form.folder.clone());
                        match &form.duplicate_of {
                            Some(source) => {
                                // The copy is of the file on disk: write a
                                // pending edit first.
                                self.flush_active();
                                self.backend
                                    .duplicate_file(source.clone(), name, tags, folder)
                            }
                            None if form.diagram => self.backend.create_diagram(name, tags, folder),
                            None => self.backend.create_file(
                                name,
                                tags,
                                form.template.clone(),
                                folder,
                                form.deck,
                            ),
                        }
                    }
                    None => {}
                }
            }
            Overlay::NewFolder(form) => {
                let folders = self
                    .project
                    .as_ref()
                    .map(|p| p.folders.clone())
                    .unwrap_or_default();
                match overlays::new_folder(ctx, form, &folders) {
                    Some(FormAction::Close) => keep = false,
                    Some(FormAction::Submit) => {
                        keep = false;
                        self.backend.create_folder(form.path());
                    }
                    None => {}
                }
            }
            Overlay::Move(form) => {
                let folders = self
                    .project
                    .as_ref()
                    .map(|p| p.folders.clone())
                    .unwrap_or_default();
                match overlays::move_dialog(ctx, form, &folders) {
                    Some(FormAction::Close) => keep = false,
                    Some(FormAction::Submit) => {
                        keep = false;
                        if form.folder != form.current {
                            let item = match &form.target {
                                MoveTarget::File(id) => Item::File(id.clone()),
                                MoveTarget::Folder(path) => Item::Folder(path.clone()),
                            };
                            self.move_item(item, form.folder.clone());
                        }
                    }
                    None => {}
                }
            }
            Overlay::Colour(state) => match colour_dialog::show(ctx, state) {
                Some(ColourAction::Close) => keep = false,
                Some(ColourAction::Apply(c)) => {
                    let stroke = state.stroke;
                    self.diagram.editor.apply_colour(Some(c), stroke);
                    keep = false;
                }
                None => {}
            },
            Overlay::ExportDiagram(state) => {
                let size = self
                    .diagram_scope(state.selected_only)
                    .map(|s| diagram_export::size(&s, &state.options));
                match export_dialog::show(ctx, state, size, false) {
                    Some(ExportAction::Close) => keep = false,
                    Some(ExportAction::Export) => keep = !self.export_diagram(ctx, state),
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
                    if self.diagram.file_id() == Some(id.as_str()) {
                        let _ = self.diagram.flush();
                    }
                    self.backend.delete_file(id.clone(), false);
                }
                Some(Confirm::No) => keep = false,
                None => {}
            },
            Overlay::DeleteFolder { path, counts } => {
                match overlays::delete_folder(ctx, path, counts) {
                    Some(Confirm::Yes) => {
                        keep = false;
                        // The tabs of the notes inside close when the
                        // folder is gone; their pending edits are dropped.
                        self.drop_edits_under(path);
                        self.backend.delete_folder(path.clone(), false);
                    }
                    Some(Confirm::No) => keep = false,
                    None => {}
                }
            }
            Overlay::DeletePermanently {
                target,
                name,
                message,
            } => match overlays::delete_permanently(ctx, name, message) {
                Some(Confirm::Yes) => {
                    keep = false;
                    match target {
                        DeleteTarget::File(id) => self.backend.delete_file(id.clone(), true),
                        DeleteTarget::Folder(path) => {
                            self.backend.delete_folder(path.clone(), true)
                        }
                    }
                }
                Some(Confirm::No) => keep = false,
                None => {}
            },
            Overlay::Conflict { id, name, content } => match overlays::conflict(ctx, name) {
                Some(ConflictAction::Reload) => {
                    keep = false;
                    self.editor.reload(id, std::mem::take(content));
                }
                Some(ConflictAction::KeepMine) => {
                    keep = false;
                    // The edits are saved over the file on disk.
                    self.editor.hold(false);
                    if self.editor.file_id() == Some(id.as_str()) {
                        if let Some(save) = self.editor.flush() {
                            self.backend
                                .write_file(save.id, save.filename, save.content);
                        }
                    }
                }
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
            Overlay::Settings(state) => {
                for a in crate::overlays::settings::show(ctx, state, &mut self.theme, top_inset) {
                    match a {
                        SettingsAction::Close => keep = false,
                        // Applied already; eframe saves it with the theme.
                        SettingsAction::ThemeChanged => {}
                        SettingsAction::SetPrefs(patch) => {
                            self.backend.set_prefs(patch);
                            self.prefs = self.backend.paths().prefs();
                            self.diagram.set_prefs(&self.prefs);
                        }
                        SettingsAction::RefreshMcp => self.backend.mcp_status(),
                        SettingsAction::OpenProjectFolder => {
                            if let Some(p) = &self.project {
                                if let Err(e) = open::that_detached(&p.folder) {
                                    self.toasts.error(format!("Couldn't open the folder: {e}"));
                                }
                            }
                        }
                        SettingsAction::Restart => restart(ctx),
                    }
                }
            }
            Overlay::Help(state) => {
                if crate::overlays::help::show(ctx, state, top_inset) {
                    keep = false;
                }
            }
            Overlay::About => {
                let modal = AboutModal::new("NoteLiner", env!("CARGO_PKG_VERSION"))
                    .description(DESCRIPTION)
                    .repo(REPO_URL, Some("github.com/NathanLaan/laankit"))
                    .icon(AboutIcon::Glyph(icons::FILE_LINES))
                    .update(&self.update);
                match modal.show(ctx) {
                    Some(AboutAction::Close) => keep = false,
                    Some(AboutAction::CheckForUpdates) => {
                        self.update = UpdateStatus::Checking;
                        self.backend.check_updates();
                    }
                    Some(AboutAction::OpenUrl(url)) => ctx.open_url(egui::OpenUrl::new_tab(url)),
                    Some(AboutAction::Download) => {
                        if let UpdateStatus::Available { url, .. } = &self.update {
                            ctx.open_url(egui::OpenUrl::new_tab(url));
                        }
                    }
                    Some(AboutAction::Install) | None => {}
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
                        let (id, filename) = (id.clone(), filename.clone());
                        self.send_presentation(id, filename, presentation.to_json());
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
                    let score = laankit_egui::commands::fuzzy_score(&tag, q);
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
                    let score = laankit_egui::commands::fuzzy_score(&e.name, q);
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
                    let score = laankit_egui::commands::fuzzy_score(&r.name, q);
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

    /// On quit: write the pending edit and the layout.
    pub fn shutdown(&mut self) {
        for save in [self.editor.flush(), self.diagram.flush()]
            .into_iter()
            .flatten()
        {
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

/// The path of the file at `to` as a link from a note in the folder `from`:
/// `./kickoff.md` beside it, `../archive/old.md` elsewhere.
fn relative_link(from: &str, to: &str) -> String {
    let from: Vec<&str> = from.split('/').filter(|s| !s.is_empty()).collect();
    let to: Vec<&str> = to.split('/').filter(|s| !s.is_empty()).collect();
    let dirs = to.len().saturating_sub(1);
    let shared = from
        .iter()
        .zip(to.iter().take(dirs))
        .take_while(|(a, b)| a == b)
        .count();
    let mut parts: Vec<&str> = vec![".."; from.len() - shared];
    parts.extend(&to[shared..]);
    if from.len() == shared {
        format!("./{}", parts.join("/"))
    } else {
        parts.join("/")
    }
}

/// The note a Markdown link in a note of the folder `from` points at: its
/// path from that folder, or (as links were written when every note was at
/// the top) its path from the project folder, its file name, or its name.
fn link_target<'a>(entries: &'a [Entry], from: &str, target: &str) -> Option<&'a Entry> {
    let target = target.split(['#', '?']).next().unwrap_or(target);
    let mut parts: Vec<&str> = from.split('/').filter(|s| !s.is_empty()).collect();
    let mut escaped = false;
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => escaped |= parts.pop().is_none(),
            p => parts.push(p),
        }
    }
    let resolved = parts.join("/");
    let plain = target.trim_start_matches("./");
    entries
        .iter()
        .find(|e| !escaped && e.filename == resolved)
        .or_else(|| entries.iter().find(|e| e.filename == plain))
        .or_else(|| {
            let mut named = entries
                .iter()
                .filter(|e| noteliner_core::tree::file_name(&e.filename) == plain);
            match (named.next(), named.next()) {
                (Some(e), None) => Some(e),
                _ => None,
            }
        })
        .or_else(|| entries.iter().find(|e| e.name == plain))
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
    fn links_between_folders() {
        assert_eq!(relative_link("", "a.md"), "./a.md");
        assert_eq!(relative_link("work", "work/a.md"), "./a.md");
        assert_eq!(relative_link("", "work/a.md"), "./work/a.md");
        assert_eq!(relative_link("work", "a.md"), "../a.md");
        assert_eq!(
            relative_link("work/meetings", "archive/a.md"),
            "../../archive/a.md"
        );
        assert_eq!(relative_link("work/meetings", "work/a.md"), "../a.md");
        assert_eq!(
            relative_link("work", "work/meetings/a.md"),
            "./meetings/a.md"
        );

        let index = json!({ "files": [
            { "id": "top", "name": "Top", "filename": "a.md" },
            { "id": "work", "name": "Work A", "filename": "work/a.md" },
            { "id": "deep", "name": "Deep", "filename": "work/meetings/kickoff.md" }
        ]});
        let entries = model::entries(&index);
        let to =
            |from: &str, target: &str| link_target(&entries, from, target).map(|e| e.id.as_str());
        // A path from the note's own folder comes first.
        assert_eq!(to("work", "./a.md"), Some("work"));
        assert_eq!(to("", "./a.md"), Some("top"));
        assert_eq!(to("work", "../a.md"), Some("top"));
        assert_eq!(to("work", "./meetings/kickoff.md"), Some("deep"));
        assert_eq!(to("work/meetings", "../../a.md"), Some("top"));
        // Then a path from the project folder, a file name, and a name.
        assert_eq!(to("work/meetings", "./a.md"), Some("top"));
        assert_eq!(to("", "kickoff.md"), Some("deep"));
        assert_eq!(to("", "Deep"), Some("deep"));
        assert_eq!(to("", "./nope.md"), None);
        // A path that leaves the project points at nothing.
        assert_eq!(to("", "../../a.md"), None);
    }

    #[test]
    fn short_paths() {
        assert_eq!(short_path("/home/u/docs/notes"), ".../docs/notes");
        assert_eq!(short_path("/notes"), "/notes");
    }
}
