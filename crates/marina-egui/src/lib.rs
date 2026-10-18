//! The native (egui) counterpart of `@marina/desktop-ui`, shared by the
//! NoteLiner, ThreadLiner and PageLiner native builds (plan Option C, see
//! `docs/plans/plan-native-rust-egui.md`).
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
//! | `not_ported`   | the "not in the native build yet" panel                                |
//! | `diagnostics`  | `MARINA_SCREENSHOT` and friends                                        |

pub mod about;
pub mod commands;
pub mod diagnostics;
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
