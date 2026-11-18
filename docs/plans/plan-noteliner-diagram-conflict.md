# NoteLiner: Ask Before Saving Over a Changed Diagram Plan

Status: implemented (2026-11-17) on the answers under "Decisions", not yet checked by eye. See "Changes during implementation". A note's save looks at the file first, and asks you when another program changed it (see `plan-noteliner-save-stamp-check.md`). A diagram's save does not. This plan gives a diagram the same comparison and the same File Changed on Disk dialog.

## Summary

- A diagram's save compares the file's stamp before it writes, as a note's does.
- A diagram that changed on disk under unsaved edits opens the File Changed on Disk dialog, with Reload and Keep Mine.
- Reload is one undo step, so Undo brings back your version.
- A diagram in a background tab asks when you return to its tab. A closed tab writes your version, with a line in the log.
- A diagram with no unsaved edits that reloads shows a toast, as a note does.
- The comparison in the backend is already built. The work is in the diagram's save state and in the app.

## Today

| Part | Where | Does |
|---|---|---|
| A diagram's save state | `Open` in `apps/noteliner/native/src/diagram.rs` | Holds the diagram as last written (`saved`), when it last differed (`dirty_since`), and whether a write is in flight (`saving`). |
| The autosave | `DiagramDoc::due` and `flush` | Writes once the diagram has been still for half a second and no drag is in progress. `flush` sets `saved` to the new diagram before the write has landed. |
| A save | `Backend::write_file` | Writes over whatever is on disk. A note uses `write_file_checked`, which compares the stamp. |
| A change on disk with no unsaved edits | `file_changed` in `apps/noteliner/native/src/app.rs` | The diagram is read again, as one undo step. Nothing tells you. |
| A change on disk under unsaved edits | The same | Your version is kept, with a line in the log. Their change is written over by your next save. |
| A change on disk while a save is in flight | The same | Taken as a change with no unsaved edits, because `is_dirty` does not count a write in flight. The diagram reloads, and then your write lands on the file. I read this from the code and did not try it. |
| The dialog | `Overlay::Conflict` and `pending_conflicts` in `app.rs` | For notes only. It is opened for the note in the tab shown. |

## The gaps

| # | Gap | Effect |
|---|---|---|
| G1 | A diagram's save does not compare the stamp. | You move a shape. Another program or an AI assistant changes the same file. Your autosave writes over their change, with nothing said. |
| G2 | When the check finds the change first, your version is kept without asking. | Their change is lost, and only the Log pane says so. |
| G3 | A write in flight is not counted as an unsaved edit. | The canvas shows their diagram while the file holds yours. |
| G4 | A reload with no unsaved edits is silent. | A shape can move under your eyes with no word about why. A note shows a toast. |

## Proposal

### The diagram's save state

| Change | Why |
|---|---|
| The autosave can be held, as a note's can. | `due` returns a write whenever the diagram differs from `saved`. Without a hold it would write again while the dialog is open. |
| A refused save puts the diagram back to unsaved. | `flush` has already set `saved` to your diagram. The state must remember that this was not written, or the diagram would count as saved. |
| `is_dirty` counts a write in flight. | Closes G3. |
| Keep Mine writes your diagram without the comparison and lets the autosave go on. | As for a note. |
| Reload takes the diagram on disk as one undo step, and drops the pending save. | The undo history is kept, so Undo brings your version back and saves it. |

### What you see

| Event | Result |
|---|---|
| The autosave finds the file changed | Nothing is written. The dialog opens. |
| The check finds the file changed under unsaved edits | The autosave is held and the dialog opens. The log line "Your version was kept" is no longer written here. |
| The check finds the file changed with no unsaved edits | The diagram reloads as one undo step, and a toast names it. |
| The file on disk reads as the same diagram as yours | Nothing is asked. The save goes ahead. |
| A drag, or a label being typed, is in progress | The dialog waits until the canvas is idle. See Q2. |
| You leave the tab | The switch goes ahead. The dialog opens when you return to the tab. |
| You close the tab, or it was closed when the save was refused | Your version is written, and the Log pane says so. |
| Reload, and the file on disk is not a diagram BlockLiner can read | Nothing changes, and a toast says the file could not be read. The dialog stays open, so you can choose Keep Mine. See Q3. |

