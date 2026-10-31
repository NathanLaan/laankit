//! Every laankit-egui widget in every theme, inside a full app shell (title
//! bar, toolbar, pane sidebar, command palette, settings, About): the native
//! stand-in for the desktop-ui Playwright visual tests.
//!
//! ```bash
//! cargo run -p laankit-egui --example gallery
//! # A screenshot in one theme (see `laankit_egui::diagnostics`):
//! WAYLAND_DISPLAY= LAANKIT_THEME=lightPurple LAANKIT_SCREENSHOT=/tmp/gallery.png \
//!   LAANKIT_SCREENSHOT_EXIT=1 cargo run -p laankit-egui --example gallery
//! ```
//!
//! Ctrl+Shift+P opens the command palette and Ctrl+, the settings.
//! `LAANKIT_OPEN` opens a dialog at launch (`palette`, `settings`,
//! `shortcuts` or `about`), for screenshots.

use std::time::Instant;

use egui::{Frame, Key, Margin, Modifiers, RichText, ScrollArea, Ui};
use laankit_egui::about::{AboutAction, AboutIcon, AboutModal, UpdateStatus};
use laankit_egui::commands::{self, Binding, Command, Registry};
use laankit_egui::diagnostics::{self, Screenshot};
use laankit_egui::not_ported::NotPorted;
use laankit_egui::palette::{CommandPalette, ItemKind, PaletteItem, PaletteOutcome};
use laankit_egui::panes::{self, Pane, PaneLayout};
use laankit_egui::settings::{self, SettingsShell, Tab};
use laankit_egui::theme::{self, ThemeId, ThemeState};
use laankit_egui::titlebar::{self, TitleBar, TitleBarAction};
use laankit_egui::toast::Toasts;
use laankit_egui::toolbar::{self, ToolbarButton};
use laankit_egui::{fonts, icons, window};

const STORAGE_KEY: &str = "theme";
const PANES_KEY: &str = "panes";
const RECENTS_KEY: &str = "recents";

const NOTES: [&str; 5] = [
    "Meeting notes",
    "Reading list",
    "Rust 1.92 release",
    "Weekly review",
    "Zebra facts",
];

/// Everything a command, palette item or button can ask the gallery to do.
#[derive(Clone, Debug, PartialEq)]
enum Act {
    Palette,
    Settings,
    About,
    NextTheme,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    ToggleToolbar,
    ToggleSidebar,
    ShowTags,
    OpenNote(String),
}

/// What the commands' `when` predicates read.
struct View {
    tags_hidden: bool,
}

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let viewport = window::undecorated(
        egui::ViewportBuilder::default()
            .with_title("LaanKit gallery")
            .with_inner_size([1100.0, 760.0])
            .with_min_inner_size([640.0, 400.0]),
    );
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "laankit-gallery",
        options,
        Box::new(|cc| Ok(Box::new(Gallery::new(cc)))),
    )
}

struct Gallery {
    theme: ThemeState,
    registry: Registry<Act, View>,
    palette: Option<CommandPalette>,
    settings_tab: Option<String>,
    about: bool,
    update: UpdateStatus,
    panes: PaneLayout,
    tags_hidden: bool,
    spell_check: bool,
    restart_needed: bool,
    toolbar_visible: bool,
    sidebar: bool,
    toasts: Toasts,
    text: String,
    notes: String,
    checked: bool,
    radio: usize,
    choice: usize,
    slider: f32,
    selected_row: Option<usize>,
    screenshot: Option<Screenshot>,
    /// Logs the first frame's duration once.
    first_frame: Option<Instant>,
}

