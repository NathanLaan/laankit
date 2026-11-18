# NoteLiner: A Change Found by the Check Can Still Be Written Over Plan

Status: implemented (2026-11-18) with the recommended answers, not yet checked by eye. The test in P1 failed before the fix, which confirmed the cause. A save looks at the file before it writes, and asks when another program changed it. There is one frame in which that does not hold: the backend has found the change and the app has not yet heard of it. This plan closes that frame, and a second one of the same kind on quit.

## Summary

- The backend finds a change on disk, records the file's new stamp, and sends the change to the app.
- Until the app has handled that message, a save it sends is compared with the new stamp. The stamp matches, so the save is written over the other program's change.
- In the tab shown you still get the dialog, and Reload puts their version back. In a background tab, on quit, and on closing the project, their version is lost with nothing said.
- The fix is a counter for each file. The backend raises it with each change it reports, and a save says which count the app has seen. A save that is behind is refused, as a save over a changed file is today.
- I read the cause from the code after a test failed once in about 20 runs. I did not prove it. P1 starts with a test that does.

## Today

| Step | Where | Does |
|---|---|---|
| The check | `Backend::check` in `apps/noteliner/native/src/backend.rs`, `Core::changed_on_disk` | Finds that a file differs from its stamp, sets the stamp to the file's, and sends `Event::FileChanged`. |
| A tag or presentation change over a changed note | `Backend::save_index`, `Backend::set_presentation` | The same: the stamp is set, and `Event::FileChanged` follows. |
| A save | `Backend::write_file_checked` | Compares the file with the stamp. They match, so it writes. |
| The save on quit and on close | `Core::write_closing` | The same comparison, with the same result. |
| The app hears of the change | `file_changed` in `app.rs` | A frame later. By then the write may be queued or done. |

## The gaps

| # | Gap | Effect |
|---|---|---|
| G1 | A save sent between the backend finding a change and the app hearing of it is written. | In the tab shown, the dialog still opens, and Reload writes their version back. Keep Mine is as you would expect. |
| G2 | The same, as you leave the tab. | The tab is in the background when the message comes. It has an edit in flight, so it does not reload, and nothing asks. Their version is gone. |
| G3 | The same, as you quit or close the project. | The message is never handled. Your version is written over theirs, and no copy is made. |
| G4 | You leave a tab, its save is refused, and you quit before the refusal reaches the app. | The app does not know the tab is waiting to ask, and the edit is no longer pending in the editor. Nothing is written, and no copy is made. Your edits in that tab are gone. |

G1 to G3 need a check and a save to fall in the same frame. The check runs every three seconds, and when the window regains the focus. The second case is the likely one: you change a file in another program, return to NoteLiner, and type or switch tabs at once. G4 needs a tab switch and a quit within a few milliseconds.

## Proposal

### A count of the changes reported for each file

The backend keeps a count for each file beside its stamp. Each time it sends `Event::FileChanged` for the file, it raises the count, and the event carries the new value. The app keeps the last value it handled for each open file, and sends it with each save.

| The save's count | Result |
|---|---|
| Equals the backend's | The stamp is compared as today. |
| Is behind the backend's | The app has not heard of a change. Nothing is written, and `Event::SaveConflict` carries the file as it is on disk. |

The save on quit and on close carries the count too. One that is behind is written as a copy, as a file that changed is today.

The app then needs no new behaviour. A refused save already puts the edit back, holds the autosave, and asks in the tab shown or when you return to the tab. The result does not depend on the order in which the backend runs its tasks, because the comparison is made on values the app and the backend each hold.

The count follows a file that is moved or renamed, as its stamp does. It is forgotten when the project closes.

### An edit in flight is saved on quit

On quit and on close, the app also sends the edits that are in flight: a save was sent, and no answer has come back. The backend treats each as follows.

| The file on disk | Result |
|---|---|
| Holds the edit | The save landed. Nothing is done. |
| Has not changed since the app last read or wrote it | The save has not run yet. The edit is written. |
| Has changed | The save was refused, or would be. The edit is written as a copy. |

This closes G4, and covers a save that the backend had not run yet when you quit.

## Questions

| # | Question | My recommendation |
|---|---|---|
| Q1 | A count for each file, or should each save carry a hash of the content the edit was based on? | The count. The hash is the fuller answer, but each editor and each diagram would have to track what it was based on through loads, reloads, saves that rewrite the frontmatter, and tag changes. The count is held in one map in the app and one in the backend. |
| Q2 | Should G4 be in this plan? | Yes, as a second phase. It is the same kind of loss, and it uses the same save on quit. |
| Q3 | Should the tests keep the short wait before an outside change is written? | Yes. With the fix, a check in flight makes a test ask a different question from the one it means to. The wait keeps each test on its own path. |

## Out of scope

- Changing how often the check runs.
- File locks.
- A change made by another program during the write itself. The window is the time between one `stat` and one write.
- The other apps.

## Phases

| Phase | Scope | Done when |
|---|---|---|
| P1: The count | A test that fails today: run the check, leave its message unhandled, and save. Then the count in the backend, in `Event::FileChanged`, in the app, in `write_file_checked`, and in `ClosingSave`. | The new test passes for the tab shown, a tab that is left, quit, and a diagram. `a_save_does_not_write_over_another_programs_change` passes 100 runs in a row. |
| P2: Edits in flight | The edits in flight join the saves on quit and on close, and `Core::write_closing` handles them as in the table above. | A tab that is left over a changed file, followed by a quit with no frame between, leaves both versions on disk. A save in flight over an unchanged file is written once, with no copy. |

P2 depends on P1 for the comparison. P1 stands on its own.

## Testing

- A test hook runs the check and waits for the backend to finish, without stepping the app. The message is then waiting, and the next action is certain to fall in the gap.
- UI tests in `ui_tests_folders.rs` and `ui_tests_diagram.rs` for each row of "The gaps".
- A unit test in `backend.rs` for a save that is behind, and for the count following a moved file.
- By eye: change a note in another editor, click back into NoteLiner, and type at once. The dialog should open, and the file should still hold their version.

## Risks

| Risk | What limits it |
|---|---|
| My reading of the cause is wrong. | P1 begins with the test. If it passes before the fix, the plan stops there and I report what the test showed. |
| The dialog opens where the note used to reload. | Only when you had typed in the frame the change was found. Their version would have been written over in that case. |
| A save is refused though the app has heard of the change. | The app records the count before it does anything else with the message, so a later save cannot be behind. |
| More copies on quit. | A copy is made only when the file on disk is neither your edit nor what NoteLiner last read. |
