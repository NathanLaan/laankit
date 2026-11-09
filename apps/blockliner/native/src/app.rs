//! The app: owns the open folder, the open files and their tabs, the
//! canvas, the current tool and the open dialog; routes commands (keyboard,
//! palette, toolbar, sidebar, tabs) and dialog actions, autosaves, and picks
//! up outside changes.
//!
//! The active tab's diagram, undo history and view live in `diagram`,
//! `history` and `canvas`, so the editing code knows nothing of tabs.
//! Switching tabs writes the active one's edits, puts its state aside in
//! `stash`, and loads the next one's (from `stash`, else from its file).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use blockliner_core::clipboard;
use blockliner_core::history::History;
use blockliner_core::style::{Colour, Style, PRESETS};
use blockliner_core::Diagram;
use egui::{Frame, Ui};
use laankit_egui::about::{AboutAction, AboutIcon, AboutModal};
use laankit_egui::commands::{Recents, Registry};
use laankit_egui::diagnostics::{self, Screenshot};
use laankit_egui::dialog::{self, Confirm};
use laankit_egui::file_dialog::FileDialogs;
use laankit_egui::palette::{CommandPalette, PaletteOutcome};
use laankit_egui::panes::{self, Pane, PaneLayout};
use laankit_egui::theme::{self, ThemeState};
use laankit_egui::titlebar::{self, TitleBar, TitleBarAction};
use laankit_egui::toast::Toasts;
use laankit_egui::{fonts, icons, window};
use serde_json::{json, Value};

use crate::backend::{self, Backend, Change, Event};
use crate::canvas::view::View as CanvasView;
use crate::canvas::{Canvas, MenuAction, Swatch, Tool};
use crate::commands::{self, Act, View};
use crate::config::Paths;
use crate::export;
use crate::files::{self, Files};
use crate::overlays::colour::{ColourAction, ColourState};
use crate::overlays::export::{ExportAction, ExportState};
use crate::overlays::name::{NameAction, NameState};
use crate::overlays::settings::{SettingsAction, SettingsState};
use crate::overlays::{self, Overlay};
use crate::properties::{self, PropertiesAction, PropertiesProps, PropertiesState};
use crate::views::{
    self, FileRow, FilesAction, SaveState, SidebarAction, StatusProps, TabAction, TabItem,
    ToolbarProps,
};

/// eframe storage keys.
const THEME_KEY: &str = "blockliner-theme";
const RECENTS_KEY: &str = "blockliner-recents";
/// The sidebar's panes (order and heights), kept with eframe's other
/// layout state.
const PANES_KEY: &str = "blockliner-panes";
/// The sidebar's panes.
const PROJECT_PANE: &str = "project";
const FILES_PANE: &str = "files";
const PROPERTIES_PANE: &str = "properties";
/// The Project and Files panes' least height: the header and two rows.
const LIST_MIN_HEIGHT: f32 = 104.0;
const DESCRIPTION: &str =
    "A diagram editor for software architecture, saving Mermaid flowcharts. Built on the laankit framework.";
const REPO_URL: &str = "https://github.com/NathanLaan/laankit";
/// How often to look at the folder for outside changes, in seconds.
const SCAN_SECONDS: f64 = 1.0;

/// The open folder.
struct Project {
    folder: PathBuf,
    /// Its diagrams, as the sidebar lists them.
    names: Vec<String>,
}

/// The diagram in the active tab.
struct Doc {
    path: PathBuf,
    /// Its name: the file name without `.mmd`.
    name: String,
    /// The diagram as last read or handed to the autosave: the canvas has
    /// unsaved edits while it differs.
    saved: Diagram,
}

/// A background tab's state, put aside while another tab is active. Its
/// edits were written before it was put aside.
struct Stash {
    diagram: Diagram,
    saved: Diagram,
    history: History,
    view: CanvasView,
}

pub struct BlockLinerApp {
    paths: Paths,
    theme: ThemeState,
    /// Whether the window was built undecorated, with LaanKit's title bar.
    custom_titlebar: bool,
    sidebar_visible: bool,
    /// The Properties pane, last in the sidebar (`Ctrl+I`).
    properties_visible: bool,
    properties: PropertiesState,
    /// The Files pane.
    files_visible: bool,
    /// The sidebar's panes: their order and heights.
    panes: PaneLayout,
    status_bar_visible: bool,
    initialized: bool,
    backend: Backend,
    project: Option<Project>,
    /// The Files pane's list and the tabs.
    files: Files,
    /// The active tab's diagram; `None` with no tab.
    doc: Option<Doc>,
    /// Background tabs that have been open, by path. A tab without one
    /// (restored at start, never shown) is read from its file when shown.
    stash: HashMap<PathBuf, Stash>,
    /// Scroll the active tab into view on the next frame.
    reveal_tab: bool,
    /// egui time of the last look at the folder.
    last_scan: f64,
    /// The window title last sent.
    title: String,
    /// The active tab's diagram; an empty one while none is open.
    diagram: Diagram,
    history: History,
    canvas: Canvas,
    tool: Tool,
    /// The More Shapes button's shape: the last of the four picked.
    more_shape: Tool,
    /// The last copy's (or paste's) clipboard text, and how many times it's
    /// been pasted, so each paste of the same copy steps further.
    clip: Option<(String, usize)>,
    /// For the end of the frame (commands run without the context): text
    /// for the system clipboard, and a paste to ask for.
    copy_out: Option<String>,
    paste_request: bool,
    /// The tool was picked with Shift, so it stays after each shape.
    sticky_tool: bool,
    /// The folders Open Containing Folder asked for, in tests.
    #[cfg(test)]
    opened_folders: Vec<std::path::PathBuf>,
    /// Shift is down this frame (for picking a tool with it).
    shift: bool,
    overlay: Option<Overlay>,
    palette: Option<CommandPalette>,
    registry: Registry<Act, View>,
    toasts: Toasts,
    screenshot: Option<Screenshot>,
    /// `LAANKIT_OPEN`: a dialog to open on the first frame, for screenshots.
    open_on_start: Option<String>,
    /// The folder and save dialogs, off the UI thread where they'd freeze it.
    dialogs: FileDialogs<Picked>,
    /// Tests: export here instead of asking with the save dialog.
    #[cfg(test)]
    export_to: Option<PathBuf>,
}

/// A file dialog's answer.
enum Picked {
    Folder(PathBuf),
    /// Open File's `.mmd` files.
    Files(Vec<PathBuf>),
    /// Where to export, with the Export dialog's choices when Export… was
    /// pressed.
    Export {
        path: PathBuf,
        options: export::Options,
        selected_only: bool,
    },
}