impl Gallery {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let start = Instant::now();
        fonts::install(&cc.egui_ctx);
        log::info!(
            "fonts::install took {:.1} ms",
            start.elapsed().as_secs_f64() * 1000.0
        );
        let mut theme: ThemeState = cc
            .storage
            .and_then(|s| eframe::get_value(s, STORAGE_KEY))
            .unwrap_or_default();
        diagnostics::theme_overrides(&mut theme);
        theme.apply(&cc.egui_ctx);
        let mut registry = registry();
        if let Some(recents) = cc.storage.and_then(|s| eframe::get_value(s, RECENTS_KEY)) {
            registry.load_recents(recents);
        }
        let mut gallery = Self {
            theme,
            registry,
            palette: None,
            settings_tab: None,
            about: false,
            update: UpdateStatus::Idle,
            panes: cc
                .storage
                .and_then(|s| eframe::get_value(s, PANES_KEY))
                .unwrap_or_default(),
            tags_hidden: false,
            spell_check: true,
            restart_needed: false,
            toolbar_visible: true,
            sidebar: true,
            toasts: Toasts::default(),
            text: "Single line".into(),
            notes: "Multi-line text.\nSecond line — with an em dash.".into(),
            checked: true,
            radio: 0,
            choice: 1,
            slider: 0.4,
            selected_row: Some(3),
            screenshot: Screenshot::from_env(),
            first_frame: Some(Instant::now()),
        };
        match std::env::var("LAANKIT_OPEN").as_deref() {
            Ok("palette") => {
                let mut p = CommandPalette::new();
                p.query = "re".into();
                gallery.palette = Some(p);
            }
            Ok("settings") => gallery.settings_tab = Some("ui".into()),
            Ok("shortcuts") => gallery.settings_tab = Some("keys".into()),
            Ok("about") => {
                gallery.about = true;
                gallery.update = UpdateStatus::Available {
                    version: "2.1.0".into(),
                    notes: Some("<p>Native builds.</p>".into()),
                };
            }
            _ => {}
        }
        gallery
    }

    fn view(&self) -> View {
        View {
            tags_hidden: self.tags_hidden,
        }
    }

    fn apply(&mut self, ctx: &egui::Context, act: Act) {
        match act {
            Act::Palette => self.palette = Some(CommandPalette::new()),
            Act::Settings => self.settings_tab = Some("ui".into()),
            Act::About => self.about = true,
            Act::NextTheme => {
                let i = ThemeId::ALL
                    .iter()
                    .position(|t| *t == self.theme.theme)
                    .unwrap_or(0);
                self.theme.theme = ThemeId::ALL[(i + 1) % ThemeId::ALL.len()];
            }
            Act::ZoomIn => self.theme.zoom_in(),
            Act::ZoomOut => self.theme.zoom_out(),
            Act::ZoomReset => self.theme.zoom_reset(),
            Act::ToggleToolbar => self.toolbar_visible = !self.toolbar_visible,
            Act::ToggleSidebar => self.sidebar = !self.sidebar,
            Act::ShowTags => self.tags_hidden = false,
            Act::OpenNote(name) => self.toasts.info(format!("Opened “{name}”")),
        }
        self.theme.apply(ctx);
    }

    fn set_theme(&mut self, ctx: &egui::Context, id: ThemeId) {
        self.theme.theme = id;
        self.theme.apply(ctx);
    }

    fn content(&mut self, ui: &mut Ui) {
        let p = theme::palette(ui.ctx());
        section(ui, "Theme", |ui| {
            ui.horizontal_wrapped(|ui| {
                for id in ThemeId::ALL {
                    if theme_chip(ui, id, self.theme.theme == id).clicked() {
                        self.set_theme(ui.ctx(), id);
                    }
                }
            });
        });

        section(ui, "Text", |ui| {
            ui.label("Body text in Inter, 14 px. The quick brown fox jumps over the lazy dog.");
            ui.horizontal_wrapped(|ui| {
                ui.label(fonts::weight("Semibold", fonts::SEMIBOLD));
                ui.label(fonts::bold("Bold"));
                ui.label(fonts::italic("Italic"));
                ui.label(fonts::styled("Bold italic", fonts::Style::BOLD_ITALIC));
                ui.label(RichText::new("Monospace").monospace());
                ui.label(fonts::styled(
                    RichText::new("Mono bold").monospace(),
                    fonts::Style {
                        mono: true,
                        ..fonts::Style::BOLD
                    },
                ));
                ui.label(RichText::new("Small text").small());
                ui.label(RichText::new("Muted").color(p.text_muted));
            });
            ui.label(RichText::new("Heading").heading());
            ui.label("CJK: 日本語のテキスト · 中文文本 · 한국어 텍스트");
            ui.label("Emoji: 😀 🎉 👍 ❤️ ★ ✓");
            ui.label("Scripts: مرحبا بالعالم · नमस्ते दुनिया · שלום · Ελληνικά · Кириллица");
            ui.label(format!(
                "Icons inline: {} Settings  {} Sync  {} Search",
                icons::GEAR,
                icons::ARROWS_ROTATE,
                icons::MAGNIFYING_GLASS
            ));
            Frame::NONE
                .fill(p.pre_bg)
                .stroke(egui::Stroke::new(1.0, p.border))
                .corner_radius(theme::RADIUS)
                .inner_margin(Margin::same(10))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new("fn main() {\n    println!(\"Hello, LaanKit\");\n}")
                            .font(fonts::mono(13.0))
                            .color(p.text_primary),
                    );
                });
        });

        section(ui, "Inputs", |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.text).desired_width(220.0));
                ui.add(
                    egui::TextEdit::singleline(&mut String::new())
                        .hint_text("Placeholder…")
                        .desired_width(180.0),
                );
            });
            ui.add(
                egui::TextEdit::multiline(&mut self.notes)
                    .desired_rows(3)
                    .desired_width(420.0),
            );
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.checked, "Checkbox");
                ui.radio_value(&mut self.radio, 0, "Radio A");
                ui.radio_value(&mut self.radio, 1, "Radio B");
                egui::ComboBox::from_id_salt("gallery-combo")
                    .selected_text(["Daily", "Weekly", "Monthly"][self.choice])
                    .show_ui(ui, |ui| {
                        for (i, label) in ["Daily", "Weekly", "Monthly"].into_iter().enumerate() {
                            ui.selectable_value(&mut self.choice, i, label);
                        }
                    });
            });
            ui.add(egui::Slider::new(&mut self.slider, 0.0..=1.0).text("Slider"));
            ui.horizontal(|ui| {
                let _ = ui.button("Button");
                primary_button(ui, "Primary");
                ui.add_enabled(false, egui::Button::new("Disabled"));
                ui.hyperlink_to("A link", "https://example.com");
                ui.add(egui::ProgressBar::new(self.slider).desired_width(140.0));
                ui.spinner();
            });
        });

        section(ui, "Toasts", |ui| {
            ui.horizontal(|ui| {
                if ui.button("Info").clicked() {
                    self.toasts.info("Settings saved to ~/.config/NoteLiner");
                }
                if ui.button("Success").clicked() {
                    self.toasts.success("Synced 3 changes");
                }
                if ui.button("Error").clicked() {
                    self.toasts
                        .error("Couldn't push: the remote has new commits");
                }
            });
        });

        section(ui, "List", |ui| {
            Frame::NONE
                .fill(p.bg_surface)
                .stroke(egui::Stroke::new(1.0, p.border))
                .corner_radius(theme::RADIUS)
                .show(ui, |ui| {
                    ScrollArea::vertical()
                        .id_salt("gallery-list")
                        .max_height(160.0)
                        .show_rows(ui, 22.0, 200, |ui, rows| {
                            ui.set_min_width(420.0);
                            for i in rows {
                                let selected = self.selected_row == Some(i);
                                if ui
                                    .selectable_label(selected, format!("Row {i}: a note title"))
                                    .clicked()
                                {
                                    self.selected_row = Some(i);
                                }
                            }
                        });
                });
        });

        section(ui, "Dialogs", |ui| {
            ui.horizontal(|ui| {
                if ui.button("Command palette (Ctrl+Shift+P)").clicked() {
                    self.palette = Some(CommandPalette::new());
                }
                if ui.button("Settings (Ctrl+,)").clicked() {
                    self.settings_tab = Some("ui".into());
                }
                if ui.button("About").clicked() {
                    self.about = true;
                }
            });
        });

        section(ui, "Not ported", |ui| {
            NotPorted::new("Slides")
                .note("Presenting arrives in Phase 4 of the native port.")
                .link(
                    "Open the release page",
                    "https://github.com/NathanLaan/laankit/releases",
                )
                .show(ui);
        });
    }

    /// The pane stack in the left sidebar (`PaneHost`).
    fn sidebar(&mut self, ui: &mut Ui) {
        let mut list = vec![Pane::new("files", "FILES").closable(false)];
        if !self.tags_hidden {
            list.push(Pane::new("tags", "TAGS"));
        }
        list.push(Pane::new("outline", "OUTLINE"));
        let out = panes::show(
            ui,
            egui::Id::unique("gallery-panes"),
            &mut self.panes,
            &list,
            |ui, id| {
                ui.add_space(6.0);
                let items: &[&str] = match id {
                    "files" => &NOTES,
                    "tags" => &["#work", "#reading", "#rust"],
                    _ => &["Heading 1", "Heading 2", "Heading 3"],
                };
                for item in items {
                    ui.horizontal(|ui| {
                        ui.add_space(12.0);
                        ui.label(*item);
                    });
                }
            },
            |_, _| {},
        );
        if out.closed.as_deref() == Some("tags") {
            self.tags_hidden = true;
            self.toasts
                .info("Tags pane closed. Reopen it from the command palette.");
        }
    }

    fn settings(&mut self, ctx: &egui::Context) {
        let Some(mut tab) = self.settings_tab.take() else {
            return;
        };
        let tabs = [
            Tab {
                id: "ui",
                label: "Interface",
            },
            Tab {
                id: "keys",
                label: "Shortcuts",
            },
        ];
        let shortcuts = self.registry.shortcut_list();
        let (theme, spell, restart) = (
            &mut self.theme,
            &mut self.spell_check,
            &mut self.restart_needed,
        );
        let close = SettingsShell::new(&tabs)
            .top_inset(theme::TITLEBAR_HEIGHT)
            .show(ctx, &mut tab, |ui, id| match id {
                "ui" => {
                    settings::setting_group(ui, "Theme", None, |ui| {
                        settings::theme_list(ui, theme)
                    });
                    settings::setting_group(
                        ui,
                        "UI scale",
                        Some("Scales the whole window, like Ctrl +/−."),
                        |ui| settings::scale_list(ui, theme),
                    );
                    settings::setting_group(ui, "Editor", None, |ui| {
                        if settings::toggle_option(ui, "Spell check", spell, true).changed() {
                            *restart = true;
                        }
                        let mut off = false;
                        settings::toggle_option(ui, "Vim keys (not yet)", &mut off, false);
                        if *restart && settings::restart_banner(ui, None) {
                            *restart = false;
                        }
                    });
                }
                _ => settings::shortcuts_list(ui, &shortcuts, None),
            });
        if !close {
            self.settings_tab = Some(tab);
        }
    }

    fn palette(&mut self, ctx: &egui::Context) {
        let Some(mut palette) = self.palette.take() else {
            return;
        };
        let view = self.view();
        let mut notes = |q: &str| -> Vec<PaletteItem<Act>> {
            if q.is_empty() {
                return Vec::new();
            }
            NOTES
                .iter()
                .filter_map(|n| {
                    let score = commands::fuzzy_score(n, q);
                    (score > 0).then(|| PaletteItem {
                        kind: ItemKind::Note,
                        id: format!("note:{n}"),
                        label: n.to_string(),
                        sub: format!("notes/{}.md", n.to_lowercase().replace(' ', "-")),
                        section: "Notes".into(),
                        score,
                        action: Act::OpenNote(n.to_string()),
                    })
                })
                .collect()
        };
        match palette.show(ctx, &mut self.registry, &view, &mut notes) {
            PaletteOutcome::Open => self.palette = Some(palette),
            PaletteOutcome::Closed => {}
            PaletteOutcome::Run(act) => self.apply(ctx, act),
        }
    }

    fn about(&mut self, ctx: &egui::Context) {
        if !self.about {
            return;
        }
        let modal = AboutModal::new("LaanKit gallery", env!("CARGO_PKG_VERSION"))
            .description("Every laankit-egui widget, in one window.")
            .repo(
                "https://github.com/NathanLaan/laankit",
                Some("github.com/NathanLaan/laankit"),
            )
            .icon(AboutIcon::Glyph(icons::LAYER_GROUP))
            .update(&self.update);
        match modal.show(ctx) {
            Some(AboutAction::Close) => self.about = false,
            Some(AboutAction::CheckForUpdates) => {
                self.update = UpdateStatus::Unavailable { dev: true };
            }
            Some(AboutAction::OpenUrl(url)) => ctx.open_url(egui::OpenUrl::new_tab(url)),
            Some(AboutAction::Download | AboutAction::Install) | None => {}
        }
    }

    fn modal_open(&self) -> bool {
        self.palette.is_some() || self.settings_tab.is_some() || self.about
    }
}

