# BlockLiner and NoteLiner: Diagram Toolbar Position Plan

Status: for review (2026-11-15). Adds an option that puts the diagram tools either in a row across the top of the canvas or in a column down its left side. It works the same in BlockLiner and in NoteLiner's diagram editor. In the row the stacking order has four buttons, and in the column it has the one Order button that opens the four moves.

## Summary

- The tools are the drawing tools (Select, the shapes, the lines, Text, and Note), the More Shapes button, and the stacking-order buttons. This plan calls them the tool strip.
- A new setting, Toolbar Position, has two values: Top and Left.
- Top: the tool strip is a row above the canvas, with four order buttons. This is NoteLiner's layout today.
- Left: the tool strip is a column at the canvas's left edge, with one Order button. This is close to BlockLiner's layout today, with one difference described under "BlockLiner".
- The strip is drawn by one shared function in `blockliner-egui`, so both apps get both layouts from the same code.
- Each app keeps its own default: Left in BlockLiner, Top in NoteLiner. Nothing changes for you until you change the setting, apart from where BlockLiner's tool column sits. See Q1.
- The work is in four phases: the shared strip, BlockLiner, NoteLiner, and tests and docs.

## Today

| Part | BlockLiner | NoteLiner |
|---|---|---|
| Where the tools are | In the app's toolbar at the far left of the window (`laankit_egui::toolbar::show`), above the app's own buttons (Open Folder, Close Folder, Properties, Export, Settings, About). The sidebar is between this toolbar and the canvas. | In a row above the canvas (`views/diagram.rs`), inside the document area. The app's own toolbar at the far left holds only app buttons. |
| What else is in that bar | The six app buttons, at the bottom | The file's name, Undo and Redo, the grid and dimension toggles, the zoom, and the Properties, Export, and History toggles |
| Stacking order | One Order button that opens a list (`tools::order_menu_button`) | Four 28 px buttons drawn by NoteLiner (`order_button` in `views/diagram.rs`) |
| The tool buttons | `tools::tool_buttons`, shared, 36 px | The same |
| When there isn't room | The window's minimum height is 712 px so the column fits | The row scrolls sideways |
| A setting for the position | None | None |

## Proposal

### The setting

| App | Where | Stored as | Default |
|---|---|---|---|
| BlockLiner | Settings → UI, in the Canvas group: Toolbar Position, a list with Top and Left | `toolbarPosition` in `ui-preferences.json`: `top` or `left` | `left` |
| NoteLiner | Settings → Diagrams: Toolbar Position, the same list | `diagramToolbarPosition` in the prefs: `top` or `left` | `top` |

- A command, Toggle Toolbar Position, is in the palette in both apps (the View section in BlockLiner, the Diagram section in NoteLiner). It has no key. See Q5.
- The change applies at once, with no restart.
- An unknown value in the file reads as the app's default.

### The tool strip

One function draws the strip in either direction:

| | Top (a row) | Left (a column) |
|---|---|---|
| Tools | Select, a divider, then the rest, left to right | The same, top to bottom |
| Stacking order | Four buttons after a divider: Move to Front, Move Up, Move Down, Move to Back | One Order button after a divider, which opens the four as a list with their icons and keys |
| More Shapes and Order lists open | Below the button | To the right of the button |
| When there isn't room | The row scrolls sideways | The column scrolls up and down |
| Size | 44 px high | 48 px wide |

- Every button is the shared 36 px toolbar button, so the tools and the order buttons match. NoteLiner's own 28 px order buttons are removed.
- A greyed order button, a tooltip with the name and key, and the number in a tool button's corner are as they are today.
- The four moves stay in the right-click menu, on their keys, and in the palette in both layouts.

### BlockLiner

| Position | Layout |
|---|---|
| Left | The tool strip is a column at the canvas's left edge, below the tab strip and to the right of the sidebar. The app's toolbar at the far left of the window keeps only the six app buttons. |
| Top | The tool strip is a row between the tab strip and the canvas. The app's toolbar is the same six buttons. |

This moves BlockLiner's tools even when the setting is Left: today they are in the app's toolbar at the window's edge, and they would be at the canvas's edge, on the other side of the sidebar. I read "it would use the left side of the canvas" as asking for this, and it is what lets both apps share one strip. With the sidebar hidden the two places are next to each other. See Q1 for keeping the tools in the app's toolbar.

A side effect is that the app's toolbar becomes short again, so the window's minimum height can go back from 712 px to 640 px. P2 measures this.

### NoteLiner

| Position | Layout |
|---|---|
| Top | As today, with the four order buttons now 36 px like the tools. |
| Left | The tool strip is a column at the canvas's left edge. The bar above the canvas stays, without the tools: the file's name, Undo and Redo, the grid and dimension toggles, the zoom, and the Properties, Export, and History toggles. |

The bar above the canvas stays in the Left layout because a column holding everything would be about 20 buttons tall, more than a small window has. See Q2.

The setting applies to diagrams only. The note editor's toolbar doesn't move. See Q4.

## Changes

### blockliner-egui

| File | Change |
|---|---|
| `tools.rs` | `enum ToolbarSide { Top, Left }` with `id()`, `parse()`, and `name()`. `tool_strip(ui, side, tool, more_shape, orderable) -> Option<EditAct>`, which draws the tools and the order buttons in a row or a column, inside a scroll area. `order_button` (one 36 px button for a move). `more_shapes_button` and `order_menu_button` take the side, for where their lists open. `STRIP_HEIGHT` and `STRIP_WIDTH`. |
| `lib.rs` | `ToolbarSide` in the re-exports. |

### BlockLiner

