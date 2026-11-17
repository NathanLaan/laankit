# NoteLiner: Undo and Redo for Open Files Plan

Status: implemented (2026-11-16) on the answers under "Decisions", not yet checked by eye. See "Changes during implementation". NoteLiner already had undo and redo that is kept in memory for each open file. A note and a diagram each have their own history, and each tab keeps its own while it is open. So this plan does not build a new system. It lists what the two histories did, where they fell short, and the work that closed those gaps: Undo and Redo as buttons and commands you can see, a diagram history that survives a reload, a limit on the memory all the histories use together, and a way to reopen a closed tab with its history.

## Summary

- Undo and redo in memory exist today for notes, presentations, and diagrams, one history for each open tab.
- A note's history has no button and no command palette entry. It is reached only by Ctrl+Z, Ctrl+Shift+Z, and Ctrl+Y, with the cursor in the editor.
- A diagram's history is thrown away when another program changes the file. A note's is kept.
- A note's history is limited in memory. A diagram's is limited in steps only, and nothing limits all the tabs together.
- Closing a tab drops its history, and there is no way to reopen the tab.
- The work is in four phases, and all four are built.
- A history that survives a restart, and undo for renaming and deleting files, are not part of this. Both are listed under "Out of scope" with the reason.

## Today

| Part | Where | Does |
|---|---|---|
| A note's history | `crates/laankit-editor/src/history.rs` | Keeps each change as the text removed and the text put in, with the cursor before and after. Typing within half a second joins one step. A command such as a list edit, a paste, or a replace is its own step. |
| A note's limits | `MAX_STEPS` and `MAX_BYTES` in the same file | 1,000 steps and 8 MiB of changed text. The oldest steps are dropped past either. |
| A diagram's history | `crates/blockliner-core/src/history.rs` | Keeps a copy of the whole diagram for each step. A step is whatever changed between two moments the canvas was idle, so a drag is one step. |
| A diagram's limits | `LIMIT` in the same file | 200 steps. There is no limit in bytes. |
| Tabs | `apps/noteliner/native/src/tabs.rs` | A tab in the background keeps its editor, with the history, the cursor, and the scroll. Closing the tab drops them. |
| Keys | `crates/laankit-editor/src/editor.rs`, `crates/blockliner-egui/src/commands.rs` | Ctrl+Z, Ctrl+Shift+Z, and Ctrl+Y in both. |
| Buttons and palette | `apps/noteliner/native/src/views/diagram.rs`, `apps/noteliner/native/src/commands.rs` | A diagram has Undo and Redo buttons above the canvas and Undo and Redo in the palette. A note has neither. |

What happens to a history when something other than your typing changes the file:

| Event | Note or presentation | Diagram |
|---|---|---|
| Another program changes the file, and you have no unsaved edits | The new text is taken as one step. Undo brings your text back. | The diagram is read again and the history is emptied. |
| The same, for a tab in the background | The same as above. | The tab's state is dropped, and the diagram is read again when you show it. |
| Another program changes the file under unsaved edits | A dialog asks. Reload is one step. | Your version is kept, with a line in the log. |
| An edit in the SLIDES pane | One step in the note's history. | Does not apply. |
| NoteLiner changes the note's frontmatter (a tag from the Tags pane, Presentation Settings) | Not a step. Later steps are kept and moved to fit. A step that changed the frontmatter, and every step before it, is dropped. | Does not apply. |
| The file is renamed or moved | The history is kept. | The history is kept. |
| The tab is closed, or NoteLiner quits | The history is dropped. | The history is dropped. |

## The gaps

| # | Gap | Effect |
|---|---|---|
| G1 | A note's Undo and Redo are keys only. | You cannot see whether there is anything to undo, and nothing in the app tells you undo exists for a note. |
| G2 | Undo works only while the editor or the canvas has the keyboard. | After a click in the SLIDES pane or the preview, Ctrl+Z may do nothing. I read this from the code and did not try it in a running app. |
| G3 | A diagram reload empties its history. | An edit by another program, or by an AI assistant through the MCP server, cannot be undone in a diagram, and your earlier steps are lost with it. |
| G4 | A diagram's history has no limit in bytes, and there is no limit across tabs. | With many tabs open the worst case is 8 MiB for each note, plus 200 copies of each diagram. For ordinary notes and diagrams this is small. I have not measured a large diagram. |
| G5 | A closed tab cannot be reopened. | A tab closed by mistake loses its history, though its text is saved. |
| G6 | Undo restores only the first cursor after an edit with several cursors. | The text is right, and the other cursors are gone. |