The dialog is the one a note uses. Its text speaks of "unsaved edits" and names the file, so it needs no second wording.

### In the code

| Where | Change |
|---|---|
| `diagram.rs` | A `held` flag, a `conflicted` call for a refused save, a `reload` call, and `is_dirty` counting `saving`. `DiagramStash` gains `conflicted` for a background tab. |
| `app.rs` | A diagram's autosave and its save before switching tabs use `write_file_checked`. `save_conflict`, `file_changed`, `keep_edits_of_closing_tab`, and the dialog's two answers each gain the diagram's side. The dialog is opened for the file in the tab shown, a note or a diagram. |
| `backend.rs` | No change. |

## Decisions

| # | Question | Decision |
|---|---|---|
| Q1 | Should the dialog be the same one, or say "diagram"? | The same one. |
| Q2 | When the change is found during a drag, should the dialog wait for the drag to end? | Yes. It also waits for a label being typed. |
| Q3 | When the file on disk cannot be read as a diagram, should Reload be offered at all? | It is offered, and refused with a toast. The dialog stays open for Keep Mine. |
| Q4 | Should a toast show for each reload, when an AI assistant edits the open diagram many times? | Yes, as a note does. |

## Changes during implementation

| Planned | Built |
|---|---|
| `is_dirty` counts a write in flight. | It also counts a drag or a label in progress, and a diagram that differs from the one last written. Before, a change found during a drag replaced the diagram under the drag, and the shape being drawn was lost. Now it asks once the drag is done. |
| Not planned. | Reload writes the file back when an autosave was already in flight as the check found the change, as a note's Reload does. |
| Not planned. | A diagram in a background tab that is waiting to ask, and whose file changes again, asks about the newest file. |
| Not planned. | The saves on quit and on closing the project write a diagram that is waiting to ask. Without this, the hold would have dropped your version on quit. |
| The existing test that a dirty diagram keeps your version is changed. | There was no such test. |
| The Help page is updated. | No Help page describes changes on disk. The two READMEs are updated. |
| A touched file is tested for a diagram. | Not tested for a diagram. The comparison is the backend's, and the note's test covers it. |

## Out of scope

- The save on quit and on closing the project, for notes and diagrams. It still writes your version over the file.
- Showing what differs between your diagram and theirs.
- Merging the two.
- BlockLiner. It saves in its own code, and has a Save command.
- A diagram shown on a slide. It is read from the file and has no unsaved edits of its own.

## Phases

| Phase | Scope | Done when |
|---|---|---|
| P1: The save state | The hold, the refused save, Reload, and `is_dirty` in `diagram.rs`. | Unit tests in `diagram.rs` cover a refused save, a hold, Reload as one undo step, and a file that cannot be read. |
| P2: The app | The checked write, the dialog for the active tab and a background tab, the closed tab rule, the toast, the Help page, and the READMEs. | A diagram's save over a changed file does not write, and the dialog opens. Both answers work in the active tab and after a return to a background tab. Tests in `ui_tests_diagram.rs` cover each, with the autosave made to run before the check. |

P2 needs P1.

## Testing

- Unit tests in `diagram.rs`, as listed under P1.
- UI tests in `ui_tests_diagram.rs`: the change found by the save, the change found by the check, a tab that is left, a tab that is closed, a touched file, and the toast.
- The existing test that a dirty diagram keeps your version is changed to expect the dialog.
- By eye: move a shape while an AI assistant edits the same diagram through the MCP server, and choose each answer.

## Risks

| Risk | What limits it |
|---|---|
| The dialog opens during work on the canvas and interrupts it. | It waits for the canvas to be idle (Q2). |
| An assistant and you both edit a diagram, and the dialog opens again and again. | Each one is a change that is lost today. Reload is one undo step, so nothing of yours is lost by taking theirs. |
| A refused save leaves `saved` wrong, and the diagram shows as Saved while the file holds theirs. | P1's unit tests check the status after a refused save. |
| Counting a write in flight as dirty changes when a reload is taken. | Only for the half second or less that a write takes. P2 tests a change found while a save is in flight. |