| File | Change |
|---|---|
| `config.rs` | `toolbar_position` in `Prefs`, read from and written to `toolbarPosition`. |
| `overlays/settings.rs` | The Toolbar Position list in the Canvas group, and `SettingsAction::SetToolbarPosition`. |
| `commands.rs` | `Act::ToggleToolbarPosition` and its command. |
| `views.rs` | The app toolbar loses the tools and the Order button, and `ToolbarProps` loses `tool`, `more_shape`, and `orderable`. |
| `app.rs` | The strip as a top or left panel inside the document area, between the tab strip and the canvas. The setting's state and the toggle. |
| `main.rs` | The minimum window height, after measuring. |
| `ui_tests.rs` | `at(x, y)` reads the canvas's origin from the app, since the canvas moves with the setting. |

### NoteLiner

| File | Change |
|---|---|
| `config.rs` | `diagram_toolbar_position` in `Prefs`, from `diagramToolbarPosition`. |
| `overlays/settings.rs` | The Toolbar Position list in the Diagrams tab. |
| `commands.rs` | `Act::ToggleDiagramToolbarPosition` and its command, on when a diagram is the active tab. |
| `views/diagram.rs` | The top bar takes the side: with Top it holds the strip as today, and with Left it leaves the strip out. The local `order_button` and `tools` are removed, and `BUTTONS_WIDTH` is worked out from the side. |
| `app.rs` | With Left, a left panel for the strip beside the canvas, under the top bar. |
| `diagram.rs` | `DiagramDoc::set_prefs` is unchanged. The side is the app's state, not the editor's. |

### Docs

`apps/blockliner/README.md` and `apps/noteliner/README.md` (the setting), the two native READMEs (the layout, the settings table, the modules, and the tests), and the version in each app's `Cargo.toml` and metainfo file.

## Phases

| Phase | Work | Done when |
|---|---|---|
| P1 | `ToolbarSide`, `tool_strip`, `order_button`, and the lists' direction in `blockliner-egui` | The crate's tests pass, and the strip draws in both directions in a small test harness |
| P2 | BlockLiner: the pref, the setting, the command, the two layouts, the shorter app toolbar, and the minimum height | Both layouts work, the setting survives a restart, and a screenshot of each at the minimum window size shows nothing cut off |
| P3 | NoteLiner: the pref, the setting, the command, and the two layouts | Both layouts work in a diagram tab, and a note tab is unchanged |
| P4 | UI tests and docs | The suites pass, and the READMEs describe the setting |

## Tests

| Layer | Covers |
|---|---|
| Shared (`blockliner-egui`) | `ToolbarSide` reads and writes its ids, and an unknown id is `None`. |
| UI, BlockLiner (`egui_kittest`) | With Left: one Order button, and its list moves a shape. With Top: four order buttons, and no Order button. The setting in Settings and the palette command switch the layout, and `toolbarPosition` is saved. A tool picked in either layout draws at the pointer, which checks that the canvas's origin follows the layout. The app's toolbar has its six buttons and no tools. |
| UI, NoteLiner | The same two layouts in a diagram tab. With Left the top bar still has Undo, the grid toggle, and Export. The setting is saved as `diagramToolbarPosition`. A note tab's toolbar is the same in both. |
| Existing tests | BlockLiner's tests that click the canvas use `at(x, y)` (138 uses). They pass unchanged once the helper reads the origin. |
| By hand | Each layout in each app at the minimum window size, with the sidebar shown and hidden. The More Shapes and Order lists open on screen in both directions. The light theme. |

## Risks

| Risk | Handling |
|---|---|
| BlockLiner's tools move even on the default | Q1. The alternative keeps them where they are. |
| BlockLiner's canvas tests assume where the canvas starts | One helper is changed to read the origin. P2 runs the whole suite before anything else is built on it. |
| The row in BlockLiner is wider than a narrow canvas | The row scrolls sideways, as NoteLiner's does. |
| A list opens off the edge of the window | The lists open below in a row and to the right in a column, and P4 checks both by hand at the minimum size. |
| Two bars in NoteLiner's Left layout take room from the canvas | The column is 48 px. Q2 covers folding the top bar away. |

## Out of scope

- Right and bottom positions.
- Dragging the toolbar to a new place.
- Moving the note editor's toolbar in NoteLiner.
- Moving either app's own toolbar (Open Folder, Settings, and the rest).
- Hiding the toolbar.

## Open questions

| # | Question | Recommendation |
|---|---|---|
| Q1 | In BlockLiner with Left, should the tools sit at the canvas's left edge (right of the sidebar), or stay in the app's toolbar at the window's edge as they are today? | The canvas's edge, as you described. It is one strip for both apps and both positions, and the app's toolbar gets short enough to lower the minimum window height. Staying put means BlockLiner's Left layout is its own code. |
| Q2 | In NoteLiner with Left, should the bar above the canvas stay for Undo, Redo, the grid, the zoom, and the Properties, Export, and History toggles? | Yes. Everything in one column is about 20 buttons, which doesn't fit a small window. |
| Q3 | Are the defaults right: Left in BlockLiner and Top in NoteLiner, as each is today? | Yes. |
| Q4 | Should the setting also move the note editor's toolbar in NoteLiner? | No. It has four buttons and belongs above the text. |
| Q5 | Should Toggle Toolbar Position have a key? | No. It is a setting you change rarely. |
| Q6 | Should BlockLiner's Top layout also show Undo and Redo buttons, as NoteLiner's bar does? BlockLiner has none today. | Not in this plan. |
| Q7 | What versions? Both apps are 2.9.0. | 2.10.0 for both. |

## Answers

Q1: 
Q2:
Q3:
Q4:
Q5:
Q6:
Q7:
