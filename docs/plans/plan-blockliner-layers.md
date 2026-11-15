# BlockLiner: Layers (Stacking Order) Plan

Status: for review (2026-11-15). Adds four actions that change which shape is drawn in front of another: Move Up, Move Down, Move to Front, and Move to Back. They work the same in BlockLiner and in NoteLiner's diagram editor, with toolbar buttons, right-click menu items, keys, and command palette entries.

## Summary

- "Layers" here means the stacking order of shapes: which one covers which where they overlap. It does not mean named layers you can show, hide, or lock. See Q7.
- A diagram already has a stacking order. Shapes are drawn in the order they were made, and the file already keeps that order. What is missing is a way to change it.
- The four actions reorder the selected shapes. Nothing changes in the file format.
- Lines stay in front of every shape, as they are today. The actions apply to shapes only. See Q2.
- Nearly all of the work is in the two shared crates, so it is written once. Each app adds its toolbar buttons and one line in its command mapping. See "Is the code modular?".
- The work is in four phases and is small: the model, the commands and menu, the toolbars, and tests and docs.

## Is the code modular?

Yes. The diagram editor is written once, in two shared crates, and both apps use it. Neither app has a copy of the editor.

| Layer | Crate | Holds | Used by |
|---|---|---|---|
| Model | `blockliner-core` | The diagram, the Mermaid reader and writer, geometry, hit-testing, routes, undo history, clipboard, and export scope | Both apps, and `noteliner-core` |
| Editor | `blockliner-egui` | `DiagramEditor`, the canvas and its gestures, painting, the right-click menu, the edit commands with their keys (`EditAct`, `commands()`), the tool buttons, the Properties pane, the Custom colour and Export dialogs, and the SVG and PNG writers | Both apps |
| App | `apps/blockliner/native`, `apps/noteliner/native` | Opening and saving files, autosave, tabs, settings, the toolbar's layout, and which overlay hosts a dialog | One app each |

What each app has of its own is the part that differs between them: BlockLiner opens a folder of `.mmd` files and has a toolbar down the left side, and NoteLiner opens a diagram as a tab in a notes project (`diagram.rs`, 279 lines) and has a toolbar across the top (`views/diagram.rs`, 284 lines).

There is a small amount of code written twice. None of it is a problem for this feature, and the first two are worth folding into the shared crate while the toolbars are being changed:

| Written twice | Where | Size | Proposal |
|---|---|---|---|
| The loop that draws the tool buttons and the More Shapes button | `apps/blockliner/native/src/views.rs:49`, `apps/noteliner/native/src/views/diagram.rs:175` | About 15 lines each, the same but for the action type and the divider | Move to `blockliner_egui::tools::tool_buttons(ui, tool, more_shape, divider) -> Option<Tool>` in P3 |
| The mapping from the shared `When` to the app's own predicate | `apps/blockliner/native/src/commands.rs:151`, `apps/noteliner/native/src/commands.rs:182` | One `match` each, with a small function per arm | Each app's view holds an `EditView`, and one generic function calls `When::holds`. See Q6. |
| The export scope (everything, or the selection) | `apps/blockliner/native/src/app.rs:1193`, `apps/noteliner/native/src/app.rs:1268` | About 12 lines each | Out of scope here. A `DiagramEditor::export_scope(selected_only)` would remove both. |

What this feature costs in each place:

| Place | Work for layers |
|---|---|
| `blockliner-core` | The reorder itself, and its tests |
| `blockliner-egui` | The four commands, their keys, the menu items, the icons, and one shared function that draws the four buttons |
| BlockLiner | One call to draw the buttons, and one arm in the `When` mapping (none if Q6 is yes) |
| NoteLiner | The same |

## Today

