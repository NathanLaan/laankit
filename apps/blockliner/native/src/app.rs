//! The app: owns the open folder and diagram, the canvas, the current tool
//! and the open dialog; routes commands (keyboard, palette, toolbar,
//! sidebar) and dialog actions, autosaves, and picks up outside changes.

use std::path::PathBuf;
use std::time::Duration;

use blockliner_core::history::History;
use blockliner_core::Diagram;
use egui::{Frame, Ui};
use marina_egui::about::{AboutAction, AboutIcon, AboutModal};
use marina_egui::commands::{Recents, Registry};
use marina_egui::diagnostics::{self, Screenshot};
use marina_egui::dialog::{self, Confirm};
use marina_egui::palette::{CommandPalette, PaletteOutcome};
use marina_egui::theme::{self, ThemeState};
use marina_egui::titlebar::{self, TitleBar, TitleBarAction};
use marina_egui::toast::Toasts;
use marina_egui::{fonts, icons, window};
use serde_json::{json, Value};

use crate::backend::{self, Backend, Change, Event};
use crate::canvas::{Canvas, Tool};
use crate::commands::{self, Act, View};
use crate::config::Paths;
use crate::overlays::name::{NameAction, NameState};
use crate::overlays::settings::{SettingsAction, SettingsState};
use crate::overlays::{self, Overlay};
use crate::views::{self, SaveState, SidebarAction, StatusProps, ToolbarProps};

/// eframe storage keys.
const THEME_KEY: &str = "blockliner-theme";
const RECENTS_KEY: &str = "blockliner-recents";
const DESCRIPTION: &str =
    "A diagram editor for software architecture, saving Mermaid flowcharts. Built on the marina framework.";
const REPO_URL: &str = "https://github.com/NathanLaan/marina";
/// How often to look at the folder for outside changes, in seconds.
const SCAN_SECONDS: f64 = 1.0;

/// The open folder.
struct Project {
    folder: PathBuf,
    /// Its diagrams, as the sidebar lists them.
    names: Vec<String>,
}

/// The diagram open on the canvas.
struct Doc {
    name: String,
    /// The diagram as last read or handed to the autosave: the canvas has
    /// unsaved edits while it differs.
    saved: Diagram,
}

