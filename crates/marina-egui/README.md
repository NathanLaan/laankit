# marina-egui

The native (egui) counterpart of `@marina/desktop-ui`, shared by the
NoteLiner, ThreadLiner and PageLiner native builds (plan Option C, see
[`docs/plans/plan-native-rust-egui.md`](../../docs/plans/plan-native-rust-egui.md)).
It runs on Linux, Windows and macOS.

## Usage

```rust
use marina_egui::{diagnostics, fonts, theme::ThemeState, titlebar, toast::Toasts, toolbar, window};

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

`examples/gallery.rs` shows every widget in every theme:

```bash
cargo run -p marina-egui --example gallery
```

## Modules

| Module        | What it does                                                                                   |
|---------------|------------------------------------------------------------------------------------------------|
| `theme`       | The six desktop-ui themes (generated from `theme/index.svelte.js`), UI scale steps, egui `Visuals` |
| `fonts`       | Bundled Inter / JetBrains Mono / Font Awesome; weights and italics; system-font fallback        |
| `icons`       | Font Awesome codepoints, drawn in the icon font family                                          |
| `window`      | Undecorated-window chrome: drag, double-click maximize, edge resize, caption buttons            |
| `titlebar`    | `TitleBar.svelte`: toolbar toggle, app actions, title, UI-scale dropdown, window controls       |
| `toolbar`     | `Toolbar`, `ToolbarButton`, `ToolbarDivider`, `ToolbarSpacer`                                   |
| `toast`       | Info, success and error notices at the bottom centre                                            |
| `diagnostics` | `MARINA_SCREENSHOT`, `MARINA_SCREENSHOT_EXIT`, `MARINA_THEME`, `MARINA_SCALE`                   |
| `dialog`      | Compact and full-size modals (`.modal-compact`, `.modal`), text inputs, buttons, `confirm`       |
| `commands`, `palette`, `settings`, `panes`, `about`, `not_ported` | Command registry and palette, settings shell, pane host, about dialog, "not in the native build yet" panel |

## Fonts

The Svelte builds use each platform's system UI font; the native builds
bundle one family so they look the same everywhere.

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

About 4.2 MB in all, embedded with `include_bytes!`. Pick a face with a
`fonts::Style` (`fonts::styled`, `fonts::bold`, `fonts::italic`,
`fonts::weight` for `RichText`; `fonts::format` for `LayoutJob`s).

Inter's `calt` feature is retagged `ss20` and JetBrains Mono NL is the
no-ligatures build, both for the same reason: egui can't switch OpenType
features off, and code should show `->` and `!=` as typed.

The faces are static instances rather than variable fonts on purpose: egui
shapes text about twice as fast with them, and first-time shaping is what
opening a long note or article costs (see `crates/marina-richtext/README.md`).

Anything these faces lack (CJK, emoji, Arabic, Devanagari, …) falls back to
the operating system's fonts through eframe's `system_fonts` feature
(fontique: fontconfig, DirectWrite, CoreText), loaded the first time a script
is drawn. Contexts eframe doesn't create (tests) call
`fonts::install_system_fallback`.

## Diagnostics

```bash
WAYLAND_DISPLAY= MARINA_THEME=light MARINA_SCALE=125 \
  MARINA_SCREENSHOT=/tmp/shot.png MARINA_SCREENSHOT_EXIT=1 \
  cargo run -p marina-egui --example gallery
```

On Wayland a locked session or hidden window gets no frames, so the capture
never fires; `WAYLAND_DISPLAY=` runs through XWayland. Headless CI uses
`egui_kittest` instead.

## Licences

Inter and JetBrains Mono are under the SIL Open Font License 1.1; Font
Awesome Free's font is under the OFL 1.1 and its icons under CC BY 4.0. The
licence files are in `assets/fonts/`. `window.rs` and `toast.rs` are adapted
from ArtCraft's `artcraft-native` (MIT OR Apache-2.0). See the repository's
`NOTICE`.
