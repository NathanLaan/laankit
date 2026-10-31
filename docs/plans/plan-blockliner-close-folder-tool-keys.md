# BlockLiner — Close Folder and Tool Shortcuts

Status: **Implemented (2026-10-30)** with the recommendations: no Close Folder
shortcut, `1`–`6` as the second set of tool keys, and the number hints always
shown (the offscreen render looked calm enough, so there's no Settings
toggle). Builds on the BlockLiner MVP ([`plan-blockliner.md`](plan-blockliner.md)).
Two small features:

1. A **Close Folder** button.
2. **Keyboard shortcuts** for the Rectangle, Rounded Rectangle, Line, Arrow
   and Text tools.

Effort: about one day for both, including tests.

---

## 1. Where things stand

### Folders

- **Opening:** Open Folder (toolbar, `Ctrl+O`, the empty state's button)
  opens a folder; its `.mmd` files fill the sidebar.
- **Restart:** the next launch reopens it (`lastProject`, `lastDiagram` in
  `ui-preferences.json`). A path on the command line opens that instead.
- **No way to close:** once a folder is open, nothing goes back to the
  "No folder open" state. Opening another folder replaces it.

### Tool shortcuts (already there)

Single keys pick a tool. They are Excalidraw's letters, registered in
`commands.rs` (`tool_key`):

| Tool | Key | With Shift |
|---|---|---|
| Select | `V` | — |
| Rectangle | `R` | keeps the tool after each shape |
| Rounded Rectangle | `O` | same |
| Line | `L` | same |
| Arrow | `A` | same |
| Text | `T` | same |

**Where they show:**
- the toolbar tooltips ("Rectangle (R)");
- Settings → Keyboard Shortcuts;
- the command palette ("Rectangle Tool  R");
- the native README.

**When they're off:** they don't fire while a label is being edited (the
text field has the keys) or while a dialog is open. `ui_tests_shapes`
covers them (for example, `O` then a click makes a rounded rectangle).

**What's missing** is how visible they are. The letters show only in a
tooltip, so they're easy to miss. There's also no second, positional set of
keys, which drawing tools commonly offer.

---

## 2. Close Folder

### Where

| Entry point | Detail |
|---|---|
| **Toolbar button** | Bottom group, right after Open Folder: Open Folder, **Close Folder**, Export, Settings, About. Icon: Font Awesome `folder-minus` (`f65d`), added to `laankit_egui::icons` as `FOLDER_MINUS`. Tooltip "Close Folder"; label "Close Folder". Disabled (not hidden) when no folder is open, like Export, so the toolbar doesn't shift. |
| **Command palette** | "Close Folder", in the File group. Available when a folder is open and no dialog is. |
| **Sidebar header** | Not added. The sidebar can be hidden (`Ctrl+E`), so it can't be the only place. One `+` in the header is enough there. |
| **Shortcut** | None by default (see §4, question 1). |

### What it does

1. **Finish the edit in progress.**
   - A label being edited is committed, as clicking elsewhere does.
   - A drag can't be in progress: the click on the button ended it. From
     the palette, the canvas is idle anyway.
2. **Save.** If the diagram has changes the autosave hasn't written, they
   are handed to it and written at once (`Backend::flush`), as when
   switching diagrams.
   - **If that write fails, the folder stays open.** The error toast says
     why. Closing must never lose an edit.
   - This needs `flush` to report failure. Today it only reports through a
     later `Event::SaveFailed`. Change it to return `Result` (or check right
     after flushing) so `close_folder` can decide straight away.
3. **Close.**
   - The diagram is closed: undo history dropped, selection cleared, the
     backend stops watching its file.
   - The project goes (`project = None`).
   - The window title goes back to "BlockLiner", and the canvas area shows
     the "No folder open" empty state.
4. **Forget it.** `lastProject` and `lastDiagram` are cleared (written as
   empty strings, which `Prefs` already reads as none). The next launch
   starts with no folder. The command line still opens a path.

### Edge cases

- **The sidebar's toggle:** `Ctrl+E` and the title-bar panel button are
  already off without a folder (`in_project`). Nothing to change.
- **Export, New, Rename and Delete Diagram:** these need a diagram or a
  folder, so they turn off by themselves. The Export dialog can't be open
  at the time: the button and the palette aren't reachable behind a modal.
- **An Open Folder dialog still open** (Linux: it doesn't block the window):
  if the user closes the current folder meanwhile, the folder they then pick
  opens as usual.
- **A diagram deleted outside** just before closing: there's nothing to save.
  The close goes ahead, with no error.

### Code

| File | Change |
|---|---|
| `commands.rs` | `Act::CloseFolder`; register `file.closeFolder` "Close Folder" `.when(in_project)`. |
| `views.rs` | The toolbar button; `ToolbarProps.can_close_folder` (or reuse `project_open`). |
| `app.rs` | `fn close_folder(&mut self)`: commit the label edit, save, flush and check, `close_doc`, `project = None`, `set_prefs({"lastProject": "", "lastDiagram": ""})`. |
| `backend.rs` | `flush` returns whether the last write succeeded. |
| `canvas/label.rs` / `canvas/mod.rs` | A `commit_label(&mut self, d: &mut Diagram)` the app can call, if it doesn't exist already. |
| `laankit-egui/src/icons.rs` | `FOLDER_MINUS`. First check the bundled Font Awesome subset has the glyph, as for `FILE_EXPORT`. |
| `config.rs` | Doc comment: `lastProject` may be empty (no folder). |

---

## 3. Tool shortcuts: what to add

### 3.1 Number keys, as a second set (recommended)

`1`–`6` pick the tools in toolbar order:

| Key | Tool |
|---|---|
| `1` | Select |
| `2` | Rectangle |
| `3` | Rounded Rectangle |
| `4` | Line |
| `5` | Arrow |
| `6` | Text |

- **Why:** positional keys are easy to remember from the toolbar, sit
  together under one hand, and are what Excalidraw and tldraw offer beside
  their letters.
- **Letters stay.** Shift + digit keeps the tool, like Shift + letter.
- **No clashes:** plain digits are unused in BlockLiner (zoom reset is
  `Ctrl+0`). The text field takes them while a label is being edited.
- **Physical keys:** the digits are bound as physical keys
  (`Binding::physical`, as Export's `Ctrl+Shift+E` is), so the number row
  works even on layouts that put the digits behind Shift (AZERTY types
  `&é"'(-` there).
- `commands::tool_digit` gives each tool's number, beside `tool_key`'s
  letter. The registry's `shortcut` text becomes "R, 2", for the palette and
  Settings.

**Alternative: Visio's keys.** Visio uses `Ctrl+1` pointer, `Ctrl+2` text,
`Ctrl+6` line and `Ctrl+8` rectangle. It has no keys for a rounded
rectangle or an arrow, so they would need inventing. Only worth it if
Visio muscle memory matters more than consistency (§4, question 2).

### 3.2 Show the key on each tool button

Draw the key in each tool button's bottom-right corner, small (9 px) and in
`text_faint`, as Excalidraw does. Then the shortcut is visible without
hovering.

- **Where:** in `views::tool_button`. It already paints the button's own
  ink, so the badge is a `painter.text` in the same place.
- **Which key:** the number, since that's the one tied to the button's
  position; the tooltip names both ("Rectangle — R or 2"). If 3.1 is
  dropped, the letter.
- **Off switch:** a Settings toggle "Show tool keys on the toolbar", on by
  default (`toolKeyHints`), for people who find the badges noisy. Optional;
  leave it out if the badges look calm enough in the offscreen renders.

### 3.3 Not changing

- **Single keys stay unmodified** (no `Ctrl`), as in other drawing tools.
- **No tool shortcuts while a label is being edited.** Typing "R" in a
  label must type an R.

---

## 4. Questions

1. **A shortcut for Close Folder?** Recommendation: none. `Ctrl+W` means
   "close window/tab" elsewhere, and a mistaken close only costs a
   re-open (nothing is lost). If you want one: `Ctrl+Shift+W`, or VS Code's
   `Ctrl+K F` (a chord the registry doesn't support yet).
2. **Which second set of tool keys:** `1`–`6` (recommended), Visio's
   `Ctrl+digit`, or none, if what you were after was that the existing
   letters weren't working or weren't visible? If pressing `R`, `O`, `L`,
   `A` or `T` did nothing for you, say when: it may be a focus bug rather
   than a missing feature.
3. **Badges on the tool buttons (3.2):** always on, or with the Settings
   toggle?

---

## 5. Tests

All headless (egui_kittest), with visual checks rendered offscreen as before.

**Close Folder:**
- **Plain close:** with a folder and a diagram open, the toolbar button
  closes it.
  - The empty state reads "No folder open".
  - The window title is "BlockLiner".
  - `lastProject` and `lastDiagram` are empty.
  - A new app on the same prefs starts with no folder.
- **Unsaved edit:** an edit not yet written (autosave delay running) is on
  disk after Close Folder.
- **Label edit in progress:** a label being edited when the palette's Close
  Folder runs is committed and saved.
- **Failed write:** an unwritable file (read-only folder) keeps the folder
  open and shows the error.
- **Availability:** the button is disabled, and the palette entry absent,
  with no folder.

**Tool keys:**
- **Each digit:** `1`–`6` picks its tool; Shift + digit keeps it after a shape.
- **While typing:** digits typed into a label go into the label, not to the
  tools.
- **Listings:** the palette and Settings list "R, 2" (and so on).
- **Render:** an offscreen render of the toolbar with the badges.

---

## 6. Docs

- `apps/blockliner/native/README.md`: the keys table (`1`–`6`) and Close
  Folder.
- `docs/plans/plan-blockliner.md` §5 "Files": closing a folder.
- `apps/blockliner/README.md` features: no change needed beyond "open any
  folder" (optional mention).