pub struct BlockLinerApp {
    paths: Paths,
    theme: ThemeState,
    /// Whether the window was built undecorated, with Marina's title bar.
    custom_titlebar: bool,
    sidebar_visible: bool,
    status_bar_visible: bool,
    initialized: bool,
    backend: Backend,
    project: Option<Project>,
    doc: Option<Doc>,
    /// egui time of the last look at the folder.
    last_scan: f64,
    /// The window title last sent.
    title: String,
    /// The open diagram; an empty one while none is open.
    diagram: Diagram,
    history: History,
    canvas: Canvas,
    tool: Tool,
    /// The tool was picked with Shift, so it stays after each shape.
    sticky_tool: bool,
    /// Shift is down this frame (for picking a tool with it).
    shift: bool,
    overlay: Option<Overlay>,
    palette: Option<CommandPalette>,
    registry: Registry<Act, View>,
    toasts: Toasts,
    screenshot: Option<Screenshot>,
    /// `MARINA_OPEN`: a dialog to open on the first frame, for screenshots.
    open_on_start: Option<String>,
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
            status_bar_visible: prefs.status_bar_visible,
            initialized: false,
            backend: Backend::new(backend::SAVE_DELAY),
            project: None,
            doc: None,
            last_scan: 0.0,
            title: String::new(),
            history: History::new(&diagram),
            diagram,
            canvas: Canvas::new(prefs.show_grid),
            tool: Tool::default(),
            sticky_tool: false,
            shift: false,
            overlay: None,
            palette: None,
            registry,
            toasts: Toasts::default(),
            screenshot: Screenshot::from_env(),
            open_on_start: std::env::var("MARINA_OPEN").ok(),
        };
        if let Some(folder) = prefs.last_project {
            app.open_project(folder, prefs.last_diagram);
        }
        app
    }

    /// Fonts and the theme, once per context.
    fn init(&mut self, ctx: &egui::Context) {
        if self.initialized {
            return;
        }
        self.initialized = true;
        self.backend.attach(ctx);
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

    #[cfg(test)]
    pub fn is_saving(&self) -> bool {
        self.backend.is_saving()
    }

    #[cfg(test)]
    pub fn flush(&self) {
        self.backend.flush();
    }

    #[cfg(test)]
    pub fn set_save_delay(&mut self, delay: Duration) {
        self.backend.set_save_delay(delay);
    }

    #[cfg(test)]
    pub fn window_title(&self) -> &str {
        &self.title
    }

    fn view(&self) -> View {
        View {
            modal_open: self.overlay.is_some() || self.palette.is_some(),
            project_open: self.project.is_some(),
            doc_open: self.doc.is_some(),
            has_selection: self.canvas.has_selection(),
            can_undo: self.history.can_undo(),
            can_redo: self.history.can_redo(),
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
            Act::ToggleSidebar => {
                self.sidebar_visible = !self.sidebar_visible;
                self.set_prefs(json!({ "sidebarVisible": self.sidebar_visible }));
            }
            Act::Tool(tool) => {
                self.tool = tool;
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
            Act::Nudge(dx, dy) => {
                let g = self.diagram.grid;
                self.canvas
                    .nudge(&mut self.diagram, f32::from(dx) * g, f32::from(dy) * g);
            }
            Act::Delete => {
                self.canvas.delete_selection(&mut self.diagram);
            }
            Act::Escape => self.canvas.escape(&mut self.diagram),
            Act::ZoomIn => self.canvas.zoom_step(1),
            Act::ZoomOut => self.canvas.zoom_step(-1),
            Act::ZoomReset => self.canvas.zoom_reset(),
            Act::ToggleGrid => self.set_show_grid(!self.canvas.show_grid),
            Act::ToggleStatusBar => {
                self.status_bar_visible = !self.status_bar_visible;
                self.set_prefs(json!({ "statusBarVisible": self.status_bar_visible }));
            }
            Act::Settings => self.open_settings(),
            Act::About => self.overlay = Some(Overlay::About),
            Act::Palette => self.palette = Some(CommandPalette::new()),
        }
    }

    fn set_show_grid(&mut self, on: bool) {
        self.canvas.show_grid = on;
        self.set_prefs(json!({ "showGrid": on }));
    }

    fn open_settings(&mut self) {
        let prefs = self.paths.prefs();
        self.overlay = Some(Overlay::Settings(SettingsState::new(
            "ui",
            prefs.custom_titlebar,
            self.custom_titlebar,
            self.canvas.show_grid,
            self.diagram.grid,
        )));
    }

    // --- The folder and its diagrams ------------------------------------------

    fn pick_folder(&mut self) {
        if let Some(folder) = rfd::FileDialog::new()
            .set_title("Open Folder")
            .pick_folder()
        {
            self.open_project(folder, None);
        }
    }

    /// Opens `folder` and the diagram `want` in it, else its first one.
    pub fn open_project(&mut self, folder: PathBuf, want: Option<String>) {
        let names = match Backend::list(&folder) {
            Ok(names) => names,
            Err(e) => {
                self.toasts
                    .error(format!("Couldn't open {}: {e}", folder.display()));
                return;
            }
        };
        self.close_doc();
        let pick = want
            .filter(|n| names.contains(n))
            .or_else(|| names.first().cloned());
        self.set_prefs(json!({ "lastProject": folder.to_string_lossy() }));
        self.project = Some(Project { folder, names });
        if let Some(name) = pick {
            self.open_diagram(&name);
        }
    }

    fn path(&self, name: &str) -> Option<PathBuf> {
        Some(backend::file(&self.project.as_ref()?.folder, name))
    }

    /// Writes the open diagram's waiting edits and closes it.
    fn close_doc(&mut self) {
        self.backend.flush();
        self.backend.close();
        self.doc = None;
        self.load(Diagram {
            grid: self.paths.prefs().grid_size,
            ..Diagram::new()
        });
    }

    /// Puts `d` on the canvas as a fresh start: no selection, no undo.
    fn load(&mut self, d: Diagram) {
        self.canvas.clear(true);
        self.history.reset(&d);
        self.diagram = d;
    }

    fn open_diagram(&mut self, name: &str) {
        let Some(path) = self.path(name) else {
            return;
        };
        let opened = Backend::read(&path).and_then(|text| {
            let d = blockliner_core::read(&text)?;
            Ok((text, d))
        });
        match opened {
            Ok((text, d)) => {
                self.close_doc();
                self.backend.watch(&path, &text);
                self.doc = Some(Doc {
                    name: name.to_owned(),
                    saved: d.clone(),
                });
                self.load(d);
                self.set_prefs(json!({ "lastDiagram": name }));
            }
            Err(e) => self.toasts.error(format!("Couldn't open “{name}”: {e:#}")),
        }
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

    /// A new, empty diagram called `name`, opened.
    fn create(&mut self, name: &str) -> anyhow::Result<()> {
        if self.taken(name, None) {
            anyhow::bail!(backend::TAKEN);
        }
        let path = self
            .path(name)
            .ok_or_else(|| anyhow::anyhow!("No folder is open."))?;
        let d = Diagram {
            grid: self.paths.prefs().grid_size,
            ..Diagram::new()
        };
        self.backend.create(&path, &blockliner_core::write(&d))?;
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
        if let Some(doc) = self.doc.as_mut().filter(|d| d.name == from) {
            doc.name = to.to_owned();
            self.set_prefs(json!({ "lastDiagram": to }));
        }
        Ok(())
    }

    /// Deletes the diagram `name`; if it was open, opens its neighbour.
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
        if self.doc.as_ref().is_some_and(|d| d.name == name) {
            self.close_doc();
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
    }

    /// Hands the diagram to the autosave when it changed. Only between
    /// gestures, so a drag is written once, when it ends.
    fn autosave(&mut self) {
        let (Some(project), Some(doc)) = (&self.project, &mut self.doc) else {
            return;
        };
        if self.diagram != doc.saved {
            doc.saved = self.diagram.clone();
            self.backend.save(
                backend::file(&project.folder, &doc.name),
                blockliner_core::write(&self.diagram),
            );
        }
    }

    /// About once a second: the folder's diagrams, and whether the open one
    /// changed on disk. An outside edit is reloaded (as an undoable step); a
    /// deleted file closes the diagram.
    fn watch_folder(&mut self, ctx: &egui::Context) {
        if self.project.is_none() {
            return;
        }
        let now = ctx.input(|i| i.time);
        ctx.request_repaint_after(Duration::from_secs_f64(SCAN_SECONDS));
        if now - self.last_scan < SCAN_SECONDS {
            return;
        }
        self.last_scan = now;
        self.relist();
        if self.canvas.is_busy() {
            return;
        }
        let Some(name) = self.doc.as_ref().map(|d| d.name.clone()) else {
            return;
        };
        match self.backend.check() {
            None => {}
            Some(Change::Removed) => {
                self.close_doc();
                self.toasts.info(format!(
                    "“{name}” was deleted or renamed outside BlockLiner."
                ));
            }
            Some(Change::Edited(text)) => match blockliner_core::read(&text) {
                Ok(d) => {
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
        self.watch_folder(&ctx);
        self.dispatch_keys(&ctx);

        if self.custom_titlebar {
            self.titlebar(ui);
        }
        self.main_view(ui);
        self.overlays(&ctx);
        self.palette(&ctx);
        self.toasts.show(&ctx);
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
            settings_open: matches!(self.overlay, Some(Overlay::Settings(_))),
            about_open: matches!(self.overlay, Some(Overlay::About)),
        };
        let acts = views::toolbar(ui, &props);
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
                zoom: self.canvas.view.zoom,
                scale: &self.theme.scale,
            };
            egui::Panel::bottom("blockliner-status")
                .exact_size(views::STATUS_BAR_HEIGHT)
                .resizable(false)
                .show_separator_line(false)
                .frame(Frame::NONE)
                .show(ui, |ui| views::status_bar(ui, &status));
        }
        let mut acts = acts;
        let mut side = Vec::new();
        if let (Some(project), true) = (&self.project, self.sidebar_visible) {
            let p = theme::palette(ui.ctx());
            let (min, default, max) = views::SIDEBAR_WIDTH;
            let folder = project
                .folder
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| project.folder.display().to_string());
            let open = self.doc.as_ref().map(|d| d.name.as_str());
            egui::Panel::left("blockliner-sidebar")
                .resizable(true)
                .default_size(default)
                .size_range(min..=max)
                .frame(Frame::NONE.fill(p.bg_surface))
                .show(ui, |ui| {
                    side = views::sidebar(ui, &folder, &project.names, open);
                });
        }
        let keys = !self.view().modal_open && !ui.ctx().egui_wants_keyboard_input();
        egui::CentralPanel::default()
            .frame(Frame::NONE)
            .show(ui, |ui| {
                if self.doc.is_none() {
                    acts.extend(views::empty_state(ui, self.project.is_some()));
                    return;
                }
                let out = self.canvas.show(ui, &mut self.diagram, self.tool, keys);
                if out.tool_done && !self.sticky_tool {
                    self.tool = Tool::Select;
                }
            });
        for act in acts {
            self.apply(act);
        }
        for action in side {
            match action {
                SidebarAction::Open(name) => self.open_diagram(&name),
                SidebarAction::New => self.apply(Act::NewDiagram),
                SidebarAction::Rename(name) => {
                    self.overlay = Some(Overlay::Name(NameState::rename(&name)));
                }
                SidebarAction::Delete(name) => self.overlay = Some(Overlay::ConfirmDelete(name)),
            }
        }
        // One undo step per finished change (see `History`), and the
        // autosave after it.
        if !self.canvas.is_busy() {
            self.history.observe(&self.diagram);
            self.autosave();
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
                    }
                }
            }
            Overlay::About => {
                let modal = AboutModal::new("BlockLiner", env!("CARGO_PKG_VERSION"))
                    .description(DESCRIPTION)
                    .repo(REPO_URL, Some("github.com/NathanLaan/marina"))
                    .icon(AboutIcon::Glyph(icons::OBJECT_GROUP));
                match modal.show(ctx) {
                    Some(AboutAction::Close) => keep = false,
                    Some(AboutAction::OpenUrl(url)) => ctx.open_url(egui::OpenUrl::new_tab(url)),
                    Some(_) | None => {}
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
