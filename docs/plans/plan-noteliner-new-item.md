# NoteLiner: New Item Dialog Plan

Status: implemented (2026-11-16) on the suggested answers, not yet checked by eye. NoteLiner can already make a folder, through a separate New Folder dialog that is reached from the FILES pane. This plan folds that dialog into the New File dialog, which becomes New Item, so one dialog and one key (Ctrl+N) make a note, a presentation, a diagram, or a folder.

## Summary

- The New File dialog is renamed New Item, and its Type row gains a fourth choice: Folder.
- With Folder chosen, the dialog asks for a name and the folder to put it in. Template and Tags are hidden, since a folder has neither.
- The separate New Folder dialog is removed. Every place that opened it opens New Item with Folder chosen.
- You can now choose the parent folder from a list when you make a folder. The old dialog only showed it.
- Nothing changes in how a folder is made on disk, or in the rules for its name.
- The work is small and is one phase.

## Today

| Part | Where | Does |
|---|---|---|
| The New File dialog | `new_file` and `NewFileForm` in `apps/noteliner/native/src/overlays/mod.rs` | A Type row (Note, Presentation, Diagram), File Name, Folder, Template, and Tags. Its title follows the type: New File, New Presentation, or New Diagram. It is also the Duplicate dialog, with the Type row hidden. |
| The New Folder dialog | `new_folder` and `NewFolderForm` in the same file | Shows the parent folder as text ("In"), and asks for a Folder Name. It checks the name as you type and says why a name cannot be used. |
| Making the folder | `Backend::create_folder` and `Project::create_folder` | Creates the folder on disk. An empty folder shows in the FILES pane. |
| Ways to New File | Ctrl+N, the toolbar and title bar buttons, the palette (New File, New Diagram..., and New Presentation), and the right-click menu in FILES | Open the New File dialog with a type chosen. |
| Ways to New Folder | The FILES header button, the palette (New Folder...), and the right-click menu in FILES | Open the New Folder dialog. |

The parent folder for either dialog is the selected folder, or the folder of the selected note.

## Proposal

### The dialog

```
 New Item
 ┌──────────────────────────────────────────────┐
 │ Type                                         │
 │ [ Note ] [ Presentation ] [ Diagram ] [Folder]│
 │                                              │
 │ Name                                         │
 │ [ Untitled                                 ] │
 │                                              │
 │ Folder                                       │
 │ [ (Top of the project)                   ▾ ] │
 │                                              │
 │ Template        (Note and Presentation only) │
 │ Tags            (not for Folder)             │
 │                                              │
 │                          [ Cancel ]  [ OK ]  │
 └──────────────────────────────────────────────┘
```

| Field | Note | Presentation | Diagram | Folder |
|---|---|---|---|---|
| Name | Yes | Yes | Yes | Yes |
| Folder | Yes | Yes | Yes | Yes, as the parent |
| Template | When the project has note templates | Yes | No | No |
| Tags | When the project has tags | When the project has tags | When the project has tags | No |

- The title is New Item for every type. Duplicate keeps its title and has no Type row, as today.
- The File Name label becomes Name, so it reads right for a folder.
- The name you have typed and the folder you have picked are kept when you switch type.
- With Folder chosen, the name is checked as it is today: the rules for a folder name, and a folder of that name already in the chosen parent. The reason shows under the field and OK is off. Changing the parent checks again.
- The Folder list is the scrolling list the dialog already has.
- If the dialog would be too narrow for four type buttons at a large UI scale, the row wraps to a second line.

### Ways in

| Way | Opens New Item with |
|---|---|
| Ctrl+N, the toolbar button, and the title bar button | Note. The tooltip becomes "New Item (Ctrl+N)". |
| The palette: New Item... (was New File) | Note |
| The palette: New Presentation, New Diagram..., and New Folder... | That type |
| The FILES header's New Folder button | Folder |
| Right-click in FILES: New File..., New Diagram..., and New Folder... | That type, in the folder you clicked |

The palette and menu entries for each type stay, because they are the quickest way to a type and people may have learned them. Only the first one is renamed.

### What is removed

`NewFolderForm`, `new_folder`, and `Overlay::NewFolder` go. The name check moves onto the New Item form. `Act::NewFolder` and the other actions stay and open the one dialog.

## Decisions for you

| # | Question | Suggested answer |
|---|---|---|
| Q1 | Is the title always New Item, or does it follow the type (New Folder, New Diagram) as it does today? | Always New Item. The Type row already shows which one is chosen, and a title that changes as you click is distracting. |
| Q2 | Do the palette entries New Diagram..., New Presentation, and New Folder... stay? | Yes. They open the same dialog with the type chosen. |
| Q3 | Does the FILES header keep its New Folder button? | Yes. It is the quickest way to a folder, and it now leads to the same dialog. |
| Q4 | Is the right-click entry New File... renamed New Item...? | Yes, to match the dialog. New Diagram... and New Folder... stay below it. |
| Q5 | Should OK on a new folder select it in FILES, so the next new item goes into it? | Yes. It already did, so nothing changed. |

## Out of scope

- Making several levels of folder at once by typing `Work/2026`. The name rules refuse a `/` today, and that stays.
- Templates for folders.
- Changing how a folder is renamed, moved, or deleted.
- BlockLiner, which has no folders in its project list.

## Phases

| Phase | Scope | Done when |
|---|---|---|
| P1: New Item | The Folder type, the Name label, the title, the name check on the form, the removed dialog, the renamed button and palette entry, the Help pages (New File becomes New Item, and Folders), and the READMEs. | Each way in opens New Item with the right type and folder. A folder is made in the chosen parent. A bad or taken name shows its reason and OK is off. Duplicate is unchanged. UI tests cover each. |

## Testing

- The existing folder tests in `ui_tests_folders.rs` open New Folder from the header, the menu, and the palette. They are changed to expect the New Item dialog with Folder chosen.
- New UI tests: switching type keeps the name and the folder, Template and Tags hide for Folder, picking another parent makes the folder there, and a taken name in the new parent turns OK off.
- The form's unit tests for the name check move with it.
- By eye: the four type buttons at 100% and 200% scale, in a light and a dark theme.

## Risks

| Risk | What limits it |
|---|---|
| Someone used to the header button gets a larger dialog than before. | Folder is already chosen, the name field has the focus, and Enter still makes the folder. It is the same two steps. |
| Four type buttons do not fit the compact dialog. | The row wraps. If it looks poor, the icons can stand alone with their names as tooltips. |
| A test or a help page still names New File. | The search for the old names is part of P1. Command ids (`file.new`, `file.newFolder`) do not change, so saved key bindings keep working. |
