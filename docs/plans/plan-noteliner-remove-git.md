# NoteLiner: Remove Git Plan

Status: for review (2026-11-15). Takes Git out of NoteLiner. A project becomes a folder of notes that you open and edit, as BlockLiner opens a folder of diagrams. NoteLiner no longer commits, pushes, pulls, or shows a note's commit history.

## Summary

- Today a NoteLiner project must be a Git repository. Every change is committed, pushes follow 30 seconds later, and opening a project pulls.
- After this plan, NoteLiner opens any folder. It reads and writes the notes and `noteliner.json`, and runs no `git` command.
- These go away: the Remote Sync dialog, the branch chip in the status bar, the History pane, the Git identity fields, Clone from Remote, and the "Syncing changes to remote" wait when closing.
- An existing project keeps working. Its `.git` folder is left alone, and you can keep using Git on it yourself from a terminal or another tool.
- Two things Git did quietly need a decision: it was the only way to get a deleted or overwritten note back, and pulling was the only way NoteLiner noticed changes made elsewhere. See Q4 and Q5.
- The shared Git code in `laankit-core` stays, since PageLiner and ThreadLiner use it. See Q7.
- The work is in four phases: the core, the app, outside changes, and packaging and docs.

## Today

| Part | Where | Does |
|---|---|---|
| A project | `noteliner-core/src/project.rs` | A Git repository holding `noteliner.json` (the index: ids, names, the tree, tags), Markdown notes, diagrams, `_attachments/`, and `_templates/`. `Project` owns a `NoteGit`. |
| Opening | `Project::open` | A repository with an index pulls, then loads. A repository without one gets a new index and a commit. A folder that isn't a repository returns `needs_setup`, and the app shows the setup dialog. |
| Setting up | `Project::init`, the setup dialog and New Project dialog (`overlays/mod.rs`) | `git init`, or Clone from Remote with a URL, then the first commit. New Project asks for a Git name and email. |
| Saving | `Project::commit_and_schedule`, 14 call sites in the core and backend | Every create, save, rename, move, delete, tag change, attachment, template, and import commits, then schedules a push. |
| Identity | `check_git_config`, `Outcome::GitConfigRequired`, Project Settings | A commit with no `user.name` or `user.email` fails, and the app opens Project Settings to ask for them. |
| Sync | `overlays/sync.rs` (314 lines), `Backend::git_op`, `Event::GitState` and `GitDone` | The Remote Sync dialog: Connect, Disconnect, Pull, Pull and Rebase, Push, and Reset from Remote. |
| Status bar | `views/mod.rs` | The branch chip with a coloured dot and "↑2" or "↓1", which opens Remote Sync. |
| History | `views/history.rs` (344 lines), `Backend::history` and `history_content` | The History pane (Ctrl+H): a note's commits, and the note as it was at each. Read-only. |
| Closing | `Backend::close_project`, the Busy overlay | Flushes the pending push. Quitting blocks until it is done. |
| Log | The Sync Log panel (`views/mod.rs`) | Shows Git's output, and also MCP activity and import notes. |
| The wrapper | `noteliner-core/src/git.rs` (165 lines) over `laankit-core/src/git.rs` (620 lines) | `NoteGit`: commit, push, pull, the push timer, and the file log. `laankit-core::git` is also used by PageLiner and ThreadLiner. |
| Packaging | `apps/noteliner/native/Cargo.toml`, the Flatpak manifest | The `.deb` and `.rpm` depend on `git`. The Flatpak builds Git as a module and asks for the SSH agent socket. |
| Outside changes | Nowhere | NoteLiner has no check for files changed on disk. Changes from elsewhere arrive only through a pull, which reloads the index. |

## Proposal

### What a project is

A folder holding `noteliner.json`, the notes, `_attachments/`, and `_templates/`. The files and their format don't change. The only difference is that the folder need not be a Git repository.

`noteliner.json` stays. It holds what the files alone can't: each note's id, the tree's shape and order, and the tags. See Q2.

### Opening a folder

| The folder has | NoteLiner does |
|---|---|
| `noteliner.json` | Loads it. No pull, no identity check. |
| No `noteliner.json`, and Markdown or diagram files | Creates the index and adds the files it finds as notes at the top of the tree. See Q3. |
| Nothing | Creates an empty index. |

- The setup dialog (New Repository or Clone from Remote) is removed. Opening a folder always works.
- New Project becomes a folder picker with no name and email fields. See Q9 for merging it with Open Folder.
- Frontmatter is reconciled on open as it is today, and written without a commit.

### Saving

Every place that committed now only writes the files. There is no push timer, and closing a project or quitting has nothing to wait for.

### What is removed from the app

