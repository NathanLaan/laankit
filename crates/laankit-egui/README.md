# laankit-egui

The native (egui) counterpart of `@laankit/desktop-ui`, shared by the NoteLiner, ThreadLiner and PageLiner native builds. It runs on Linux, Windows and macOS.

## Usage

```rust
use laankit_egui::{diagnostics, fonts, theme::ThemeState, titlebar, toast::Toasts, toolbar, window};

fn main() -> eframe::Result {
    let viewport = window::undecorated(egui::ViewportBuilder::default().with_title("NoteLiner"));
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native("NoteLiner", options, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        fonts::install(&cc.egui_ctx);
        let mut theme: ThemeState = cc.storage.and_then(|s| eframe::get_value(s, "theme")).unwrap_or_default();
        diagnostics::theme_overrides(&mut theme);
        theme.apply(&cc.egui_ctx);
        // …
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let bar = titlebar::TitleBar { app_name: "NoteLiner", toolbar_visible: self.toolbar };
        for action in titlebar::show(ui, &bar, &mut self.theme, |ui| { /* app buttons */ }) {
            // ToggleToolbar, ScaleChanged(id): persist the theme state
        }
        toolbar::show(ui, |ui| { /* top buttons */ }, |ui| { /* bottom buttons */ });
        egui::CentralPanel::default().show(ui, |ui| { /* content */ });
        self.toasts.show(ui.ctx());
        window::window_edges(ui.ctx());
    }
}
```

`examples/gallery.rs` shows every widget in every mode, theme, and colour:

```bash
cargo run -p laankit-egui --example gallery
```

## Modules

| Module        | What it does                                                                                   |
|---------------|------------------------------------------------------------------------------------------------|
| `theme`       | Mode, theme, and colour (see Themes below), UI scale steps, egui `Visuals`                      |
| `fonts`       | Bundled Inter / JetBrains Mono / Font Awesome; weights and italics; system-font fallback        |
| `icons`       | Font Awesome codepoints, drawn in the icon font family                                          |
| `window`      | Undecorated-window chrome: drag, double-click maximize, edge resize, caption buttons            |
| `titlebar`    | `TitleBar.svelte`: toolbar toggle, app actions, title, UI-scale dropdown, window controls       |
| `toolbar`     | `Toolbar`, `ToolbarButton`, `ToolbarDivider`, `ToolbarSpacer`                                   |
| `toast`       | Info, success and error notices at the bottom centre                                            |
| `diagnostics` | `LAANKIT_SCREENSHOT`, `LAANKIT_SCREENSHOT_EXIT`, `LAANKIT_MODE`, `LAANKIT_STYLE`, `LAANKIT_COLOUR`, `LAANKIT_THEME`, `LAANKIT_SCALE` |
| `dialog`      | Compact and full-size modals (`.modal-compact`, `.modal`), text inputs, buttons, `confirm`       |
| `file_dialog` | `FileDialogs`: runs an `rfd` dialog on its own thread on Linux, so the window keeps answering the compositor (no "not responding" prompt) |
| `commands`, `palette`, `settings`, `panes`, `tabs`, `about`, `not_ported` | Command registry and palette, settings shell, pane host, document tabs, about dialog, "not in the native build yet" panel |

## Themes

What the apps look like is three settings, chosen separately in Settings and held in `ThemeState`.

| Setting | Type | Choices | Decides |
|---|---|---|---|
| Mode | `Mode` | Light, Dark, System | Whether the light or the dark set of the colour is used. System follows the computer, and is Dark where the computer reports nothing. |
| Theme | `Look` | Classic, Modern | The `Shape`: corners, row and header heights, and how the title bar, toolbar, pane headers, tabs, selections, and buttons are drawn. |
| Colour | `Colour` | Blue, Purple, Midnight, Mulberry, Rasberryson, Neutral | A light and a dark `Palette`. |

`ThemeState::apply` hands the palette and the shape to the widgets, which read them with `theme::palette(ctx)` and `theme::shape(ctx)`. An app calls `ThemeState::follow_system(ctx)` each frame so that System mode follows the computer.

