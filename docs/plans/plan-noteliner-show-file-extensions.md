# NoteLiner: Show File Extensions in the Files Pane Plan

Status: for review (2026-11-19). Adds a checkbox to Settings, "Show file extensions", that puts each file's extension after its name in the Files pane. It is off by default, so nothing changes until you turn it on.

## Summary

- The Files pane shows each file's name with no extension. An icon tells a note, a presentation, and a diagram apart, but `.mmd` and `.mermaid` look the same, and so would any other kind of file added later.
- With the setting on, a row reads `Budget.md` or `Floor Plan.mmd`.
- The name in the pane is not the file's name on disk. A note called `My Plan` is the file `my-plan.md`. The plan recommends showing the name you gave it with the extension added, `My Plan.md`. See Q1, which is the one decision that changes what you see.
- The change is one preference, one checkbox, and one place where the row's text is drawn. It is a single phase.

## Today

| Part | Where | Does |
|---|---|---|
| A row in the Files pane | `paint_row` in `apps/noteliner/native/src/views/tree.rs` | Draws the icon, then `entry.name`, cut short with an ellipsis when the pane is narrow. |
| The name | `Entry::name` in `apps/noteliner/native/src/model.rs` | The name in the project's index, as you typed it. |
| The file on disk | `Entry::filename`, `file_slug` in `crates/noteliner-core/src/project.rs` | A slug of the name, in lowercase with dashes, plus the extension. A file added by another program keeps its own name, and its name in the index is that name without the extension. |
| The kind | `FileKind::of` | Read from the extension: `.mmd` and `.mermaid` are diagrams, and anything else is a note. |
| Rename | `Project::rename_file` | Takes a new name, and keeps the extension. |
| What a screen reader hears | `widget_info` in `tree.rs` | The name. The UI tests find rows by this label. |
| Preferences | `Prefs` in `apps/noteliner/native/src/config.rs`, `ui-preferences.json` | Read once at start, and changed from Settings with `SettingsAction::SetPrefs`. |
| Settings | `apps/noteliner/native/src/overlays/settings.rs` | Tabs for the UI, the project, the editor, diagrams, and MCP. Each checkbox is a `toggle_option`. |

## Proposal

### The setting

| Item | Value |
|---|---|
| Label | Show file extensions |
| Where | Settings, the UI tab, in a new group called Files below Window. See Q2. |
| Description under the group's title | Shows each file's extension after its name in the Files pane. |
| Key in `ui-preferences.json` | `filesShowExtensions` |
| Default | Off |
| When it takes effect | At once, with Settings still open. No restart. |
| Scope | The app, not the project, as the sort order is. |

### What a row shows

| The file | Setting off | Setting on |
|---|---|---|
| A note called `Budget`, in `budget.md` | Budget | Budget.md |
| A presentation called `Kickoff`, in `kickoff.md` | Kickoff | Kickoff.md |
| A diagram called `Floor Plan`, in `floor-plan.mmd` | Floor Plan | Floor Plan.mmd |
| A diagram added by another program as `wiring.mermaid` | wiring | wiring.mermaid |
| A file with no extension | Its name | Its name, unchanged |
| A folder | Its name | Its name, unchanged |

The extension is shown as it is on disk, including its case.

### What stays the same

| Part | Behaviour with the setting on |
|---|---|
| Sorting by name | Sorts by the name, as today. The extension is not part of the order. |
| Renaming in the pane | The field holds the name without the extension. The extension cannot be changed there, as today. |
| A narrow pane | The name is cut short and the extension stays whole: `Quarterly Bud….md`. See Q3. |
| Tag counts and the paperclip | Unchanged, at the right of the row. |
| What a screen reader hears | The same text the row shows, so the extension is read out when it is shown. |
| Tabs, the title bar, Search, Backlinks, the Command Palette | Unchanged. See Q4. |
| Files on disk and the index | Nothing is written. The setting only changes what is drawn. |

## Questions

| # | Question | My recommendation |
|---|---|---|
| Q1 | With the setting on, should a row show the name plus the extension (`My Plan.md`), or the file's real name on disk (`my-plan.md`)? | The name plus the extension. The pane stays a list of the names you gave, the order does not change, and the rename field still starts from what you see. The cost is that `My Plan.md` is not a file that exists. If what you want is to see the real file names, say so, and the plan changes: the row would show `my-plan.md`, and the sort and the rename field would each need a decision. |
| Q2 | Which Settings tab? | UI, in a new Files group. The Project tab is about what NoteLiner writes into your files, and this setting writes nothing. |
| Q3 | In a narrow pane, should the extension stay visible? | Yes. Otherwise the setting does nothing when the pane is narrow, which is when names are hardest to tell apart. |
| Q4 | Should the extension also show in tabs, Search, Backlinks, and the Command Palette? | No. You asked for the Files pane, and the setting's name says where it applies. It can be widened later without changing the preference. |
| Q5 | Should the extension be drawn in the muted text colour? | No, the same colour as the name. One colour is simpler, and a muted extension is harder to read on the selected row. |
| Q6 | Should the Command Palette get a command to turn it on and off? | No. It is a setting you choose once. |

## Out of scope

- Changing a file's extension from NoteLiner.
- Showing the path or the file's real name. See Q1.
- Showing files that are not notes or diagrams.
- The other apps.

## Phases

| Phase | Scope | Done when |
|---|---|---|
| P1 | The preference in `Prefs` and `SettingsState`, the checkbox, the flag passed through `SidebarProps` and `TreeProps` to `paint_row`, the row's text and its label for a screen reader, the row in `config.rs`'s table of keys, and the README. | Turning the checkbox on adds extensions to the rows at once. Turning it off removes them. The choice is kept after a restart. With it off, every existing test passes unchanged. |

## Testing

- A unit test in `config.rs`: the preference is off when the key is missing, and is read back after it is set.
- A UI test in `ui_tests_panes.rs`: open Settings, turn the checkbox on, and find the rows `Alpha.md` and a diagram's `.mmd` by label. Turn it off, and find `Alpha` again.
- A UI test that a rename started with the setting on begins with the name alone, and that the file keeps its extension.
- A UI test at a narrow pane width that the row's text still ends with the extension.
- By eye: a project with a note, a presentation, a `.mmd` diagram, and a `.mermaid` diagram, with the pane wide and narrow, in a light and a dark theme.

## Risks

| Risk | What limits it |
|---|---|
| `My Plan.md` is read as the file's real name. | The description under the setting says it shows the extension after the name. Q1 is there so you can choose the other way before anything is built. |
| The UI tests find rows by name, and the label changes with the setting on. | The default is off, and the tests run with a fresh configuration. |
| Keeping the extension whole leaves no room for the name in a very narrow pane. | When the extension alone does not fit, the whole text is cut short as today. |
