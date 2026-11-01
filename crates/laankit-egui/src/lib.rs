//! The native (egui) counterpart of `@laankit/desktop-ui`, shared by the
//! LaanKit apps.
//!
//! | Module         | desktop-ui equivalent                                                  |
//! |----------------|------------------------------------------------------------------------|
//! | `theme`        | `theme/index.svelte.js`: the six themes and the UI scale               |
//! | `fonts`        | the system font stack, plus bundled faces and glyph fallback           |
//! | `window`       | window controls and `-webkit-app-region: drag`, for undecorated windows |
//! | `titlebar`     | `TitleBar.svelte`                                                      |
//! | `toolbar`      | `Toolbar`, `ToolbarButton`, `ToolbarDivider`, `ToolbarSpacer`          |
//! | `icons`        | Font Awesome icons                                                     |
//! | `toast`        | transient notices                                                      |
//! | `commands`     | `command-palette/registry.svelte.js` and `lib/fuzzy.js`                |
//! | `palette`      | `CommandPalette.svelte`                                                |
//! | `settings`     | `settings/*`: `SettingsShell`, `SettingGroup`, `ThemeList`, …          |
//! | `panes`        | `PaneHost.svelte`                                                      |
//! | `about`        | `AboutModal.svelte`                                                    |
//! | `dialog`       | compact and full modals, form inputs, buttons, confirmations           |
//! | `file_dialog`  | system file dialogs that don't freeze the window (`FileDialogs`)       |
//! | `not_ported`   | the "not in the native build yet" panel                                |
//! | `diagnostics`  | `LAANKIT_SCREENSHOT` and friends                                        |

pub mod about;
pub mod commands;
pub mod diagnostics;
pub mod dialog;
pub mod file_dialog;
pub mod fonts;
pub mod icons;
pub mod not_ported;
pub mod palette;
pub mod panes;
pub mod settings;
pub mod theme;
pub mod titlebar;
pub mod toast;
pub mod toolbar;
pub mod window;