`settings::appearance(ui, state, colours, looks)` draws the three groups. An app passes the colours it offers, and passes `looks: true` only once its own screens are drawn from the shape. No app does yet, so the apps are Classic and only the gallery offers Modern.

The seven `ThemeId` palettes (Midnight, Dark, Dark-Purple, Light, Light-Purple, Light-Mulberry, and Light-Rasberryson) are the sets of the first five colours that existed as themes, and are still NoteLiner's slide themes. A state is saved as `{ theme, mode, style, colour, scale }`, where `theme` is the nearest of the seven. A state that has only `theme` opens as that theme's mode and colour in Classic, with the same palette.

## Fonts

The Svelte builds use each platform's system UI font; the native builds bundle one family so they look the same everywhere.

| Family                     | Face                                   | Size    |
|----------------------------|----------------------------------------|---------|
| `Proportional`             | Inter 4.1 Regular                      | 402 KB  |
| `fonts::SEMIBOLD_FAMILY`   | Inter 4.1 SemiBold                     | 410 KB  |
| `fonts::BOLD_FAMILY`       | Inter 4.1 Bold                         | 411 KB  |
| `fonts::ITALIC`            | Inter 4.1 Italic                       | 408 KB  |
| `fonts::BOLD_ITALIC`       | Inter 4.1 Bold Italic                  | 415 KB  |
| `Monospace`                | JetBrains Mono NL 2.304 Regular        | 203 KB  |
| `fonts::MONO_BOLD`         | JetBrains Mono NL 2.304 Bold           | 206 KB  |
| `fonts::MONO_ITALIC`       | JetBrains Mono NL 2.304 Italic         | 206 KB  |
| `fonts::SERIF` (+ `_ITALIC`, `_BOLD`, `_BOLD_ITALIC`) | Source Serif 4.005 Regular, Italic, Bold, Bold Italic (books) | 893 KB |
| `icons::FAMILY`            | Font Awesome Free 6.7.2 Solid          | 416 KB  |

About 4.2 MB in all, embedded with `include_bytes!`. Pick a face with a `fonts::Style` (`fonts::styled`, `fonts::bold`, `fonts::italic`, `fonts::weight` for `RichText`; `fonts::format` for `LayoutJob`s).

Inter's `calt` feature is retagged `ss20` and JetBrains Mono NL is the no-ligatures build, both for the same reason: egui can't switch OpenType features off, and code should show `->` and `!=` as typed.

The faces are static instances rather than variable fonts on purpose: egui shapes text about twice as fast with them, and first-time shaping is what opening a long note or article costs (see `crates/laankit-richtext/README.md`).

Anything these faces lack (CJK, emoji, Arabic, Devanagari, …) falls back to the operating system's fonts through eframe's `system_fonts` feature (fontique: fontconfig, DirectWrite, CoreText), loaded the first time a script is drawn. Contexts eframe doesn't create (tests) call `fonts::install_system_fallback`.

## Diagnostics

```bash
WAYLAND_DISPLAY= LAANKIT_MODE=light LAANKIT_STYLE=modern LAANKIT_COLOUR=purple LAANKIT_SCALE=125 \
  LAANKIT_SCREENSHOT=/tmp/shot.png LAANKIT_SCREENSHOT_EXIT=1 \
  cargo run -p laankit-egui --example gallery
```

On Wayland a locked session or hidden window gets no frames, so the capture never fires; `WAYLAND_DISPLAY=` runs through XWayland. Headless CI uses `egui_kittest` instead.

## Licences

Inter and JetBrains Mono are under the SIL Open Font License 1.1; Font Awesome Free's font is under the OFL 1.1 and its icons under CC BY 4.0. The licence files are in `assets/fonts/`. `window.rs` and `toast.rs` are adapted from ArtCraft's `artcraft-native` (MIT OR Apache-2.0). See the repository's `NOTICE`.