impl BlockLinerApp {
    /// `storage` is eframe's (theme, scale, palette recents); `None` in tests.
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
        let diagram = Diagram {
            grid: prefs.grid_size,
            ..Diagram::new()
        };
        let mut app = Self {
            paths,
            theme,
            custom_titlebar: prefs.custom_titlebar,
            sidebar_visible: prefs.sidebar_visible,
            properties_visible: prefs.properties_visible,
            properties: PropertiesState::default(),
            files_visible: prefs.files_visible,
            panes: storage
                .and_then(|s| eframe::get_value(s, PANES_KEY))
                .unwrap_or_else(default_panes),
            status_bar_visible: prefs.status_bar_visible,
            initialized: false,
            backend: Backend::new(backend::SAVE_DELAY),
            project: None,
            files: Files::default(),
            doc: None,
            stash: HashMap::new(),
            reveal_tab: false,
            last_scan: 0.0,
            title: String::new(),
            history: History::new(&diagram),
            diagram,
            canvas: Canvas::new(prefs.show_grid, prefs.show_dimensions, prefs.grid_unit),
            tool: Tool::default(),
            more_shape: Tool::MORE[0],
            clip: None,
            copy_out: None,
            paste_request: false,
            sticky_tool: false,
            #[cfg(test)]
            opened_folders: Vec::new(),
            shift: false,
            overlay: None,
            palette: None,
            registry,
            toasts: Toasts::default(),
            screenshot: Screenshot::from_env(),
            open_on_start: std::env::var("LAANKIT_OPEN").ok(),
            // Tests answer in place, with no dialog.
            dialogs: if cfg!(test) {
                FileDialogs::in_place()
            } else {
                FileDialogs::new()
            },
            #[cfg(test)]
            export_to: None,
        };
        if let Some(folder) = prefs.last_project.clone() {
            app.set_project(folder);
        }
        app.restore_files(&prefs);
        app
    }

    /// The Files pane, the tabs and the active tab as BlockLiner last
    /// closed with them. Before there were tabs (no `openFiles` yet), the
    /// project's last diagram, else its first.
    fn restore_files(&mut self, prefs: &crate::config::Prefs) {
        let Some(list) = &prefs.open_files else {
            let names = self.project.as_ref().map(|p| p.names.clone());
            let pick = prefs
                .last_diagram
                .clone()
                .filter(|n| names.as_ref().is_some_and(|names| names.contains(n)))
                .or_else(|| names.and_then(|n| n.first().cloned()));
            if let Some(name) = pick {
                self.open_diagram(&name);
            }
            return;
        };
        for path in list.iter().filter(|p| p.is_file()) {
            self.files.list.push(files::OpenFile {
                path: path.clone(),
                missing: false,
            });
        }
        for tab in &prefs.open_tabs {
            if self.files.contains(tab) && !self.files.has_tab(tab) {
                self.files.tabs.push(tab.clone());
            }
        }
        let active = prefs
            .active_file
            .clone()
            .filter(|a| self.files.has_tab(a))
            .or_else(|| self.files.tabs.first().cloned());
        if let Some(path) = active {
            self.open_file(&path);
        }
    }

    /// Fonts and the theme, once per context.
    fn init(&mut self, ctx: &egui::Context) {
        if self.initialized {
            return;
        }
        self.initialized = true;
        self.backend.attach(ctx);
        self.dialogs.attach(ctx);
        fonts::install(ctx);
        self.theme.apply(ctx);
        match self.open_on_start.take().as_deref() {
            Some("settings") => self.apply(Act::Settings),
            Some("about") => self.apply(Act::About),
            Some("palette") => self.apply(Act::Palette),
            _ => {}
        }
    }

    // --- Accessors for tests ------------------------------------------------

    #[cfg(test)]
    pub fn tool(&self) -> Tool {
        self.tool
    }

    #[cfg(test)]
    pub fn opened_folders(&self) -> &[std::path::PathBuf] {
        &self.opened_folders
    }

    #[cfg(test)]
    pub fn canvas(&self) -> &Canvas {
        &self.canvas
    }

    #[cfg(test)]
    pub fn diagram(&self) -> &Diagram {
        &self.diagram
    }

    #[cfg(test)]
    pub fn selection(&self) -> &[String] {
        &self.canvas.selection
    }

    #[cfg(test)]
    pub fn selected_lines(&self) -> &[usize] {
        &self.canvas.lines
    }

    #[cfg(test)]
    pub fn editing_label(&self) -> bool {
        self.canvas.is_editing_label()
    }

    #[cfg(test)]
    pub fn diagram_mut(&mut self) -> &mut Diagram {
        &mut self.diagram
    }

    #[cfg(test)]
    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    /// The last copy's clipboard text.
    #[cfg(test)]
    pub fn clipboard_text(&self) -> Option<&str> {
        self.clip.as_ref().map(|(text, _)| text.as_str())
    }

    #[cfg(test)]
    pub fn has_overlay(&self) -> bool {
        self.overlay.is_some()
    }

    #[cfg(test)]
    pub fn diagram_names(&self) -> Vec<String> {
        self.project
            .as_ref()
            .map(|p| p.names.clone())
            .unwrap_or_default()
    }

    #[cfg(test)]
    pub fn doc_name(&self) -> Option<&str> {
        self.doc.as_ref().map(|d| d.name.as_str())
    }

    /// The Files pane's files' names, in order.
    #[cfg(test)]
    pub fn file_names(&self) -> Vec<String> {
        self.files
            .list
            .iter()
            .map(|f| files::name(&f.path))
            .collect()
    }

    /// The tabs' names, in order.
    #[cfg(test)]
    pub fn tab_names(&self) -> Vec<String> {
        self.files.tabs.iter().map(|t| files::name(t)).collect()
    }

    /// Whether the Files pane shows `name` as missing.
    #[cfg(test)]
    pub fn is_missing(&self, name: &str) -> bool {
        self.files
            .list
            .iter()
            .any(|f| f.missing && files::name(&f.path) == name)
    }

    #[cfg(test)]
    pub fn is_saving(&self) -> bool {
        self.backend.is_saving()
    }

    /// Runs a command as if from the palette or a shortcut.
    #[cfg(test)]
    pub fn run(&mut self, act: Act) {
        self.apply(act);
    }

    #[cfg(test)]
    pub fn flush(&self) {
        let _ = self.backend.flush();
    }

    #[cfg(test)]
    pub fn set_save_delay(&mut self, delay: Duration) {
        self.backend.set_save_delay(delay);
    }

    #[cfg(test)]
    pub fn set_export_to(&mut self, path: PathBuf) {
        self.export_to = Some(path);
    }

    #[cfg(test)]
    pub fn window_title(&self) -> &str {
        &self.title
    }

    fn view(&self) -> View {
        let kinds = self.canvas.selected_kinds(&self.diagram);
        View {
            modal_open: self.overlay.is_some() || self.palette.is_some(),
            project_open: self.project.is_some(),
            doc_open: self.doc.is_some(),
            doc_in_project: self.doc_in_project(),
            tabs: self.files.tabs.len(),
            has_content: self.has_content(),
            has_selection: self.canvas.has_selection(),
            straight_selected: self.canvas.selected_routes(&self.diagram).0,
            elbow_selected: self.canvas.selected_routes(&self.diagram).1,
            stylable: self.canvas.stylable(&self.diagram),
            fillable: self.canvas.fillable(&self.diagram),
            labellable: self.canvas.can_edit_label(&self.diagram),
            changeable: self.canvas.fillable(&self.diagram),
            plain_line_selected: kinds.any_line(),
            arrow_selected: kinds.any_arrow(),
            can_undo: self.history.can_undo(),
            can_redo: self.history.can_redo(),
        }
    }

    /// The active tab's diagram is one of the project folder's.
    fn doc_in_project(&self) -> bool {
        match (&self.doc, &self.project) {
            (Some(doc), Some(project)) => doc.path.parent() == Some(project.folder.as_path()),
            _ => false,
        }
    }

    fn top_inset(&self) -> f32 {
        if self.custom_titlebar {
            theme::TITLEBAR_HEIGHT
        } else {
            0.0
        }
    }

    fn set_prefs(&mut self, patch: Value) {
        if let Err(e) = self.paths.set_prefs(&patch) {
            self.toasts
                .error(format!("Couldn't save the preferences: {e}"));
        }
    }

    // --- Commands ------------------------------------------------------------

    fn apply(&mut self, act: Act) {
        match act {
            Act::NewDiagram => {
                if let Some(project) = &self.project {
                    let name = backend::unique_name("Untitled", &project.names);
                    self.overlay = Some(Overlay::Name(NameState::new_diagram(name)));
                }
            }
            Act::RenameDiagram => {
                if let Some(doc) = &self.doc {
                    self.overlay = Some(Overlay::Name(NameState::rename(&doc.name)));
                }
            }
            Act::DeleteDiagram => {
                if let Some(doc) = &self.doc {
                    self.overlay = Some(Overlay::ConfirmDelete(doc.name.clone()));
                }
            }
            Act::OpenFolder => self.pick_folder(),
            Act::CloseFolder => self.close_folder(),
            Act::OpenFile => self.pick_files(),
            Act::CloseTab => {
                if let Some(path) = self.files.active.clone() {
                    self.close_tab(&path);
                }
            }
            Act::CloseFile => {
                if let Some(path) = self.files.active.clone() {
                    self.close_file(&path);
                }
            }
            Act::NextTab => {
                if let Some(path) = self.files.step(1) {
                    self.open_file(&path);
                }
            }
            Act::PreviousTab => {
                if let Some(path) = self.files.step(-1) {
                    self.open_file(&path);
                }
            }
            // As Toggle Properties: hides the pane if it's showing,
            // otherwise shows it with the sidebar.
            Act::ToggleFiles => {
                let showing = self.files_visible && self.sidebar_visible;
                self.files_visible = !showing;
                let mut patch = json!({ "filesVisible": !showing });
                if !showing && !self.sidebar_visible {
                    self.sidebar_visible = true;
                    patch["sidebarVisible"] = json!(true);
                }
                self.set_prefs(patch);
            }
            Act::Export => {
                if self.has_content() {
                    self.overlay = Some(Overlay::Export(ExportState {
                        options: self.paths.prefs().export,
                        selected_only: false,
                        has_selection: self.canvas.has_selection(),
                    }));
                }
            }
            // Hides the pane if it's showing; otherwise shows it, bringing
            // back a hidden sidebar with it.
            Act::ToggleProperties => {
                let showing = self.properties_visible && self.sidebar_visible;
                self.properties_visible = !showing;
                let mut patch = json!({ "propertiesVisible": !showing });
                if !showing && !self.sidebar_visible {
                    self.sidebar_visible = true;
                    patch["sidebarVisible"] = json!(true);
                }
                self.set_prefs(patch);
            }
            Act::ToggleSidebar => {
                self.sidebar_visible = !self.sidebar_visible;
                self.set_prefs(json!({ "sidebarVisible": self.sidebar_visible }));
            }
            Act::Tool(tool) => {
                self.tool = tool;
                self.sticky_tool = self.shift;
                if tool.is_more() {
                    self.more_shape = tool;
                }
            }
            Act::MoreShapes => {
                self.tool = self.more_shape;
                self.sticky_tool = self.shift;
            }
            Act::Undo => {
                self.canvas.cancel(&mut self.diagram);
                if self.history.undo(&mut self.diagram) {
                    // Line indices may now name other lines.
                    self.canvas.lines.clear();
                }
            }
            Act::Redo => {
                self.canvas.cancel(&mut self.diagram);
                if self.history.redo(&mut self.diagram) {
                    self.canvas.lines.clear();
                }
            }
            Act::SelectAll => self.canvas.select_all(&self.diagram),
            Act::SelectShapes => self.canvas.select_shapes(&self.diagram, false),
            Act::SelectLines => self.canvas.select_lines(&self.diagram),
            Act::SelectText => self.canvas.select_shapes(&self.diagram, true),
            Act::Nudge(dx, dy) => {
                let g = self.diagram.grid;
                self.canvas
                    .nudge(&mut self.diagram, f32::from(dx) * g, f32::from(dy) * g);
            }
            Act::Copy => {
                self.copy();
            }
            Act::Cut => {
                if self.copy() {
                    self.canvas.delete_selection(&mut self.diagram);
                }
            }
            Act::Paste => self.paste_request = true,
            Act::Delete => {
                self.canvas.delete_selection(&mut self.diagram);
            }
            Act::EditLabel => {
                self.tool = Tool::Select;
                self.canvas.edit_selected_label(&self.diagram);
            }
            Act::RouteElbow => {
                self.canvas.set_route(&mut self.diagram, true);
            }
            Act::RouteStraight => {
                self.canvas.set_route(&mut self.diagram, false);
            }
            Act::ChangeShape(kind) => {
                self.canvas.change_shape(&mut self.diagram, &kind);
            }
            Act::ChangeLine(tool) => {
                self.canvas.change_line(&mut self.diagram, tool);
            }
            Act::AddArrowheads => {
                self.canvas.set_arrowheads(&mut self.diagram, true);
            }
            Act::RemoveArrowheads => {
                self.canvas.set_arrowheads(&mut self.diagram, false);
            }
            Act::Fill(swatch) => self.style(swatch, false),
            Act::Stroke(swatch) => self.style(swatch, true),
            Act::StrokeStyle(dash) => {
                self.canvas
                    .style_selection(&mut self.diagram, true, |s| s.set_dash(dash.clone()));
            }
            Act::StrokeWeight(weight) => {
                self.canvas
                    .style_selection(&mut self.diagram, true, |s| s.set_weight(weight.clone()));
            }
            Act::TextWeight(bold) => {
                self.canvas
                    .style_selection(&mut self.diagram, true, |s| s.set_bold(bold));
            }
            Act::TextStyle(italic) => {
                self.canvas
                    .style_selection(&mut self.diagram, true, |s| s.set_italic(italic));
            }
            // Normal when everything is Bold, else all Bold.
            Act::ToggleBold => {
                let all = self.canvas.selected_style(&self.diagram).bold
                    == blockliner_core::style::Agreement::Same(true);
                self.canvas
                    .style_selection(&mut self.diagram, true, |s| s.set_bold(!all));
            }
            Act::ResetStyle => self
                .canvas
                .style_selection(&mut self.diagram, true, Style::reset),
            Act::Escape => self.canvas.escape(&mut self.diagram),
            Act::ZoomIn => self.canvas.zoom_step(1),
            Act::ZoomOut => self.canvas.zoom_step(-1),
            Act::ZoomReset => self.canvas.zoom_reset(),
            Act::ToggleGrid => self.set_show_grid(!self.canvas.show_grid),
            Act::ToggleDimensions => self.set_show_dimensions(!self.canvas.show_dimensions),
            Act::ToggleStatusBar => {
                self.status_bar_visible = !self.status_bar_visible;
                self.set_prefs(json!({ "statusBarVisible": self.status_bar_visible }));
            }
            Act::Settings => self.open_settings(),
            Act::About => self.overlay = Some(Overlay::About),
            Act::Palette => self.palette = Some(CommandPalette::new()),
        }
    }

    /// The selection to the clipboard as Mermaid (`clipboard::copy`).
    /// `false` when nothing is selected.
    fn copy(&mut self) -> bool {
        let Some(clip) = clipboard::copy(&self.diagram, &self.canvas.selection, &self.canvas.lines)
        else {
            return false;
        };
        let text = blockliner_core::write(&clip);
        self.clip = Some((text.clone(), 0));
        self.copy_out = Some(text);
        true
    }

    /// Clipboard text into the diagram, selected: a flowchart (a copy, or
    /// one from elsewhere) where `clipboard::offset` puts it, else a text
    /// box in the middle of the view.
    fn paste(&mut self, text: &str) {
        if self.doc.is_none() || self.canvas.is_busy() {
            return;
        }
        let n = match &self.clip {
            Some((last, n)) if last == text => n + 1,
            _ => 1,
        };
        self.clip = Some((text.to_owned(), n));
        let view = self.canvas.visible();
        let (shapes, lines) = match clipboard::parse(text) {
            Some(clip) => {
                let (dx, dy) = clipboard::offset(&clip, n, view, self.diagram.grid);
                clipboard::paste(&mut self.diagram, &clip, dx, dy)
            }
            None => match clipboard::paste_text(&mut self.diagram, text, view.center()) {
                Some(id) => (vec![id], Vec::new()),
                None => return,
            },
        };
        self.canvas.select_only(shapes, lines);
        self.tool = Tool::Select;
    }

    /// The selected shapes' fill (or `stroke`): a preset, the default, or
    /// the Custom… dialog.
    fn style(&mut self, swatch: Swatch, stroke: bool) {
        let colour = match swatch {
            Swatch::Default => None,
            Swatch::Preset(i) => {
                let preset = PRESETS[i];
                Some(Colour::parse(if stroke {
                    preset.stroke
                } else {
                    preset.fill
                }))
            }
            Swatch::Custom => {
                // The first selected shape's (or, for a stroke, line's).
                let d = &self.diagram;
                let shapes = self
                    .canvas
                    .selection
                    .iter()
                    .filter_map(|id| d.shape(id).filter(|s| !s.external))
                    .map(|s| &s.style);
                let lines = self
                    .canvas
                    .lines
                    .iter()
                    .filter_map(|&i| d.connectors.get(i))
                    .filter(|_| stroke)
                    .map(|c| &c.style);
                let current = shapes.chain(lines).next().and_then(|s| {
                    if stroke {
                        s.stroke.as_ref()
                    } else {
                        s.fill.as_ref()
                    }
                });
                self.overlay = Some(Overlay::Colour(ColourState::new(stroke, current)));
                return;
            }
        };
        self.apply_colour(colour, stroke);
    }

    fn apply_colour(&mut self, colour: Option<Colour>, stroke: bool) {
        self.canvas.style_selection(&mut self.diagram, stroke, |s| {
            if stroke {
                s.set_stroke(colour.clone());
            } else {
                s.set_fill(colour.clone());
            }
        });
    }

    fn set_show_grid(&mut self, on: bool) {
        self.canvas.show_grid = on;
        self.set_prefs(json!({ "showGrid": on }));
    }

    fn set_show_dimensions(&mut self, on: bool) {
        self.canvas.show_dimensions = on;
        self.set_prefs(json!({ "showDimensions": on }));
    }

    fn open_settings(&mut self) {
        let prefs = self.paths.prefs();
        self.overlay = Some(Overlay::Settings(SettingsState::new(
            "ui",
            prefs.custom_titlebar,
            self.custom_titlebar,
            self.canvas.show_grid,
            self.diagram.grid,
            self.canvas.unit,
            self.canvas.show_dimensions,
        )));
    }

    // --- The folder and its diagrams ------------------------------------------

    /// Shows the folder holding diagram `name` in the OS file manager.
    fn open_containing_folder(&mut self, name: &str) {
        if let Some(path) = self.path(name) {
            self.show_folder_of(&path);
        }
    }

    /// Shows the folder holding `path` in the OS file manager.
    fn show_folder_of(&mut self, path: &Path) {
        let Some(folder) = path.parent().map(Path::to_path_buf) else {
            return;
        };
        // Tests record it rather than starting a file manager.
        #[cfg(test)]
        {
            self.opened_folders.push(folder);
        }
        #[cfg(not(test))]
        if let Err(e) = open::that_detached(&folder) {
            self.toasts.error(format!("Couldn't open the folder: {e}"));
        }
    }

    fn pick_folder(&mut self) {
        self.dialogs.start(|| {
            rfd::FileDialog::new()
                .set_title("Open Folder")
                .pick_folder()
                .map(Picked::Folder)
        });
    }

    /// Open File: one or more `.mmd` files from anywhere.
    fn pick_files(&mut self) {
        // Tests open files with `open_path`, never the dialog.
        if cfg!(test) {
            return;
        }
        let start = self
            .doc
            .as_ref()
            .and_then(|d| d.path.parent().map(Path::to_path_buf))
            .or_else(|| self.project.as_ref().map(|p| p.folder.clone()));
        self.dialogs.start(move || {
            let mut dialog = rfd::FileDialog::new()
                .set_title("Open File")
                .add_filter("Mermaid diagram", &[backend::EXT]);
            if let Some(start) = start {
                dialog = dialog.set_directory(start);
            }
            dialog.pick_files().map(Picked::Files)
        });
    }

    /// A file dialog's answer.
    fn picked(&mut self, ctx: &egui::Context) {
        let Some(picked) = self.dialogs.poll() else {
            return;
        };
        match picked {
            Picked::Folder(folder) => self.open_project(folder, None),
            Picked::Files(paths) => {
                for path in paths {
                    self.open_path(&path);
                }
            }
            Picked::Export {
                path,
                options,
                selected_only,
            } => {
                if self.export(ctx, &path, options, selected_only)
                    && matches!(self.overlay, Some(Overlay::Export(_)))
                {
                    self.overlay = None;
                }
            }
        }
    }

    /// Writes the active tab's edits, a label being edited included.
    /// `false` when the write failed (its error is shown).
    fn write_active(&mut self) -> bool {
        self.canvas.commit_label(&mut self.diagram);
        self.autosave();
        self.backend.flush()
    }

    /// Saves the open diagram, closes the folder's own files (their tabs
    /// and Files rows; files from elsewhere stay open), and goes back to
    /// "No folder open"; the next start opens no folder either. A failed
    /// save keeps the folder open (its error is shown), so closing never
    /// loses an edit.
    fn close_folder(&mut self) {
        let Some(folder) = self.project.as_ref().map(|p| p.folder.clone()) else {
            return;
        };
        if !self.write_active() {
            return;
        }
        let own = self.files.in_folder(&folder);
        self.drop_files(&own);
        self.project = None;
        self.set_prefs(json!({ "lastProject": "" }));
        self.save_files();
    }

    /// Takes `paths` out of the Files pane and the tabs, with no saving
    /// (done already, or the files are gone). If the active tab went, the
    /// next one left takes its place.
    fn drop_files(&mut self, paths: &[PathBuf]) {
        let mut next = None;
        for path in paths {
            if self.files.active.as_deref() == Some(path.as_path()) {
                self.clear_active();
            }
            self.stash.remove(path);
            self.backend.unwatch(path);
            next = self.files.close(path).or(next);
        }
        let next = next.filter(|n| !paths.contains(n));
        if self.doc.is_none() {
            self.files.active = None;
            if let Some(path) = next.or_else(|| self.files.tabs.first().cloned()) {
                self.open_file(&path);
            }
        }
    }

    /// Empties the canvas: no active tab. Nothing is written.
    fn clear_active(&mut self) {
        self.doc = None;
        self.load(self.blank());
    }

    /// A new, empty diagram on the preferred grid.
    fn blank(&self) -> Diagram {
        Diagram {
            grid: self.paths.prefs().grid_size,
            ..Diagram::new()
        }
    }

    /// Makes `folder` the project, without opening anything. `false` when
    /// it can't be read (its error is shown).
    fn set_project(&mut self, folder: PathBuf) -> bool {
        match Backend::list(&folder) {
            Ok(names) => {
                self.set_prefs(json!({ "lastProject": folder.to_string_lossy() }));
                self.project = Some(Project { folder, names });
                true
            }
            Err(e) => {
                self.toasts
                    .error(format!("Couldn't open {}: {e}", folder.display()));
                false
            }
        }
    }

    /// Opens `folder` as the project, closing the last project's files
    /// first (as Close Folder does), and opens the diagram `want` in it.
    /// Without `want`, its first diagram opens when no tab is open.
    pub fn open_project(&mut self, folder: PathBuf, want: Option<String>) {
        if let Err(e) = Backend::list(&folder) {
            self.toasts
                .error(format!("Couldn't open {}: {e}", folder.display()));
            return;
        }
        let old = self.project.as_ref().map(|p| p.folder.clone());
        if let Some(old) = old.filter(|old| *old != folder) {
            if !self.write_active() {
                return;
            }
            let own = self.files.in_folder(&old);
            self.drop_files(&own);
        }
        if !self.set_project(folder) {
            return;
        }
        let names = self
            .project
            .as_ref()
            .map(|p| p.names.clone())
            .unwrap_or_default();
        let pick = match want {
            Some(want) => names.iter().find(|n| **n == want).cloned(),
            None if self.files.tabs.is_empty() => names.first().cloned(),
            None => None,
        };
        if let Some(name) = pick {
            self.open_diagram(&name);
        }
        self.save_files();
    }

    /// Opens what the command line (or Open File) names: a folder of
    /// diagrams as the project, or one `.mmd` file in a tab.
    pub fn open_path(&mut self, path: &Path) {
        let path = std::path::absolute(path).unwrap_or_else(|_| path.to_owned());
        if path.is_dir() {
            self.open_project(path, None);
        } else if !path.is_file() {
            self.toasts
                .error(format!("Couldn't open {}: no such file", path.display()));
        } else if path.extension().is_some_and(|e| e == backend::EXT) {
            self.open_file(&path);
        } else {
            self.toasts.error(format!(
                "Couldn't open {}: BlockLiner opens .{} diagrams",
                path.display(),
                backend::EXT
            ));
        }
    }

    fn path(&self, name: &str) -> Option<PathBuf> {
        Some(backend::file(&self.project.as_ref()?.folder, name))
    }

    /// Puts `d` on the canvas as a fresh start: no selection, no undo.
    fn load(&mut self, d: Diagram) {
        self.canvas.clear(true);
        self.history.reset(&d);
        self.diagram = d;
    }

    /// The project's diagram `name`, in a tab.
    fn open_diagram(&mut self, name: &str) {
        if let Some(path) = self.path(name) {
            self.open_file(&path);
        }
    }

    /// Opens `path` in a tab (adding it to the Files pane) and makes it the
    /// active tab. The tab it replaces has its edits written and its state
    /// put aside. `false` when the file can't be read; nothing changes then
    /// but the file's Files row, which shows as missing if it's gone.
    pub fn open_file(&mut self, path: &Path) -> bool {
        let path = std::path::absolute(path).unwrap_or_else(|_| path.to_owned());
        if self.doc.as_ref().is_some_and(|d| d.path == path) {
            self.reveal_tab = true;
            return true;
        }
        // Read before leaving the active tab, so a bad file changes nothing.
        let fresh = if self.stash.contains_key(&path) {
            None
        } else {
            let opened = Backend::read(&path).and_then(|text| {
                let d = blockliner_core::read(&text)?;
                Ok((text, d))
            });
            match opened {
                Ok(opened) => Some(opened),
                Err(e) => {
                    let name = files::name(&path);
                    self.toasts.error(format!("Couldn't open “{name}”: {e:#}"));
                    if !path.exists() {
                        self.mark_missing(&path);
                    }
                    if self.doc.is_none() {
                        self.files.active = None;
                    }
                    return false;
                }
            }
        };
        self.leave_tab();
        let name = files::name(&path);
        match fresh {
            Some((text, d)) => {
                self.backend.watch(&path, &text);
                self.doc = Some(Doc {
                    path: path.clone(),
                    name,
                    saved: d.clone(),
                });
                self.load(d);
            }
            None => {
                if let Some(s) = self.stash.remove(&path) {
                    self.canvas.clear(false);
                    self.canvas.view = s.view;
                    self.diagram = s.diagram;
                    self.history = s.history;
                    self.doc = Some(Doc {
                        path: path.clone(),
                        name,
                        saved: s.saved,
                    });
                }
            }
        }
        self.files.open(&path);
        self.files.active = Some(path);
        self.reveal_tab = true;
        self.save_files();
        true
    }

    /// Puts the active tab aside, its edits written first, so another tab
    /// can have the canvas.
    fn leave_tab(&mut self) {
        self.write_active();
        if let Some(doc) = self.doc.take() {
            let blank = self.blank();
            let history = std::mem::replace(&mut self.history, History::new(&blank));
            let stash = Stash {
                diagram: std::mem::replace(&mut self.diagram, blank),
                saved: doc.saved,
                history,
                view: self.canvas.view,
            };
            self.stash.insert(doc.path, stash);
        }
        self.canvas.clear(true);
    }

    /// Closes `path`'s tab; the file stays in the Files pane. The active
    /// tab's edits are written first: if that fails, the tab stays open and
    /// `false` comes back.
    fn close_tab(&mut self, path: &Path) -> bool {
        if !self.files.has_tab(path) {
            return true;
        }
        if self.files.active.as_deref() == Some(path) {
            if !self.write_active() {
                return false;
            }
            self.clear_active();
        }
        self.stash.remove(path);
        self.backend.unwatch(path);
        if let Some(next) = self.files.close_tab(path) {
            self.files.active = None;
            self.open_file(&next);
        }
        self.save_files();
        true
    }

    /// Closes `path`: its tab, if it has one, and its Files row.
    fn close_file(&mut self, path: &Path) {
        if self.close_tab(path) {
            self.files.close(path);
            self.save_files();
        }
    }

    /// The Files row for `path` shows as missing, and its tab (if it was
    /// never shown, so nothing is lost) closes.
    fn mark_missing(&mut self, path: &Path) {
        for f in &mut self.files.list {
            if f.path == path {
                f.missing = true;
            }
        }
        if self.files.active.as_deref() != Some(path) {
            self.stash.remove(path);
            self.files.close_tab(path);
        }
    }

    /// Writes the Files pane, the tabs and the active tab to the prefs.
    fn save_files(&mut self) {
        let list = |paths: Vec<&PathBuf>| -> Vec<String> {
            paths
                .into_iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect()
        };
        let open_files = list(self.files.list.iter().map(|f| &f.path).collect());
        let open_tabs = list(self.files.tabs.iter().collect());
        let active = self
            .files
            .active
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.set_prefs(json!({
            "openFiles": open_files,
            "openTabs": open_tabs,
            "activeFile": active,
            "lastDiagram": null,
        }));
    }

    /// The folder's diagrams, read again.
    fn relist(&mut self) {
        let Some(project) = &mut self.project else {
            return;
        };
        if let Ok(names) = Backend::list(&project.folder) {
            project.names = names;
        }
    }

    /// Whether another diagram than `except` is called `name`, ignoring
    /// case: the folder may also be used where file names ignore it.
    fn taken(&self, name: &str, except: Option<&str>) -> bool {
        let name = name.to_lowercase();
        self.project.as_ref().is_some_and(|p| {
            p.names
                .iter()
                .any(|n| Some(n.as_str()) != except && n.to_lowercase() == name)
        })
    }

    /// A new, empty diagram called `name`, opened in a tab.
    fn create(&mut self, name: &str) -> anyhow::Result<()> {
        if self.taken(name, None) {
            anyhow::bail!(backend::TAKEN);
        }
        let path = self
            .path(name)
            .ok_or_else(|| anyhow::anyhow!("No folder is open."))?;
        self.backend
            .create(&path, &blockliner_core::write(&self.blank()))?;
        self.relist();
        self.open_diagram(name);
        Ok(())
    }

    fn rename(&mut self, from: &str, to: &str) -> anyhow::Result<()> {
        if from == to {
            return Ok(());
        }
        if self.taken(to, Some(from)) {
            anyhow::bail!(backend::TAKEN);
        }
        let (Some(a), Some(b)) = (self.path(from), self.path(to)) else {
            return Ok(());
        };
        self.backend.rename(&a, &b)?;
        self.relist();
        self.files.rename(&a, &b);
        if let Some(stash) = self.stash.remove(&a) {
            self.stash.insert(b.clone(), stash);
        }
        if let Some(doc) = self.doc.as_mut().filter(|d| d.path == a) {
            doc.path = b;
            doc.name = to.to_owned();
        }
        self.save_files();
        Ok(())
    }

    /// Deletes the diagram `name`, closing its tab and Files row. If it was
    /// the active tab, the next tab takes its place, else (no tab left) its
    /// neighbour in the folder opens.
    fn delete(&mut self, name: &str) {
        let Some(path) = self.path(name) else {
            return;
        };
        let index = self
            .project
            .as_ref()
            .and_then(|p| p.names.iter().position(|n| n == name));
        if let Err(e) = self.backend.delete(&path) {
            self.toasts
                .error(format!("Couldn't delete “{name}”: {e:#}"));
            return;
        }
        self.relist();
        let was_active = self.files.active.as_deref() == Some(path.as_path());
        self.drop_files(&[path]);
        if was_active && self.doc.is_none() {
            let names = self
                .project
                .as_ref()
                .map(|p| p.names.clone())
                .unwrap_or_default();
            if let Some(next) = index.and_then(|i| names.get(i.min(names.len().saturating_sub(1))))
            {
                self.open_diagram(&next.clone());
            }
        }
        self.save_files();
    }

    /// Hands the diagram to the autosave when it changed. Only between
    /// gestures, so a drag is written once, when it ends.
    fn autosave(&mut self) {
        let Some(doc) = &mut self.doc else {
            return;
        };
        if self.diagram != doc.saved {
            doc.saved = self.diagram.clone();
            self.backend
                .save(doc.path.clone(), blockliner_core::write(&self.diagram));
        }
    }

    /// About once a second: the folder's diagrams, which open files are
    /// missing, and whether an open tab's file changed on disk. An outside
    /// edit is reloaded (as an undoable step); a deleted file closes its
    /// tab and shows as missing in the Files pane.
    fn watch_folder(&mut self, ctx: &egui::Context) {
        if self.project.is_none() && self.files.list.is_empty() {
            return;
        }
        let now = ctx.input(|i| i.time);
        ctx.request_repaint_after(Duration::from_secs_f64(SCAN_SECONDS));
        if now - self.last_scan < SCAN_SECONDS {
            return;
        }
        self.last_scan = now;
        self.relist();
        // Files without a loaded tab: just whether they're there.
        let unloaded: Vec<PathBuf> = self
            .files
            .list
            .iter()
            .map(|f| f.path.clone())
            .filter(|p| {
                !self.stash.contains_key(p) && self.doc.as_ref().is_none_or(|d| d.path != *p)
            })
            .collect();
        for path in unloaded {
            if path.exists() {
                for f in &mut self.files.list {
                    if f.path == path {
                        f.missing = false;
                    }
                }
            } else {
                self.mark_missing(&path);
            }
        }
        if self.canvas.is_busy() {
            return;
        }
        for (path, change) in self.backend.check() {
            self.outside_change(&path, change);
        }
    }

    /// `path`, an open tab's file, was changed or removed by something else.
    fn outside_change(&mut self, path: &Path, change: Change) {
        let name = files::name(path);
        let active = self.doc.as_ref().is_some_and(|d| d.path == path);
        match change {
            Change::Removed => {
                if active {
                    self.clear_active();
                }
                self.stash.remove(path);
                for f in &mut self.files.list {
                    if f.path == path {
                        f.missing = true;
                    }
                }
                if let Some(next) = self.files.close_tab(path) {
                    self.files.active = None;
                    self.open_file(&next);
                }
                self.save_files();
                self.toasts.info(format!(
                    "“{name}” was deleted or renamed outside BlockLiner."
                ));
            }
            Change::Edited(text) => match blockliner_core::read(&text) {
                Ok(d) if active => {
                    if d != self.diagram {
                        self.canvas.clear(false);
                        self.diagram = d.clone();
                        self.toasts
                            .info(format!("Reloaded “{name}”: it changed on disk."));
                    }
                    if let Some(doc) = &mut self.doc {
                        doc.saved = d;
                    }
                }
                Ok(d) => {
                    if let Some(s) = self.stash.get_mut(path) {
                        if d != s.diagram {
                            s.diagram = d.clone();
                            s.history.observe(&s.diagram);
                            self.toasts
                                .info(format!("Reloaded “{name}”: it changed on disk."));
                        }
                        s.saved = d;
                    }
                }
                Err(e) => self
                    .toasts
                    .error(format!("“{name}” changed on disk and can't be read: {e}")),
            },
        }
    }

    fn backend_events(&mut self) {
        for event in self.backend.poll() {
            match event {
                Event::Saved(_) => {}
                Event::SaveFailed { path, message } => {
                    let name = path.file_stem().map(|s| s.to_string_lossy().into_owned());
                    self.toasts.error(format!(
                        "Couldn't save “{}”: {message}",
                        name.unwrap_or_default()
                    ));
                }
            }
        }
    }

    /// "Name — BlockLiner" while a diagram is open.
    fn update_title(&mut self, ctx: &egui::Context) {
        let title = match &self.doc {
            Some(doc) => format!("{} — BlockLiner", doc.name),
            None => "BlockLiner".to_owned(),
        };
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    // --- Export ----------------------------------------------------------------

    fn has_content(&self) -> bool {
        self.doc.is_some()
            && (!self.diagram.shapes.is_empty() || !self.diagram.connectors.is_empty())
    }

    fn export_scope(&self, selected_only: bool) -> Option<blockliner_core::export::Scope> {
        use blockliner_core::export::{scope, Objects};
        let objects = if selected_only {
            Objects::Selected {
                shapes: &self.canvas.selection,
                lines: &self.canvas.lines,
            }
        } else {
            Objects::All
        };
        scope(&self.diagram, objects)
    }

    /// Asks where to export (the answer comes to [`Self::picked`]).
    fn ask_export(&mut self, state: &ExportState) {
        let Some(doc) = &self.doc else {
            return;
        };
        let folder = doc.path.parent().map(Path::to_path_buf).unwrap_or_default();
        let o = state.options;
        let suffix = if state.selected_only {
            " (selection)"
        } else {
            ""
        };
        let name = format!("{}{suffix}.{}", doc.name, o.format.ext());
        let ask = self.save_dialog(folder, name, o.format);
        let selected_only = state.selected_only;
        self.dialogs.start(move || {
            ask().map(|path| Picked::Export {
                path,
                options: o,
                selected_only,
            })
        });
    }

    /// The system save dialog, in the diagram's folder.
    #[cfg(not(test))]
    fn save_dialog(
        &self,
        folder: PathBuf,
        name: String,
        format: export::Format,
    ) -> impl FnOnce() -> Option<PathBuf> + Send + 'static {
        move || {
            rfd::FileDialog::new()
                .set_title("Export Diagram")
                .set_directory(folder)
                .set_file_name(name)
                .add_filter(format.label(), &[format.ext()])
                .save_file()
        }
    }

    /// Tests never open the save dialog.
    #[cfg(test)]
    fn save_dialog(
        &self,
        _: PathBuf,
        _: String,
        _: export::Format,
    ) -> impl FnOnce() -> Option<PathBuf> + Send + 'static {
        let to = self.export_to.clone();
        move || to
    }

    /// Writes the image to `path`. Returns whether it was written (the
    /// Export dialog then closes).
    fn export(
        &mut self,
        ctx: &egui::Context,
        path: &Path,
        o: export::Options,
        selected_only: bool,
    ) -> bool {
        let Some(scope) = self.export_scope(selected_only) else {
            self.toasts
                .error("Couldn't export: there's nothing to export.");
            return false;
        };
        let written = export::render(ctx, &self.diagram, &scope, &o)
            .and_then(|bytes| export::write(path, &bytes));
        match written {
            Ok(()) => {
                let file = path.file_name().map(|f| f.to_string_lossy().into_owned());
                self.toasts
                    .success(format!("Exported “{}”", file.unwrap_or_default()));
                self.set_prefs(json!({
                    "exportFormat": o.format.ext(),
                    "exportGrid": o.grid,
                    "exportScale": o.scale,
                }));
                true
            }
            Err(e) => {
                self.toasts.error(format!("Couldn't export: {e:#}"));
                false
            }
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
            self.apply(act);
        }
        self.nudge_keys(ctx);
        self.clipboard_events(ctx);
    }

    /// egui's copy, cut and paste events: the platform's keys, or the
    /// clipboard a Paste command asked for. Called only while no text field
    /// has the keyboard (a label being edited keeps them).
    fn clipboard_events(&mut self, ctx: &egui::Context) {
        if self.view().modal_open || self.doc.is_none() {
            return;
        }
        let events = ctx.input_mut(|i| {
            let mut out = Vec::new();
            i.events.retain(|e| match e {
                egui::Event::Copy | egui::Event::Cut | egui::Event::Paste(_) => {
                    out.push(e.clone());
                    false
                }
                _ => true,
            });
            out
        });
        for e in events {
            match e {
                egui::Event::Copy => self.apply(Act::Copy),
                egui::Event::Cut => self.apply(Act::Cut),
                egui::Event::Paste(text) => self.paste(&text),
                _ => {}
            }
        }
    }

    /// The arrow keys nudge the selection: one grid step, five with Shift,
    /// one pixel with Alt. Read from the key events themselves, which carry
    /// their modifiers.
    fn nudge_keys(&mut self, ctx: &egui::Context) {
        if self.view().modal_open || !self.canvas.has_selection() {
            return;
        }
        let grid = self.diagram.grid;
        let moves: Vec<(f32, f32)> = ctx.input_mut(|i| {
            let mut moves = Vec::new();
            i.events.retain(|e| {
                let egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } = e
                else {
                    return true;
                };
                let (dx, dy) = match key {
                    egui::Key::ArrowLeft => (-1.0, 0.0),
                    egui::Key::ArrowRight => (1.0, 0.0),
                    egui::Key::ArrowUp => (0.0, -1.0),
                    egui::Key::ArrowDown => (0.0, 1.0),
                    _ => return true,
                };
                let step = if modifiers.alt {
                    1.0
                } else if modifiers.shift {
                    5.0 * grid
                } else {
                    grid
                };
                moves.push((dx * step, dy * step));
                false
            });
            moves
        });
        for (dx, dy) in moves {
            self.canvas.nudge(&mut self.diagram, dx, dy);
        }
    }

    /// The whole UI, once per frame.
    pub fn show(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.init(&ctx);
        self.shift = ctx.input(|i| i.modifiers.shift);
        self.backend_events();
        self.picked(&ctx);
        self.watch_folder(&ctx);
        self.dispatch_keys(&ctx);

        if self.custom_titlebar {
            self.titlebar(ui);
        }
        self.main_view(ui);
        self.overlays(&ctx);
        self.palette(&ctx);
        self.toasts.show(&ctx);
        if let Some(text) = self.copy_out.take() {
            ctx.copy_text(text);
        }
        if std::mem::take(&mut self.paste_request) {
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
        }
        self.update_title(&ctx);
        if self.custom_titlebar {
            window::window_edges(&ctx);
        }
        if let Some(shot) = &mut self.screenshot {
            shot.tick(&ctx);
        }
    }

    fn titlebar(&mut self, ui: &mut Ui) {
        let bar = TitleBar {
            app_name: "BlockLiner",
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
                // The title bar's panel toggle shows and hides the sidebar.
                TitleBarAction::ToggleToolbar => act = Some(Act::ToggleSidebar),
                // Already applied by the title bar; saved with the theme.
                TitleBarAction::ScaleChanged(_) => {}
            }
        }
        if let Some(a) = act {
            self.apply(a);
        }
    }

    fn main_view(&mut self, ui: &mut Ui) {
        let props = ToolbarProps {
            tool: self.tool,
            more_shape: self.more_shape,
            can_close_folder: self.project.is_some(),
            properties_open: self.properties_visible && self.sidebar_visible,
            can_export: self.has_content(),
            export_open: matches!(self.overlay, Some(Overlay::Export(_))),
            settings_open: matches!(self.overlay, Some(Overlay::Settings(_))),
            about_open: matches!(self.overlay, Some(Overlay::About)),
        };
        let mut acts = views::toolbar(ui, &props);
        if self.status_bar_visible {
            let save = match &self.doc {
                None => SaveState::None,
                Some(_) if self.backend.is_saving() => SaveState::Saving,
                Some(_) => SaveState::Saved,
            };
            let status = StatusProps {
                tool: self.tool,
                save,
                pointer: self.canvas.pointer,
                selected: self.canvas.selected_count(),
                free_lines: self.diagram.has_free_lines(),
                limitation: self.canvas.limitation(&self.diagram),
                grid: self.diagram.grid,
                unit: self.canvas.unit,
                zoom: self.canvas.view.zoom,
                scale: &self.theme.scale,
            };
            let clicked = egui::Panel::bottom("blockliner-status")
                .exact_size(views::STATUS_BAR_HEIGHT)
                .resizable(false)
                .show_separator_line(false)
                .frame(Frame::NONE)
                .show(ui, |ui| views::status_bar(ui, &status));
            acts.extend(clicked.inner);
        }
        let mut side = Vec::new();
        let mut file_acts = Vec::new();
        let mut pane = None;
        let props = PropertiesProps::new(
            &self.diagram,
            &self.canvas.selection,
            self.canvas.selected_style(&self.diagram),
            self.canvas.selected_kinds(&self.diagram),
        );
        let has_files = !self.files.list.is_empty();
        if self.sidebar_visible && (self.project.is_some() || has_files) {
            let p = theme::palette(ui.ctx());
            let (min, default, max) = views::SIDEBAR_WIDTH;
            let folder = self.project.as_ref().map(|project| {
                project
                    .folder
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| project.folder.display().to_string())
                    .to_uppercase()
            });
            let open = self
                .doc
                .as_ref()
                .filter(|_| self.doc_in_project())
                .map(|d| d.name.clone());
            let rows = self.file_rows();
            let mut list = Vec::new();
            if let Some(folder) = &folder {
                list.push(
                    Pane::new(PROJECT_PANE, folder)
                        .min_height(LIST_MIN_HEIGHT)
                        .closable(false),
                );
            }
            if self.files_visible {
                list.push(Pane::new(FILES_PANE, "FILES").min_height(LIST_MIN_HEIGHT));
            }
            if self.properties_visible {
                list.push(
                    Pane::new(PROPERTIES_PANE, "PROPERTIES").min_height(properties::MIN_HEIGHT),
                );
            }
            let mut extra = None;
            let host = egui::Panel::left("blockliner-sidebar")
                .resizable(true)
                .default_size(default)
                .size_range(min..=max)
                .frame(Frame::NONE.fill(p.bg_surface))
                .show(ui, |ui| {
                    let names = self
                        .project
                        .as_ref()
                        .map(|p| p.names.as_slice())
                        .unwrap_or(&[]);
                    panes::show(
                        ui,
                        ui.scope_id().with("blockliner-panes"),
                        &mut self.panes,
                        &list,
                        |ui, id| match id {
                            PROJECT_PANE => side = views::project_list(ui, names, open.as_deref()),
                            FILES_PANE => file_acts = views::files_list(ui, &rows),
                            _ => pane = properties::show(ui, &mut self.properties, &props),
                        },
                        |ui, id| {
                            let (key, glyph, label, act) = match id {
                                PROJECT_PANE => (
                                    "new-diagram",
                                    icons::PLUS,
                                    "New Diagram (Ctrl+N)",
                                    Act::NewDiagram,
                                ),
                                FILES_PANE => (
                                    "open-file",
                                    icons::FILE_IMPORT,
                                    "Open File (Ctrl+Shift+O)",
                                    Act::OpenFile,
                                ),
                                _ => return,
                            };
                            if views::pane_button(ui, key, glyph, label) {
                                extra = Some(act);
                            }
                        },
                    )
                });
            acts.extend(extra);
            match host.inner.closed.as_deref() {
                Some(FILES_PANE) => acts.push(Act::ToggleFiles),
                Some(PROPERTIES_PANE) => acts.push(Act::ToggleProperties),
                _ => {}
            }
        }
        let mut tab_act = None;
        if !self.files.tabs.is_empty() {
            let tabs = self.tab_items();
            let reveal = std::mem::take(&mut self.reveal_tab);
            tab_act = egui::Panel::top("blockliner-tabs")
                .exact_size(views::TAB_HEIGHT)
                .resizable(false)
                .show_separator_line(false)
                .frame(Frame::NONE)
                .show(ui, |ui| views::tab_strip(ui, &tabs, reveal))
                .inner;
        }
        let keys = !self.view().modal_open && !ui.ctx().egui_wants_keyboard_input();
        egui::CentralPanel::default()
            .frame(Frame::NONE)
            .show(ui, |ui| {
                if self.doc.is_none() {
                    acts.extend(views::empty_state(ui, self.project.is_some(), has_files));
                    return;
                }
                let out = self.canvas.show(ui, &mut self.diagram, self.tool, keys);
                if out.tool_done && !self.sticky_tool {
                    self.tool = Tool::Select;
                }
                acts.extend(out.menu.map(|m| match m {
                    MenuAction::EditLabel => Act::EditLabel,
                    MenuAction::ChangeShape(kind) => Act::ChangeShape(kind),
                    MenuAction::AddArrowheads => Act::AddArrowheads,
                    MenuAction::RemoveArrowheads => Act::RemoveArrowheads,
                    MenuAction::Cut => Act::Cut,
                    MenuAction::Copy => Act::Copy,
                    MenuAction::Paste => Act::Paste,
                    MenuAction::RouteElbow => Act::RouteElbow,
                    MenuAction::RouteStraight => Act::RouteStraight,
                    MenuAction::Fill(s) => Act::Fill(s),
                    MenuAction::Stroke(s) => Act::Stroke(s),
                    MenuAction::StrokeStyle(d) => Act::StrokeStyle(d),
                    MenuAction::StrokeWeight(w) => Act::StrokeWeight(w),
                    MenuAction::TextWeight(b) => Act::TextWeight(b),
                    MenuAction::TextStyle(i) => Act::TextStyle(i),
                    MenuAction::ResetStyle => Act::ResetStyle,
                    MenuAction::Delete => Act::Delete,
                    MenuAction::SelectAll => Act::SelectAll,
                    MenuAction::SelectShapes => Act::SelectShapes,
                    MenuAction::SelectLines => Act::SelectLines,
                    MenuAction::SelectText => Act::SelectText,
                }));
            });
        for act in acts {
            self.apply(act);
        }
        match pane {
            Some(PropertiesAction::Fill(s)) => self.apply(Act::Fill(s)),
            Some(PropertiesAction::Stroke(s)) => self.apply(Act::Stroke(s)),
            Some(PropertiesAction::FillHex(c)) => self.apply_colour(Some(c), false),
            Some(PropertiesAction::StrokeHex(c)) => self.apply_colour(Some(c), true),
            Some(PropertiesAction::StrokeStyle(d)) => self.apply(Act::StrokeStyle(d)),
            Some(PropertiesAction::StrokeWeight(w)) => self.apply(Act::StrokeWeight(w)),
            Some(PropertiesAction::TextWeight(b)) => self.apply(Act::TextWeight(b)),
            Some(PropertiesAction::TextStyle(i)) => self.apply(Act::TextStyle(i)),
            Some(PropertiesAction::ResetStyle) => self.apply(Act::ResetStyle),
            Some(PropertiesAction::ChangeShape(kind)) => self.apply(Act::ChangeShape(kind)),
            Some(PropertiesAction::ChangeLine(tool)) => self.apply(Act::ChangeLine(tool)),
            None => {}
        }
        match tab_act {
            Some(TabAction::Activate(i)) => {
                if let Some(path) = self.files.tabs.get(i).cloned() {
                    self.open_file(&path);
                }
            }
            Some(TabAction::Close(i)) => {
                if let Some(path) = self.files.tabs.get(i).cloned() {
                    self.close_tab(&path);
                }
            }
            None => {}
        }
        for action in file_acts {
            let path = |i: usize| self.files.list.get(i).map(|f| (f.path.clone(), f.missing));
            match action {
                FilesAction::Open(i) => match path(i) {
                    Some((path, true)) => self.toasts.error(format!(
                        "Couldn't open “{}”: it can't be found.",
                        files::name(&path)
                    )),
                    Some((path, false)) => {
                        self.open_file(&path);
                    }
                    None => {}
                },
                FilesAction::Close(i) => {
                    if let Some((path, _)) = path(i) {
                        self.close_file(&path);
                    }
                }
                FilesAction::OpenContainingFolder(i) => {
                    if let Some((path, _)) = path(i) {
                        self.show_folder_of(&path);
                    }
                }
            }
        }
        for action in side {
            match action {
                SidebarAction::Open(name) => self.open_diagram(&name),
                SidebarAction::Rename(name) => {
                    self.overlay = Some(Overlay::Name(NameState::rename(&name)));
                }
                SidebarAction::Delete(name) => self.overlay = Some(Overlay::ConfirmDelete(name)),
                SidebarAction::OpenContainingFolder(name) => self.open_containing_folder(&name),
            }
        }
        // One undo step per finished change (see `History`), and the
        // autosave after it.
        if !self.canvas.is_busy() {
            self.history.observe(&self.diagram);
            self.autosave();
        }
    }

    /// The Files pane's rows.
    fn file_rows(&self) -> Vec<FileRow> {
        let project = self.project.as_ref().map(|p| p.folder.as_path());
        self.files
            .list
            .iter()
            .map(|f| FileRow {
                name: files::name(&f.path),
                hint: files::hint(&f.path, project, &self.files),
                path: f.path.display().to_string(),
                active: self.files.active.as_ref() == Some(&f.path),
                tabbed: self.files.has_tab(&f.path),
                missing: f.missing,
            })
            .collect()
    }

    /// The tab strip's tabs.
    fn tab_items(&self) -> Vec<TabItem> {
        self.files
            .tabs
            .iter()
            .map(|t| TabItem {
                name: files::name(t),
                path: t.display().to_string(),
                active: self.files.active.as_ref() == Some(t),
            })
            .collect()
    }

    // --- Dialogs ---------------------------------------------------------------

    fn overlays(&mut self, ctx: &egui::Context) {
        let Some(mut overlay) = self.overlay.take() else {
            return;
        };
        let top_inset = self.top_inset();
        let mut keep = true;
        match &mut overlay {
            Overlay::Settings(state) => {
                for a in overlays::settings::show(ctx, state, &mut self.theme, top_inset) {
                    match a {
                        SettingsAction::Close => keep = false,
                        // Applied already; eframe saves it with the theme.
                        SettingsAction::ThemeChanged => {}
                        SettingsAction::SetCustomTitlebar(on) => {
                            self.set_prefs(json!({ "customTitlebar": on }));
                        }
                        SettingsAction::Restart => restart(ctx),
                        SettingsAction::SetShowGrid(on) => self.set_show_grid(on),
                        SettingsAction::SetGridSize(g) => {
                            self.diagram.grid = g;
                            self.set_prefs(json!({ "gridSize": g as i64 }));
                        }
                        SettingsAction::SetGridUnit(u) => {
                            self.canvas.unit = u;
                            self.set_prefs(json!({ "gridUnit": u.id() }));
                        }
                        SettingsAction::SetShowDimensions(on) => self.set_show_dimensions(on),
                    }
                }
            }
            Overlay::About => {
                let modal = AboutModal::new("BlockLiner", env!("CARGO_PKG_VERSION"))
                    .description(DESCRIPTION)
                    .repo(REPO_URL, Some("github.com/NathanLaan/laankit"))
                    .icon(AboutIcon::Glyph(icons::OBJECT_GROUP));
                match modal.show(ctx) {
                    Some(AboutAction::Close) => keep = false,
                    Some(AboutAction::OpenUrl(url)) => ctx.open_url(egui::OpenUrl::new_tab(url)),
                    Some(_) | None => {}
                }
            }
            Overlay::Export(state) => {
                let scope = self.export_scope(state.selected_only);
                let size = scope.as_ref().map(|s| export::size(s, &state.options));
                match overlays::export::show(ctx, state, size, self.dialogs.busy()) {
                    Some(ExportAction::Close) => keep = false,
                    Some(ExportAction::Export) => self.ask_export(state),
                    None => {}
                }
            }
            Overlay::Name(state) => match overlays::name::show(ctx, state) {
                Some(NameAction::Close) => keep = false,
                Some(NameAction::Submit(text)) => {
                    let result = backend::valid_name(&text)
                        .map_err(anyhow::Error::msg)
                        .and_then(|name| match state.renaming.clone() {
                            Some(from) => self.rename(&from, &name),
                            None => self.create(&name),
                        });
                    match result {
                        Ok(()) => keep = false,
                        Err(e) => state.refuse(format!("{e:#}")),
                    }
                }
                None => {}
            },
            Overlay::Colour(state) => match overlays::colour::show(ctx, state) {
                Some(ColourAction::Close) => keep = false,
                Some(ColourAction::Apply(c)) => {
                    let stroke = state.stroke;
                    self.apply_colour(Some(c), stroke);
                    keep = false;
                }
                None => {}
            },
            Overlay::ConfirmDelete(name) => {
                let message = format!("Delete “{name}”?");
                match dialog::confirm(
                    ctx,
                    "blockliner-delete",
                    "Delete Diagram",
                    &message,
                    Some("The file is deleted from the folder. This can't be undone."),
                    "Delete",
                ) {
                    Some(Confirm::Yes) => {
                        let name = name.clone();
                        self.delete(&name);
                        keep = false;
                    }
                    Some(Confirm::No) => keep = false,
                    None => {}
                }
            }
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
        eframe::set_value(storage, PANES_KEY, &self.panes);
    }
}

impl eframe::App for BlockLinerApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.save_state(storage);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.backend.flush();
    }
}

/// The sidebar's first layout: short lists, so Properties (last, taking
/// the rest) has room.
fn default_panes() -> PaneLayout {
    PaneLayout {
        order: [PROJECT_PANE, FILES_PANE, PROPERTIES_PANE]
            .map(String::from)
            .to_vec(),
        heights: [(PROJECT_PANE, 120.0), (FILES_PANE, 120.0)]
            .map(|(id, h)| (id.to_string(), h))
            .into(),
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