## Proposal

### Undo and Redo as commands

Undo and Redo become NoteLiner commands that act on the file in the active tab, whatever its type.

| Place | Change |
|---|---|
| The command palette | Undo and Redo, under Edit, with their keys shown. They replace the two entries that today show for a diagram only. Each is greyed when there is nothing to undo or redo. |
| The keys | Ctrl+Z, Ctrl+Shift+Z, and Ctrl+Y reach the active file when the keyboard is in the SLIDES pane, the preview, or nowhere. In a text field of a pane or a dialog, they still undo that field's own typing. |
| Buttons | Undo and Redo on the left toolbar, above Files. They act on the active tab, and each is off when there is nothing to undo or redo. |

The old command ids for a diagram (`diagram.edit.undo` and `diagram.edit.redo`) keep working, so a saved key binding is not lost.

### A diagram reload keeps its history

When a diagram with no unsaved edits changes on disk, the new diagram is taken as one step, as a note already does. Undo then brings back the diagram as you had it, and your earlier steps are still there. A diagram tab in the background keeps its state and takes the change in the same way, where today its state is dropped.

A file that is no longer a diagram BlockLiner can read is refused as today, and the history is untouched.

### Limits on memory

| Limit | Today | Proposed |
|---|---|---|
| A note | 1,000 steps and 8 MiB | No change. |
| A diagram | 200 steps | 200 steps and 8 MiB. A step's size is taken as the size of the diagram written as text. |
| All open tabs together | None | 64 MiB. When the total passes it, the oldest steps of the background tabs are dropped first, a tab at a time from the one shown longest ago. The active tab is trimmed last. |

The total is checked when you switch tabs, not on every keystroke.

### NoteLiner's own changes to a note

A tag added from the Tags pane and a change in Presentation Settings stay as they are: not a step in the note's history. A tag rename can change many notes at once, and undo in one note's editor would put back only that one. See Q3.

### Reopening a closed tab

Reopen Closed Tab (Ctrl+Shift+W) brings back the tab you closed last, with its history, cursor, and scroll. NoteLiner keeps the last ten closed tabs of the project, inside the 64 MiB total, where they are the first to be trimmed. If the file changed on disk while the tab was closed, the new content is taken as one step, as for a tab in the background. A file that was deleted is skipped.

## Decisions

| # | Question | Decision |
|---|---|---|
| Q1 | Should a note have Undo and Redo buttons, and where? | Yes, on the main toolbar, active only when there is something to undo or redo. |
| Q2 | Is 64 MiB the right total, and should it be a setting? | 64 MiB, and not a setting. |
| Q3 | Should a tag change or a Presentation Settings change be a step in the note's history? | No. |
| Q4 | Is Reopen Closed Tab wanted? | Yes. |
| Q5 | Should undo with several cursors restore all of them? | Left out. |

## Changes during implementation

