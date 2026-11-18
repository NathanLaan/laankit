# NoteLiner: Check the File Before Each Save Plan

Status: implemented (2026-11-17) for notes, on the decisions below, not yet checked by eye. See "Changes during implementation". NoteLiner looks for changes made by other programs every few seconds, and saves your edits half a second after you stop typing. So a save usually ran before the next look, and wrote over a change it never saw. A note's save now looks at the file first, and asks you when the file is not the one NoteLiner last read or wrote.

## Summary

- NoteLiner keeps a stamp for each file it has read or written: the modification time, the size, and a hash of the content.
- A note's save compares the stamp just before the write.
- A file that changed is not written. You get the File Changed on Disk dialog, which offers Reload and Keep Mine.
- Keep Mine writes without the comparison.
- A file that was written again with the same content is not a change.
- A note with no unsaved edits that reloads from disk now shows a toast.
- Autosave stays. A model where you save by hand was considered and turned down.
- Diagrams, the save on quit, and NoteLiner's own changes to a note are not part of this. They are listed under "Out of scope".

## Before

| Part | Where | Did |
|---|---|---|
| The stamps | `stamps` in `apps/noteliner/native/src/backend.rs` | Held each file's modification time and size, set when NoteLiner read or wrote the file. |
| The check | `Backend::check` in the same file | Compared the stamp of each open file, every few seconds while the window had the focus. A file that differed was read and sent to the app as a change. |
| A change with no unsaved edits | `file_changed` in `apps/noteliner/native/src/app.rs` | The note reloaded, as one undo step, and nothing told you. |
| A change under unsaved edits | The same | The autosave was held and the File Changed on Disk dialog offered Reload and Keep Mine. |
| A save | `Backend::write_file` | Wrote the file and set the stamp. It did not look at the stamp first. |

## The gap

You type in a note. Another program, a sync client, or an AI assistant through the MCP server changes the same file. Your autosave runs before the next check, and their change is gone with no dialog and no line in the log. The same happened for a save as you left a tab.

## What was built

### The comparison

A note's save takes the project lock, reads the file's time and size, and compares them with the stamp NoteLiner holds. The check and every other write take the same lock, so nothing inside NoteLiner can change the file between the comparison and the write.

| The file on disk | The save |
|---|---|
| Has the time and size NoteLiner holds | Writes, and sets the new stamp. |
| Has no stamp held (NoteLiner has not read it) | Writes. |
| Is missing | Writes. A note removed by another program is put back with its index entry. |
| Has another time or size, and the same hash | Writes. A sync client or a backup can touch a file without changing it. |
| Has another time or size, and another hash | Does not write. The held stamp is set to the one on disk, and the save reports a conflict with the content on disk. |

The file is read only when the time or size differs. Another program can still write in the moment between the comparison and the write. That window is very short where it used to be several seconds, and closing it needs file locks that other programs would not honour.

### What you see

| Save | On a conflict |
|---|---|
| The autosave of the note in the tab shown | The autosave is held and the File Changed on Disk dialog opens. |
| An edit in the SLIDES pane | The same. |
| Keep Mine | Writes without the comparison. |
| Reload | Takes the content on disk. Nothing is written. |
| The save as you leave a tab | The switch goes ahead. The note you left keeps its unsaved edits, and the dialog opens when you return to its tab. |
| The save as you close a tab | There is nowhere to ask. Your edits are written, and the Log pane says so. |
| Closing a tab that is waiting to ask | The same. |

### The toast

A note with no unsaved edits that changes on disk reloads as before, and a toast names it. This is for the note in the tab shown. A note in a background tab reloads without one.

## Decisions

| # | Question | Decision |
|---|---|---|
| D1 | Autosave, or a Save command you run by hand? | Autosave. A manual save still needs the comparison and the dialog. It also adds Save commands, a question on every close and quit, unsaved edits in background tabs, crash recovery, and a disk that is stale for search, export, and the MCP server. |
| D2 | How much of the first plan to build? | Notes only. |
| D3 | For a tab in the background, when does the dialog open? | When you return to the tab. |
| D4 | When only the time changed and the content did not, write without asking? | Yes, by a hash kept with each stamp. |
| D5 | Should a clean reload tell you? | Yes, with a toast. |

## Changes during implementation

| Planned | Built |
|---|---|
| The write back that Reload does when an autosave was already in flight (built earlier on 2026-11-17) is removed. | It is kept. The check sets the held stamp when it reports a change, so a write already waiting behind the check passes the comparison. The write back still covers that order. |
| A tab left with a conflict has a mark on its tab. | No mark. The dialog opens when you return. |
| Not planned. | A tab closed with a conflict writes your edits, with a line in the log. Without this the edits would be lost with the tab. |
| Not planned. | The check also uses the hash, so a file that was only touched is no longer sent to the app as a change. |
| Tests in `backend.rs`. | The backend has no unit tests. The comparison is tested through the app in `ui_tests_folders.rs`. |

## Out of scope

- Diagrams. A diagram that changes on disk while you edit it keeps your version, with a line in the log, and its save does not compare the stamp.
- The save on quit and on closing the project. It writes your edits over the file, as before.
- A tag change and a Presentation Settings change. They rewrite the note's frontmatter from what NoteLiner read earlier, without the comparison.
- Showing what differs in the dialog. Reload is one undo step, so you can take theirs and undo to compare.
- Merging your edits with theirs.
- File locks.
- The other apps. BlockLiner and PageLiner save in their own code.

## Testing

| Test | Covers |
|---|---|
| `a_save_does_not_write_over_another_programs_change` in `ui_tests_folders.rs` | A touched file is written. A changed file is not, with no check before the autosave, and Keep Mine writes. A tab that is left asks on return, and Reload takes the file. A tab that is closed writes your edits and logs a line. |
| `a_reload_from_disk_shows_a_toast` in the same file | The toast. |
| `changes_made_by_other_programs` in the same file | The check finding the change first, as before. |
| `flush_before_switching_and_failed_saves` in `editor.rs` | The editor's state after a refused save. |

Not yet done by eye: edit a note in NoteLiner and in another editor at once, and let an AI assistant change the open note through the MCP server while you type.

## Risks

| Risk | What limits it |
|---|---|
| A file system with coarse times, or a sync client that rewrites files, makes the dialog open when nothing changed. | The hash is compared before asking. |
| A file system that reports another time than the one just written makes every save read the file. | The hash still matches, so the save goes ahead. The cost is one read for each save. |
| Each save now reads the file back once, to hash it. | Notes are small. Not measured on a large note. |
| The dialog opens more often than before, since most conflicts were missed. | Each one is a change that used to be lost. |
| A write by the MCP server is seen as another program's change. | That is wanted. It is how the open note takes an assistant's edit. |
| An assistant that edits the open note many times shows a toast each time. | Not limited. See whether it bothers you in use. |
