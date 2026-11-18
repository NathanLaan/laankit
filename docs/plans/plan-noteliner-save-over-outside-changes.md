# NoteLiner: The Last Saves That Write Over Outside Changes Plan

Status: implemented (2026-11-18) with the recommended answers, not yet checked by eye. A note's save and a diagram's save look at the file first, and ask you when another program changed it. Three paths still wrote without looking: the save on quit, the save on closing the project, and NoteLiner's own changes to a note's frontmatter. This plan closes them.

## Summary

- An autosave compares the file's stamp (time, size, and a hash of the content) before it writes. On a difference it writes nothing and the File Changed on Disk dialog offers Reload and Keep Mine.
- The save on quit and on closing the project does not compare. Your version replaces whatever is on disk.
- A file that is waiting to ask in a background tab is not written on quit at all. Its unsaved edits are lost.
- A tag change and a Presentation Settings change do read the file again before they write, so the text another program wrote is not lost at that moment. But the open editor is never told, and its next save writes the old text back.
- The plan has two phases: the frontmatter changes, then quit and close.

## Today

| Path | Where | Does |
|---|---|---|
| The save on quit | `shutdown` in `apps/noteliner/native/src/app.rs`, `Backend::write_file_now` | Writes the active tab's pending edit over the file, without the comparison. Quit does not wait for a dialog. |
| The save on closing the project | `go_home` in `app.rs`, `Backend::close_project` | The same, on the backend's thread. |
| A file waiting to ask, in the tab shown | The same two | A note is written. A diagram is written too. |
| A file waiting to ask, in a background tab | The same two | Not written. Only the active tab is saved on quit and on close. |
| A tag change | `Backend::save_index`, `Project::rewrite_frontmatter` in `crates/noteliner-core/src/project.rs` | Reads the note from disk, replaces the tags in its frontmatter, writes it, and sets the stamp to the result. |
| A Presentation Settings change | `Backend::set_presentation`, `Project::set_presentation` | Reads the note from disk, replaces the `presentation` block, writes it, and sets the stamp. |
| The open editor after either | `tags_into_editors` and the `Presentation` event in `app.rs` | The editor's frontmatter is updated to match. Its text is not read again. |

## The gaps

| # | Gap | Effect |
|---|---|---|
| G1 | The save on quit and on close does not compare the stamp. | You type, another program changes the file, and you quit within the same few seconds. Their change is gone with nothing said. |
| G2 | A background tab that is waiting to ask is not saved on quit or on close. | You leave a tab whose save was refused, and quit without returning to it. Your edits in that tab are gone. The dialog you never saw was the only notice. |
| G3 | A frontmatter change sets the stamp over an outside change it did not report. | Another program changes a note's text. Before the next check you add a tag from the Tags pane. The file on disk is right: their text with your tag. But the stamp now matches, so the check never reports their text, the editor still shows the old text, and your next edit saves the old text over theirs. |
| G4 | A frontmatter change replaces their tags or their `presentation` block. | If the other program changed the same field you changed, yours wins. This is kept. See Q4. |

I read G2 and G3 from the code and did not reproduce them in a running app.

## Proposal

### Frontmatter changes report what they found

Before `save_index` or `set_presentation` writes, the backend compares the stamp of each file it is about to rewrite, as an autosave does. The write then goes ahead as today, because it changes only the frontmatter of the file as it is on disk. Afterwards, for each file that had changed, the backend reads the result and sends it to the app as a change on disk.

The app already knows what to do with that:

| The note | Result |
|---|---|
| Is open with no unsaved edits | It reloads as one undo step, and a toast says so. |
| Is open with unsaved edits | The autosave is held and the dialog asks. |
| Is in a background tab | It reloads, or asks when you return to its tab. |
| Is not open | Nothing more is needed. |

No dialog is added and nothing of theirs or yours is lost. Diagrams have no frontmatter and are not touched.

### Quit and close keep both versions

The save on quit and on close compares the stamp. When the file has not changed, it writes as today. When it has, see Q1. My recommendation is a conflict copy:

| Step | Result |
|---|---|
| The file on disk | Left as the other program wrote it. |
| Your version | Written beside it as `name (your edits YYYY-MM-DD HHMM).md`, or `.mmd` for a diagram. |
| The next time the project opens | The copy is in the Files pane as a file of its own, and a line in the Log pane names it. |

The same is done for every tab that is waiting to ask, in the tab shown or in the background, which closes G2. Quit is never held up by a dialog.

A copy of a note keeps its frontmatter, so it has the same tags. Its `created` field is kept too. It gets its own id in the index when the project is next read, as any file added by another program does.

## Questions

| # | Question | My recommendation |
|---|---|---|
| Q1 | On quit or close with a conflict, what happens to the two versions? | A conflict copy, as above. The other choices are to stop the quit and show the dialog, which means a quit that sometimes does not quit, or to write your version as today, which keeps the loss. |
| Q2 | Should closing the project ask, since NoteLiner is still running then? | No. Use the copy for both, so there is one rule and one piece of code. |
| Q3 | What should the copy be called? | `name (your edits 2026-11-17 1432).md`. It sorts beside the note and says whose it is. |
| Q4 | When both sides changed the same frontmatter field, whose wins? | Yours, as today. You made the change a moment ago in NoteLiner, and their value is one click from being put back. |
| Q5 | Should the toast for a reload say why, when it came from a tag change? | No. The same toast as any reload. |

## Out of scope

- Showing what differs between the two versions.
- Merging them.
- File locks.
- Opening the copy for you, or a command that compares it with the note.
- Deleting old copies. They are your files.
- The other apps.

## Phases

| Phase | Scope | Done when |
|---|---|---|
| P1: Frontmatter changes | The comparison before `save_index` and `set_presentation`, and the change sent to the app afterwards. | A note changed by another program, then tagged in NoteLiner before any check, shows their text in the editor, and a later edit keeps it. The same for Presentation Settings. Tests in `ui_tests_folders.rs` and `ui_tests_present.rs` cover both, for a clean note and for one with unsaved edits. |
| P2: Quit and close | The comparison in the save on quit and on close, the conflict copy, the tabs that are waiting to ask, the log line on the next start, and the READMEs. | Quitting over a changed file leaves both versions on disk. A background tab that was waiting to ask is kept as a copy. A file that was only touched is written with no copy. Tests in `ui_tests_editor.rs`, `ui_tests_tabs.rs`, and `ui_tests_diagram.rs` cover each. |

The phases do not depend on each other. P1 is the smaller one, and closes the gap you are more likely to meet in ordinary use.

## Testing

- UI tests as listed in each phase. Each writes the outside change with no frame between it and the action under test, so the app's own check cannot run first.
- A unit test for the copy's name, including a name that is already taken.
- By eye: change a note in another editor, add a tag in NoteLiner at once, and type. Then change a note in another editor, type in NoteLiner, and quit at once.

## Risks

| Risk | What limits it |
|---|---|
| A conflict copy appears that you did not ask for. | It is made only when both sides changed the file and you quit before answering. The log names it, and its name says what it is. |
| The copy's name is already taken. | A number is added, as for a duplicated note. |
| The comparison slows quitting. | It is one `stat` for each file to be written, and a read only when the time or size differs. |
| The line in the log on the next start needs to be remembered across a quit. | It is written to the project's state file with the layout, and removed once shown. If that write fails, the copy is still in the Files pane. |
| A frontmatter change now makes a dialog open, where before it did not. | Only when another program changed the note and you have unsaved edits in it. That case loses their text today. |