| Part | Where | Does |
|---|---|---|
| Stacking order | `Diagram.shapes` (`blockliner-core/src/model.rs`) | The list's order is the drawing order. A new or pasted shape goes on the end, so it is in front. |
| Drawing | `paint::diagram` (`blockliner-egui/src/canvas/paint.rs:142`) | Draws every shape in list order, then every line, so lines are always in front of shapes. |
| Clicking | `Diagram::shape_at` and the line hit-tests (`geometry.rs:537`, `connect.rs:225`) | Search from the end of the list, so a click takes the shape in front. |
| Saving | `mermaid::write` | Writes the node lines and the `%% @shape` lines in list order. |
| Reading | `mermaid::read`, `build` (`read.rs:362`) | Sorts the shapes by the order of their `@shape` lines. A saved order comes back as it was, for shapes in kept statements too. |
| Export | `blockliner-egui/src/export.rs` | Writes shapes in list order, so an SVG or PNG stacks as the canvas does. |
| Undo | `History::observe` | Records a step whenever the diagram differs, and a different order is a difference. |
| Autosave in NoteLiner | `DiagramDoc::due` | Saves when the diagram differs from the last save. |
| Selection | `Canvas.selection` (shape ids) and `Canvas.lines` (line indexes) | Shapes are selected by id, so reordering them keeps the selection. Lines are selected by index, so reordering lines would not. |
| Changing the order | Nowhere | The only way to bring a shape forward is to cut and paste it, which gives it a new id when the old one is taken and drops lines glued to shapes that weren't copied. |

## Proposal

### The four actions