/// The gallery's commands, as an app would register them.
fn registry() -> Registry<Act, View> {
    let mut registry = Registry::new();
    let cmd = Modifiers::COMMAND;
    let shift = cmd | Modifiers::SHIFT;
    registry.register(
        Command::new("view.palette", "Command Palette", "View", Act::Palette)
            .key(Binding::physical(shift, Key::P)),
    );
    registry.register(
        Command::new("app.settings", "Settings", "App", Act::Settings)
            .key(Binding::logical(cmd, Key::Comma)),
    );
    registry.register(Command::new("app.about", "About", "App", Act::About));
    registry.register(
        Command::new("view.theme", "Next Theme", "View", Act::NextTheme)
            .key(Binding::physical(shift, Key::T)),
    );
    registry.register(
        Command::new("view.toolbar", "Toggle Toolbar", "View", Act::ToggleToolbar)
            .key(Binding::physical(shift, Key::E)),
    );
    registry.register(
        Command::new("view.sidebar", "Toggle Sidebar", "View", Act::ToggleSidebar)
            .key(Binding::logical(cmd, Key::B)),
    );
    registry.register(
        Command::new("view.zoomIn", "Zoom In", "View", Act::ZoomIn)
            .key(Binding::loose(cmd, Key::Plus)),
    );
    registry.register(
        Command::new("view.zoomOut", "Zoom Out", "View", Act::ZoomOut)
            .key(Binding::logical(cmd, Key::Minus)),
    );
    registry.register(
        Command::new("view.zoomReset", "Reset Zoom", "View", Act::ZoomReset)
            .key(Binding::logical(cmd, Key::Num0)),
    );
    registry.register(
        Command::new("view.tags", "Show Tags Pane", "View", Act::ShowTags)
            .when(|v: &View| v.tags_hidden),
    );
    registry
}