| Removed | Notes |
|---|---|
| Remote Sync dialog and the Remote Sync command | `overlays/sync.rs`, `Act::Sync`, `project.sync` |
| The branch chip in the status bar | The MCP chip and the note's details stay |
| History pane, Toggle History, and Ctrl+H | `views/history.rs`, `Act::ToggleHistory`, the History button in the note and diagram toolbars. See Q1. |
| Git identity in New Project and Project Settings | Project Settings keeps the project folder. If nothing else is left in it, it is removed, and its folder line moves to Settings. |
| Clone from Remote | With the setup dialog |
| "Syncing changes to remote" | The Busy overlay stays for imports |
| The "needs a Git identity" path | `Outcome::GitConfigRequired`, `Event::GitConfigRequired`, `needsGitConfig` |

### What stays, renamed

- The Sync Log panel still has a job: MCP activity and import notes. It is renamed Activity Log, and its empty text becomes "No activity yet." See Q8.
- The MCP tools keep working. The descriptions that say a tool "auto-commits to git" lose that sentence, and the consent dialog's "you see every operation in the Sync Log" names the Activity Log.

### Existing projects

- A project that is a Git repository opens as any folder does. NoteLiner doesn't touch `.git`, and doesn't delete it.
- Changes you make in NoteLiner show as uncommitted changes in that repository. You commit and push them yourself if you want to keep using Git.
- Nothing is migrated, since the files don't change.
- The first time a project with a `.git` folder is opened in the new version, one notice says that NoteLiner no longer commits or syncs, and that the repository is untouched. See Q6.

### Deleted and overwritten notes

Today every change is a commit, so a deleted note or a bad edit can be recovered from Git, in the History pane or by hand. With Git gone there is no way back once the editor's undo is lost.

The proposal is to send deleted notes and attachments to the system's trash in place of deleting them, as a file manager does. This covers the worst case with little code. A history of a note's earlier versions is a larger feature and is not part of this plan. See Q4.

### Changes made outside NoteLiner

Today a pull on open, or from the Sync dialog, reloads the index. With no pull, a project you sync with Git by hand, or keep in a synced folder, changes on disk while NoteLiner has it open, and NoteLiner would overwrite those changes with what it has in memory.

The proposal follows what BlockLiner does for a diagram changed outside the app:

- While the window has the focus, NoteLiner checks `noteliner.json` and the open notes for a changed size or time every few seconds, and when the window regains the focus.
- A changed index is reloaded, and the tree updates.
- A changed open note with no unsaved edits is reloaded.
- A changed open note with unsaved edits keeps your edits and shows a notice with Reload and Keep Mine.

See Q5.

## Changes

### noteliner-core

| File | Change |
|---|---|
| `git.rs` | Deleted. |
| `lib.rs` | The `git` module is removed, and the header comment no longer names the Tauri build. |
| `project.rs` | `Project::new()` takes no `NoteGit`. `open` loads or creates the index for any folder, and adopts loose files (Q3). `init`, `check_git_config`, `get_git_config`, `set_git_config`, and `commit_and_schedule` are removed, with their call sites. The header comment says a project is a folder. |
| `templates.rs`, `import/mod.rs` | The commits and push scheduling are removed. |
| `mcp.rs` | The tool descriptions. |
| Tests | The fixtures stop running `git init` and setting an identity. The test that reads a file's Git log after a rename checks the file on disk. |

The `Project` methods stay `async`, since the MCP server and the backend share the workspace behind an async lock. Making them plain functions is a separate tidy-up.

### noteliner-native

| File | Change |
|---|---|
| `backend.rs` | Removed: `Op`, `git_op`, `git_state`, `git_config`, `set_git_config`, `init_project`'s remote, `history`, `history_content`, the push flush in the two close paths, `Outcome::GitConfigRequired`, and the events `GitState`, `GitDone`, `GitConfig`, `GitConfigRequired`, `History`, and `HistoryContent`. Added for Q5: the outside-change check. |
| `app.rs` | The sync state, the History pane's state, the setup flow, and the handlers for the removed events. The reload on an outside change. |
| `overlays/sync.rs` | Deleted. |
| `overlays/mod.rs` | The setup dialog, the Git fields in New Project, and the Git section of Project Settings. |
| `views/history.rs` | Deleted. |
| `views/mod.rs` | The branch chip, `sync_label`, and `sync_dot`. The log panel's title and empty text. |
| `views/diagram.rs`, the note toolbar | The History button. |
| `commands.rs` | `project.sync` and `view.toggleHistory`. Ctrl+H is free. |
| `editor.rs`, `diagram.rs` | The reload path stays and is used by the outside-change check. The comments that name `git:resetToRemote` and a history restore are updated. |
| `examples/noteliner_demo_data.rs` | Writes the demo project without a repository. |
| `assets/help-pages.json` | The pages on sync, history, and setting up a repository. |
| UI tests | The fixtures lose `NoteGit`. Tests of the Sync dialog, the History pane, and the identity prompt are removed. New tests are listed below. |

### Packaging

| File | Change |
|---|---|
| `apps/noteliner/native/Cargo.toml` | `git` leaves the `.deb` and `.rpm` dependencies. The summaries and descriptions lose "with git sync". |
| `packaging/flatpak/com.nathanlaan.noteliner.yml` | The Git module and the SSH agent socket are removed. Network access stays for the update check. |
| `packaging/linux/com.nathanlaan.noteliner.metainfo.xml` | The summary, the description, and a release entry. |