| Action | Does | Key |
|---|---|---|
| Move to Front | The selected shapes go in front of every other shape | Ctrl+Shift+] |
| Move Up | The selected shapes go in front of the nearest shape that covers one of them | Ctrl+] |
| Move Down | The selected shapes go behind the nearest shape that one of them covers | Ctrl+[ |
| Move to Back | The selected shapes go behind every other shape | Ctrl+Shift+[ |

- Selected shapes keep their order among themselves.
- Selected lines are ignored. With only lines selected, the four actions are off.
- Each action is one undo step.
- A label being edited is committed first, as the other actions do.
- Shapes in kept statements (the ones with a lock badge) can be reordered, as they can be moved. Their order is in the `@shape` lines, not in their declaration.

### What Move Up and Move Down step over

A diagram can have dozens of shapes, and most of them don't touch. Stepping one place in the whole list would often change nothing you can see, and you would press the key many times before the shape came forward.

So Move Up steps over the nearest shape in front that overlaps a selected shape (their rectangles intersect), and Move Down is the mirror. The selected shapes land directly in front of, or behind, that shape.

- Move Up is off when no shape in front overlaps the selection. Move to Front is still on, unless the selection is already in front of everything.
- With several shapes selected, the ones behind the stepped-over shape move to just in front of it, and the ones already in front of it stay where they are.

See Q3 for the plain one-place alternative.

### When each action is on

| Action | On when |
|---|---|
| Move to Front | A shape is selected, and an unselected shape is in front of one of the selected shapes |
| Move Up | A shape is selected, and an unselected shape in front overlaps one of the selected shapes behind it |
| Move Down | The mirror of Move Up |
| Move to Back | The mirror of Move to Front |

An action that is off is greyed in the toolbar and the menu, and is left out of the command palette, as the other edit commands are.

### Toolbar buttons

The four buttons are drawn by one shared function, `blockliner_egui::tools::order_buttons`, in the order Move to Front, Move Up, Move Down, Move to Back. Each has a tooltip with its name and key.

The icons are painted in `glyphs.rs`, as the tool icons are: two overlapping squares with the moving one filled, and a bar at the top or bottom for Front and Back. The icon font's own forward and backward icons are not in the free set the apps ship.

| App | Where the buttons go |
|---|---|
| NoteLiner | In the diagram toolbar across the top, after Undo and Redo, with a divider. This toolbar already scrolls sideways when the window is narrow. |
| BlockLiner | In the toolbar down the left side, after the tools, with a divider. See below. |

BlockLiner's side toolbar has a fit problem. It holds ten tool buttons at the top and six app buttons at the bottom. At 36 px a button with a 4 px gap that is about 640 px, and the window's minimum height is 640 px with the title bar and status bar inside it. Four more buttons need about 160 px more. This is arithmetic from the constants, not a measurement, and P3 starts with a screenshot at the minimum size to confirm it.

Three ways to fit them, in order of preference:

| Option | What | Cost |
|---|---|---|
| A (recommended) | One Order button in BlockLiner's side toolbar that opens the four as a list with icons, names, and keys, built like the More Shapes button | One more click in BlockLiner. NoteLiner still has four buttons. |
| B | Four buttons, and the tools scroll when the window is short | The side toolbar gains scrolling, which no LaanKit app has |
| C | Four buttons, and the minimum window height rises to about 800 px | The app no longer fits a 768 px screen |

See Q1.

### Right-click menu

Four items in a group of their own, after the line group and before Delete, with their keys shown:

```
Move to Front      Ctrl+Shift+]
Move Up            Ctrl+]
Move Down          Ctrl+[
Move to Back       Ctrl+Shift+[
```

- The group shows when the right-click selection includes a shape.
- Each item is greyed when its action is off.
- The menu is built in `Canvas::context_menu`, which both apps share, so it is the same in both.

### Command palette and keys

- Four commands in the Edit section (the Diagram section in NoteLiner, which renames it): "Move to Front", "Move Up", "Move Down", and "Move to Back".
- They are registered in `blockliner_egui::commands::commands()`, so both apps list them with the same keys.
- No command in either app uses the bracket keys today.
- The keys are bound as physical keys, since on many keyboard layouts the brackets need AltGr.

### Lines

Lines stay in front of all shapes and keep their own order.

- Putting a line behind a shape needs shapes and lines in one stacking list. That changes painting, hit-testing, the selection (lines are selected by index), and the file format, which has no place for it.
- Reordering lines among themselves has little effect on screen, and the writer always puts Mermaid edges before free lines, so their order would not survive a save.

### The file

Nothing is added to the format. A reorder changes the order of the node lines and of the `%% @shape` lines, both of which follow the model's order today.

One effect outside BlockLiner: Mermaid's own layout reads the node lines in order, so a diagram rendered by another Mermaid tool (GitHub, a docs site) can place sibling nodes differently after a reorder. Mermaid ignores the positions BlockLiner saves, so its picture already differs from BlockLiner's. See Q4.

## Changes

### blockliner-core

| File | Change |
|---|---|
| `model.rs` | `enum Order { Front, Up, Down, Back }` with `name()` and `ALL`. `Diagram::reorder(&mut self, ids: &[String], order: Order) -> bool`, which returns whether anything moved. `Diagram::can_reorder(&self, ids: &[String], order: Order) -> bool`. The doc comment on `Diagram.shapes` says the order is the stacking order. |
| `lib.rs` | `Order` in the re-exports. |

### blockliner-egui

| File | Change |
|---|---|
| `commands.rs` | `EditAct::Order(Order)`. `EditView::orderable: [bool; 4]`. `When::Orderable(Order)`. The four commands with their keys. |
| `editor.rs` | `apply` runs `Canvas::reorder`. `view` fills `orderable`. `show` maps `MenuAction::Order` to the act. |
| `canvas/mod.rs` | `MenuAction::Order(Order)`, the menu group, and `Canvas::reorder(d, order)`, which commits a label edit and calls the model with the selection. |
| `glyphs.rs` | `Glyph::Order(Order)` and its painting. |
| `tools.rs` | `order_buttons(ui, orderable) -> Option<Order>`, and `tool_buttons`, which replaces the loop both apps have. For option A, `order_menu_button`, built on the More Shapes popup. |

### The apps

| File | Change |
|---|---|
| `apps/blockliner/native/src/views.rs` | The Order button (or the four buttons) after the tools, and `tool_buttons` in place of the loop. `ToolbarProps` gains `orderable`. |
| `apps/blockliner/native/src/commands.rs` | The `When::Orderable` arm. |
| `apps/noteliner/native/src/views/diagram.rs` | The four buttons after Redo, and `tool_buttons` in place of the loop. `ToolbarProps` gains `orderable`. |
| `apps/noteliner/native/src/commands.rs` | The `When::Orderable` arm. |

### Docs

`apps/blockliner/README.md` (the Editing feature line, and "in model order" in the format table gains "which is the stacking order"), `apps/blockliner/native/README.md` and `apps/noteliner/native/README.md` (the keys and the menu), `apps/noteliner/README.md` (the diagram features), and the version in each app's `Cargo.toml` and metainfo file.

## Phases

| Phase | Work | Done when |
|---|---|---|
| P1 | `Order`, `reorder`, and `can_reorder` in the model | Core tests pass |
| P2 | The act, the view, the commands with their keys, the menu group, and the glyphs | In both apps the four keys and the right-click items reorder shapes, and one Ctrl+Z puts them back |
| P3 | A screenshot of BlockLiner at its minimum size, then the toolbar buttons in both apps and the shared `tool_buttons` | The buttons work in both apps, are greyed when off, and BlockLiner's toolbar fits at 800 by 640 |
| P4 | UI tests and docs | The READMEs describe the actions, and the suites pass |

## Tests

| Layer | Covers |
|---|---|
| Core | Each of the four on one shape and on several, with the selection's own order kept. Move Up steps over the nearest overlapping shape and skips shapes that don't overlap. Each returns false and changes nothing when it is off. `can_reorder` agrees with `reorder` in every case. A shape in a kept statement moves. Unknown ids are ignored. `write` then `read` gives the same order back, and the text is the same on a second save. |
| Editor (`blockliner-egui`) | An order change is one undo step. The selection is the same shapes afterwards. A label being edited is committed first. With only lines selected the view has all four off. |
| UI, BlockLiner (`egui_kittest`) | Two overlapping shapes: clicking the overlap selects the front one, Move to Back from the right-click menu makes the click select the other, and Ctrl+Z restores it. The toolbar buttons are disabled with nothing selected and enabled with the back shape selected. Ctrl+] and Ctrl+[ work. The saved file has the node lines in the new order. |
| UI, NoteLiner | The four toolbar buttons reorder a diagram in a tab, and the autosave writes the new order. |
| By hand | The exported SVG and PNG stack as the canvas does. The icons read clearly in both themes. The keys work on a keyboard layout where brackets need AltGr. |

## Risks

| Risk | Handling |
|---|---|
| The bracket keys don't reach the app on some layouts | Physical key bindings, checked by hand in P4. The toolbar, the menu, and the palette work without keys. |
| BlockLiner's side toolbar is already full | P3 starts with a measurement. Option A needs one button. |
| Copy and paste may not keep the copied shapes' stacking | `clipboard.rs` builds its diagram from the selection. P2 checks whether it follows list order or selection order, and a core test pins list order. |
| A reorder changes many lines of the file | The diff is the moved node lines and `@shape` lines only. The text is stable on the next save. |

## Out of scope

- Named layers with show, hide, and lock.
- Lines behind shapes, and reordering lines.
- A list of shapes in stacking order in the Properties pane.
- Grouping shapes.
- Dragging to reorder.

## Open questions

| # | Question | Recommendation |
|---|---|---|
| Q1 | BlockLiner's side toolbar looks full at the minimum window height. One Order button that opens the four (A), four buttons with a scrolling toolbar (B), or four buttons and a taller minimum window (C)? | A in BlockLiner, and four buttons in NoteLiner, whose toolbar has the room. |
| Q2 | Should the actions apply to lines as well as shapes? | No. Lines stay in front of shapes. Putting a line behind a shape is a larger change to the model and the file format, and is worth its own plan if you need it. |
| Q3 | Should Move Up and Move Down step over the nearest overlapping shape, or move exactly one place in the whole order as Visio and PowerPoint do? | The nearest overlapping shape, so each press changes what you see. |
| Q4 | A reorder changes the order of the node lines, which can change how other Mermaid tools lay the diagram out. Accept that, or keep the node lines in their first order and store the stacking order only in the `@shape` lines? | Accept it. It needs no format change, and other tools already ignore BlockLiner's positions. Keeping the first order needs a second order in the model. |
| Q5 | Are the keys right: Ctrl+] and Ctrl+[ for Up and Down, with Shift for Front and Back? | Yes. They are the usual keys for these actions in drawing apps. |
| Q6 | Should the two apps' `When` mappings be replaced by one generic function while this is being done? | Yes, if it turns out to be a small change. It removes one edit per app for every later diagram command. |
| Q7 | Is stacking order what you mean by layers, or do you also want named layers? | Stacking order only, from the four actions you listed. |
| Q8 | What versions? BlockLiner is 2.8.1 and NoteLiner is 2.4.1. | BlockLiner 2.9.0 and NoteLiner 2.5.0. |