impl eframe::App for Gallery {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // Commands first, so their keys don't reach text fields.
        if !self.modal_open() {
            let view = self.view();
            for act in self.registry.dispatch_keys(&ctx, &view) {
                self.apply(&ctx, act);
            }
        }
        let bar = TitleBar {
            app_name: "LaanKit gallery",
            toolbar_visible: self.toolbar_visible,
        };
        let mut cycle_theme = false;
        let mut about = false;
        let actions = titlebar::show(ui, &bar, &mut self.theme, |ui| {
            cycle_theme = titlebar::button(ui, icons::SLIDERS, "Next theme", false).clicked();
            about = titlebar::button(ui, icons::CIRCLE_INFO, "About", false).clicked();
        });
        self.about |= about;
        for action in actions {
            match action {
                TitleBarAction::ToggleToolbar => self.toolbar_visible = !self.toolbar_visible,
                TitleBarAction::ScaleChanged(scale) => {
                    self.toasts.info(format!("UI scale {scale}%"))
                }
            }
        }
        if cycle_theme {
            let i = ThemeId::ALL
                .iter()
                .position(|t| *t == self.theme.theme)
                .unwrap_or(0);
            self.set_theme(&ctx, ThemeId::ALL[(i + 1) % ThemeId::ALL.len()]);
        }