| Planned | Built |
|---|---|
| The plan said a note has no bar above the editor. | That was wrong. A note has a bar with its name, the list buttons, and the Preview toggle. The buttons went on the left toolbar, as Q1 asked, where they also serve a diagram. |
| Reopen Closed Tab on Ctrl+Shift+T. | Ctrl+Shift+T is the Tags pane. The key is Ctrl+Shift+W, beside Ctrl+W for Close Tab. |
| One Undo and one Redo in the palette under Edit, for every file type, replacing the diagram's two. | A note has Undo and Redo under Edit (`edit.undo` and `edit.redo`). A diagram keeps its two under Diagram, with their ids. Only one pair shows at a time. |
| Palette entries are greyed when there is nothing to undo. | The palette leaves out a command that does not apply, as it does for every other command. The toolbar buttons are greyed. |
| BlockLiner gains the reload change. | BlockLiner already took a reload as one step. Only NoteLiner emptied the history. BlockLiner gains the limit in bytes. |
| The total is checked when you switch tabs. | It is checked when you switch tabs and when you close one. |
| The oldest steps of a closed tab are trimmed. | The same. A closed tab can lose all its steps and still be reopened with its text, cursor, and scroll. |
| A measured figure for a large diagram replaces the estimate in G4. | Not measured. |
| Not planned. | A closed tab that you open again from the FILES pane is read from the file, and what was kept for it is dropped. Only Reopen Closed Tab brings the history back. |
| UI tests for the keys with the keyboard in the SLIDES pane, and for a text field in the Tags pane keeping its own undo. | The keys are tested with the keyboard nowhere. The text field rule is tested on the command's condition, not through a pane. |

## Out of scope

- A history that survives quitting NoteLiner. The request is for a history in memory. Every edit is also saved to the file within half a second, so a history on disk would be a second copy of the project's changes to keep in step.
- Undo for file operations: new, rename, move, duplicate, and delete. They change the project and not an open file, and each needs its own way back (delete would need a trash folder).
- A history pane that lists the steps and lets you jump to one.
- Versions of a note over time. That is what a git repository around the folder gives you.
- Undo in BlockLiner, PageLiner, and the other apps. BlockLiner gains the reload and byte limit changes, since it shares the diagram code.

## Phases

| Phase | Scope | Done when |
|---|---|---|
| P1: Commands | Undo and Redo as app commands for the active tab, in the palette with their keys, greyed when empty. The keys reach the active file from the SLIDES pane and the preview. The buttons, if Q1 is yes. | The palette's Undo and Redo work for a note, a presentation, and a diagram. Ctrl+Z after a click in the SLIDES pane undoes the last slide edit. A text field in a pane or dialog keeps its own undo. UI tests cover each. |
| P2: Diagram reload | A reload is one step, for the active tab and for a background tab. | After a change on disk, Undo shows the diagram as it was, and the steps before it still work. Tests in `blockliner-core`, `diagram.rs`, and `ui_tests_diagram.rs` cover both tabs. |
| P3: Memory | The byte limit for a diagram and the total across tabs. | Tests show a diagram's history stays under its limit, and that the total drops background steps before the active tab's. A measured figure for a large diagram replaces the estimate in G4. |
| P4: Reopen Closed Tab | The command, its key (Ctrl+Shift+W), the ten kept tabs, and the Help pages and READMEs. | A closed tab comes back with its history and cursor. A changed file is taken as one step, and a deleted one is skipped. UI tests in `ui_tests_tabs.rs` cover each. |

P1 to P3 do not depend on each other. P4 needs P3, because closed tabs count toward the total. The Help pages and READMEs are updated in each phase for what it changes.

## Testing

- Unit tests for the diagram history: a reload as a step, the byte limit, and the oldest steps dropped first.
- Unit tests for the total: which tab is trimmed first, and that the active tab's newest steps are never dropped.
- UI tests for Undo and Redo from the palette for each file type, the greyed state, and the keys with the keyboard in the SLIDES pane.
- UI tests that a text field in the Tags pane and in a dialog undoes its own typing and leaves the note alone.
- By eye: the buttons in a light and a dark theme, and Ctrl+Z after working in each pane.

## Risks

| Risk | What limits it |
|---|---|
| Ctrl+Z reaches the note when you meant a text field in a pane. | The app command runs only when no text field has the keyboard. Tests cover the Tags pane, the Search pane, and a dialog. |
| A reload taken as a step lets you undo into a diagram that no longer matches the file. | That is the point of the step, and a note already works this way. The undo is an edit like any other, and is saved to the file. |
| Trimming a background tab's history surprises someone who returns to it. | The total is large, and only the oldest steps go. The newest steps of every tab are kept. |
| The size of a diagram step is costly to work out. | It is worked out once for each step, when the canvas is idle, and not while you drag. P3 measures it. |
| Ten closed tabs hold memory for files you have finished with. | They are inside the total and are trimmed first. |