### Docs

`apps/noteliner/README.md` (the features list, "What a project is", and a short section on using Git yourself), `apps/noteliner/native/README.md`, `ARCHITECTURE.md` (which apps use `laankit-core::git`, and the backend's description), `SECURITY.md` (NoteLiner leaves the Git lines, and its data is a folder), and `docs/packaging.md` (the dependency and Flatpak notes).

The plans in `apps/noteliner/docs/plans/` that build on Git (`plans-remote-pages-deploy.md` in particular) need a look after this is decided. They are not changed here.

## Phases

| Phase | Work | Done when |
|---|---|---|
| P1 | The core: `Project` without Git, opening any folder, adopting loose files, and the core tests | `noteliner-core` has no `git` module, and its tests pass without running `git` |
| P2 | The app: the backend, the removed dialogs, panes, chip, and commands, trash on delete, the renamed log, and the UI tests | NoteLiner opens a plain folder, edits and saves, and no `git` process is started |
| P3 | Outside changes: the check, the reloads, and the notice | A note changed by another program shows the new text, and unsaved edits are never lost without asking |
| P4 | Packaging, help pages, docs, and the version | The packages no longer depend on Git, and the docs describe a folder of notes |

## Tests

| Layer | Covers |
|---|---|
| Core | Opening a folder with an index, a folder with loose Markdown and diagram files, and an empty folder. Every write (create, save, rename, move, delete, tags, attachments, templates, import) leaves the files and index right with no repository present. A folder that is a repository opens, and its `.git` is byte-for-byte unchanged afterwards. |
| App | No `git` binary on the `PATH`: the app opens a project, edits, and closes. |
| UI (`egui_kittest`) | Opening a plain folder shows the tree with no setup dialog. The status bar has no branch chip. The palette has no Remote Sync or Toggle History. Deleting a note sends it to the trash (recorded in tests, as opening a folder is). A note changed on disk reloads, and one with unsaved edits shows the notice. |
| By hand | An existing project with a remote opens and shows the one-time notice. The `.deb` installs on a machine without Git. The Flatpak builds without the Git module. |

## Risks

| Risk | Handling |
|---|---|
| Losing the safety net of a commit on every change | Trash on delete (Q4), and the README says plainly that NoteLiner keeps no history. |
| Someone relies on the automatic push as their backup | The one-time notice on a project with a `.git` folder, and a release note. |
| Outside changes overwritten | P3. Until it ships, a project should not be changed elsewhere while it is open. P2 and P3 ship together. |
| Adopting loose files picks up files that aren't notes | Only `.md` and `.mmd` files at the top of the folder are adopted, and names starting with `_` or `.` are skipped. The file layout is confirmed at the start of P1. |
| Hidden uses of Git | The check in P2 is that the app runs with no `git` on the `PATH`. |

## Out of scope

- A history of a note's earlier versions that doesn't use Git.
- Sync of any kind.
- Removing Git from PageLiner and ThreadLiner, or `laankit-core::git` itself.
- Dropping `noteliner.json` so that the folder's files alone are the project.
- Making the `Project` methods synchronous.

## Open questions

| # | Question | Recommendation |
|---|---|---|
| Q1 | Should the History pane go entirely, or stay as a read-only view when the folder happens to be a Git repository and Git is installed? | Remove it. Keeping it keeps most of the Git code and the dependency, for a feature that would only work for some folders. |
| Q2 | Does `noteliner.json` stay? | Yes. The tree, the tags, and the ids live there. Without it NoteLiner is a different app. |
| Q3 | When a folder has Markdown files but no `noteliner.json`, should they be added to the tree as notes? | Yes, the `.md` and `.mmd` files at the top of the folder, in name order. This is what "open a folder of documents" suggests, and it lets you open notes written elsewhere. |
| Q4 | With no commits, should deleting a note move its file to the system trash? | Yes. It is the one loss Git protected against that the editor's undo doesn't. |
| Q5 | Should NoteLiner notice and reload files changed on disk? | Yes. Without it, using Git by hand or a synced folder on an open project loses changes. It is the largest new piece in this plan. |
| Q6 | Should a project with a `.git` folder show a one-time notice that NoteLiner no longer commits or syncs? | Yes, once per project. |
| Q7 | PageLiner and ThreadLiner also sync with Git. Is this change for NoteLiner only? | Yes, as you asked. `laankit-core::git` stays for them. |
| Q8 | Should the log panel stay, as Activity Log, for MCP and import messages? | Yes. Removing it would hide what an AI assistant does to your notes. |
| Q9 | Should New Project and Open Folder become one Open Folder action, as in BlockLiner? | Yes. A new project is an empty folder you open. |
| Q10 | What version? NoteLiner is 2.4.1, and the layers plan proposes 2.5.0. | 3.0.0, since a headline feature is removed. |