        let (mut act, mut bottom_act) = (None, None);
        if self.toolbar_visible {
            toolbar::show(
                ui,
                |ui| {
                    toolbar::button(
                        ui,
                        ToolbarButton::new(icons::FILE_CIRCLE_PLUS, "New file (Ctrl+N)"),
                    );
                    if toolbar::button(
                        ui,
                        ToolbarButton::new(icons::FOLDER, "Sidebar").active(self.sidebar),
                    )
                    .clicked()
                    {
                        self.sidebar = !self.sidebar;
                    }
                    if toolbar::button(
                        ui,
                        ToolbarButton::new(icons::MAGNIFYING_GLASS, "Command palette"),
                    )
                    .clicked()
                    {
                        act = Some(Act::Palette);
                    }
                    toolbar::divider(ui);
                    toolbar::button(
                        ui,
                        ToolbarButton::new(icons::TRASH, "Delete").disabled(true),
                    );
                },
                |ui| {
                    toolbar::button(ui, ToolbarButton::new(icons::CLOUD_ARROW_UP, "Sync"));
                    if toolbar::button(ui, ToolbarButton::new(icons::GEAR, "Settings")).clicked() {
                        bottom_act = Some(Act::Settings);
                    }
                },
            );
        }
        if let Some(act) = act.or(bottom_act) {
            self.apply(&ctx, act);
        }

