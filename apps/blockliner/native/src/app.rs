//! The app: owns the open diagram, the canvas, the current tool and the open
//! dialog; routes commands (keyboard, palette, toolbar) and dialog actions.

use blockliner_core::Diagram;
use egui::{Frame, Ui};
use marina_egui::about::{AboutAction, AboutIcon, AboutModal};
use marina_egui::commands::{Recents, Registry};
use marina_egui::diagnostics::{self, Screenshot};
use marina_egui::palette::{CommandPalette, PaletteOutcome};
use marina_egui::theme::{self, ThemeState};
use marina_egui::titlebar::{self, TitleBar, TitleBarAction};
use marina_egui::toast::Toasts;
use marina_egui::{fonts, icons, window};
use serde_json::{json, Value};

use crate::canvas::{Canvas, Tool};
use crate::commands::{self, Act, View};
use crate::config::Paths;
use crate::overlays::settings::{SettingsAction, SettingsState};
use crate::overlays::{self, Overlay};
use crate::views::{self, StatusProps, ToolbarProps};

/// eframe storage keys.
const THEME_KEY: &str = "blockliner-theme";
const RECENTS_KEY: &str = "blockliner-recents";
const DESCRIPTION: &str =
    "A diagram editor for software architecture, saving Mermaid flowcharts. Built on the marina framework.";
const REPO_URL: &str = "https://github.com/NathanLaan/marina";

pub struct BlockLinerApp {
    paths: Paths,
    theme: ThemeState,
    /// Whether the window was built undecorated, with Marina's title bar.
    custom_titlebar: bool,
    status_bar_visible: bool,
    initialized: bool,
    diagram: Diagram,
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
        Self {
            paths,
            theme,
            custom_titlebar: prefs.custom_titlebar,
            status_bar_visible: prefs.status_bar_visible,
            initialized: false,
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
        }
    }

    /// Fonts and the theme, once per context.
    fn init(&mut self, ctx: &egui::Context) {
        if self.initialized {
            return;
        }
        self.initialized = true;
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

    fn view(&self) -> View {
        View {
            modal_open: self.overlay.is_some() || self.palette.is_some(),
            has_selection: !self.canvas.selection.is_empty(),
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
            Act::Tool(tool) => {
                self.tool = tool;
                self.sticky_tool = self.shift;
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
    }

    /// The whole UI, once per frame.
    pub fn show(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.init(&ctx);
        self.shift = ctx.input(|i| i.modifiers.shift);
        self.dispatch_keys(&ctx);

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
        if let Some(shot) = &mut self.screenshot {
            shot.tick(&ctx);
        }
    }

    fn titlebar(&mut self, ui: &mut Ui) {
        let bar = TitleBar {
            app_name: "BlockLiner",
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
                // There's no sidebar to toggle yet (plan phase 5).
                TitleBarAction::ToggleToolbar => {}
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
            let status = StatusProps {
                tool: self.tool,
                pointer: self.canvas.pointer,
                selected: self.canvas.selection.len(),
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
        let keys = !self.view().modal_open && !ui.ctx().egui_wants_keyboard_input();
        egui::CentralPanel::default()
            .frame(Frame::NONE)
            .show(ui, |ui| {
                let out = self.canvas.show(ui, &mut self.diagram, self.tool, keys);
                if out.tool_done && !self.sticky_tool {
                    self.tool = Tool::Select;
                }
            });
        for act in acts {
            self.apply(act);
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