        let p = theme::palette(&ctx);
        if self.sidebar {
            egui::Panel::left("gallery-sidebar")
                .resizable(true)
                .default_size(240.0)
                .frame(Frame::NONE.fill(p.bg_surface))
                .show(ui, |ui| self.sidebar(ui));
        }
        egui::CentralPanel::default()
            .frame(Frame::NONE.fill(p.bg_base).inner_margin(Margin::same(16)))
            .show(ui, |ui| {
                ScrollArea::vertical()
                    .auto_shrink(false)
                    .show(ui, |ui| self.content(ui));
            });

        self.palette(&ctx);
        self.settings(&ctx);
        self.about(&ctx);
        self.toasts.show(&ctx);
        window::window_edges(&ctx);
        if let Some(start) = self.first_frame.take() {
            log::info!(
                "first frame after startup: {:.1} ms",
                start.elapsed().as_secs_f64() * 1000.0
            );
        }
        if let Some(shot) = &mut self.screenshot {
            shot.tick(&ctx);
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, STORAGE_KEY, &self.theme);
        eframe::set_value(storage, PANES_KEY, &self.panes);
        eframe::set_value(storage, RECENTS_KEY, &self.registry.recents());
    }
}

/// A titled group (the settings modal's `SettingGroup` look).
fn section(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui)) {
    let p = theme::palette(ui.ctx());
    ui.add_space(6.0);
    ui.label(fonts::bold(
        RichText::new(title.to_uppercase())
            .size(11.0)
            .color(p.text_muted),
    ));
    ui.add_space(4.0);
    add(ui);
    ui.add_space(10.0);
}

/// An accent-filled button.
fn primary_button(ui: &mut Ui, text: &str) -> egui::Response {
    let p = theme::palette(ui.ctx());
    ui.add(egui::Button::new(RichText::new(text).color(p.accent_on)).fill(p.accent))
}

/// A theme preview chip painted in that theme's own colours (`ThemeList`).
fn theme_chip(ui: &mut Ui, id: ThemeId, selected: bool) -> egui::Response {
    let t = id.palette();
    let stroke = if selected {
        egui::Stroke::new(2.0, t.accent)
    } else {
        egui::Stroke::new(1.0, t.border)
    };
    let resp = Frame::NONE
        .fill(t.bg_surface)
        .stroke(stroke)
        .corner_radius(theme::RADIUS)
        .inner_margin(Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (dot, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                ui.painter().circle_filled(dot.center(), 6.0, t.accent);
                ui.label(RichText::new(id.name()).color(t.text_primary));
            });
        })
        .response
        .interact(egui::Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, id.name()));
    resp
}
