# NoteLiner: Remove Git and Use Folders Plan

Status: for review (2026-11-15). Takes Git out of NoteLiner and makes the folders on disk the tree. A project becomes a folder of notes and sub-folders that you open and edit. NoteLiner no longer commits, pushes, pulls, or shows a note's commit history, you can add folders and organize notes in them, and the editor shows each note's frontmatter.

## Summary

- Today a NoteLiner project must be a Git repository. Every change is committed, pushes follow 30 seconds later, and opening a project pulls.
- Today the tree is not folders. Every note is a file at the top of the project folder, and a note sits "under" another note through a `parentId` in `noteliner.json`.
- After this plan, NoteLiner opens any folder and runs no `git` command. The sidebar shows the folder's real sub-folders, with the notes and diagrams inside them.
- You can add, rename, move, and delete folders. Moving a note in the tree moves its file on disk.
- A note can no longer be the parent of another note. Only folders hold notes. NoteLiner has no step that converts an old project. See "Older projects".
- These go away: the Remote Sync dialog, the branch chip in the status bar, the History pane, the Git identity fields, Clone from Remote, and the "Syncing changes to remote" wait when closing.
- Deleted notes and folders go to the system's trash.
- NoteLiner notices notes and folders added, changed, moved, or removed by other programs.
- `noteliner.json` stays, for what the files and folders can't hold: ids, names, a diagram's tags, and attachments.
- The editor shows a note's whole file, with the frontmatter at the top, and you can edit it there. The frontmatter holds the note's tags and created date. It no longer holds an id, a name, or an updated time.
- User Order is removed. The tree sorts by name or by date, with folders first.
- The shared Git code in `laankit-core` stays, since PageLiner and ThreadLiner use it.
- The work is in seven phases: the core without Git, folders in the core, the app without Git, folders in the app, frontmatter in the editor, outside changes, and packaging and docs.

## Decisions

| # | Decision |
|---|---|
| Q1 | The History pane is removed. |
| Q2 | `noteliner.json` stays. |
| Q3 | A folder's Markdown and diagram files are added as notes, in name order. The tree can also be sorted by modification date. |
| Q4 | Delete moves the file to the trash. |
| Q5 | NoteLiner notices and reloads files changed on disk. |
| Q6 | No notice is shown for a project that has a `.git` folder. |
| Q7 | The change is for NoteLiner only. `laankit-core::git` stays. |
| Q8 | The log panel stays, as Activity Log, for MCP and import messages. |
| Q9 | New Project and Open Folder become one Open Folder action. |
| Q10 | The version is 3.0.0. |
| Q11 | NoteLiner has no conversion. The one project with notes under notes was converted by hand, with each parent note as `<Folder>/index.md`. |
| Q12 | Attachments are stored with the note: in `_attachments/` inside the note's folder. |
| Q13 | User Order is removed. Folders come first, and the name and date sorts stay. |
| Q14 | A note with no frontmatter gets it the first time you edit the note, not when the folder is opened. |
| Q15 | A diagram's file doesn't carry its id. |
| Q16 | An AI assistant can't delete a folder that still has notes in it. |
| Q17 | A folder's name in NoteLiner is its name on disk. Folders aren't slugged. |
| Q18 | Note files stay slugged. |
| Q19 | A folder's `index.md` is an ordinary note. |
| Q20 | The frontmatter has no `name`. A note's name is in the index, and you rename a note in the tree. |
| Q21 | The frontmatter has no `updated`. The index's `modifiedAt` and the file's own date say when a note changed. |
| Q22 | The frontmatter always shows, in a dimmer colour. There is no setting or fold to hide it. |
| Q23 | The frontmatter has no `id`. On disk a note is known by its path. |
| Q24 | The MCP tools `read_note` and `update_note` read and write the text without the frontmatter. |
| Q25 | The index keeps an id for each note and diagram. No file carries it. |
| Q26 | A deleted frontmatter block comes back on the next save, with the note's tags and created date. |
| Q27 | NoteLiner doesn't remove the `id`, `name`, and `updated` lines already in a note. It keeps them as it keeps any other field. |
| Frontmatter | The editor shows a note's frontmatter. |
| Folders | The folders on disk are the tree, and NoteLiner can add folders. |

## Today

| Part | Where | Does |
|---|---|---|
| A project | `noteliner-core/src/project.rs` | A Git repository holding `noteliner.json` (the index), Markdown notes, diagrams, `_attachments/`, and `_templates/`. `Project` owns a `NoteGit`. |
| The tree | `parentId` and `order` on each index entry, `model.rs`, `views/tree.rs` | Notes nest under notes. All the files are side by side at the top of the project folder, named from a slug of the note's name. Sub-folders on disk are not read. |
| Sorting | `SortMode` in `model.rs` | User Order (drag to reorder), Name A-Z, Name Z-A, Last Modified, and Created. |
| Opening | `Project::open` | A repository with an index pulls, then loads. A repository without one gets a new index and a commit. A folder that isn't a repository returns `needs_setup`, and the app shows the setup dialog. |
| Setting up | `Project::init`, the setup dialog and New Project dialog (`overlays/mod.rs`) | `git init`, or Clone from Remote with a URL, then the first commit. New Project asks for a Git name and email. |
| Saving | `Project::commit_and_schedule`, 14 call sites in the core and backend | Every create, save, rename, move, delete, tag change, attachment, template, and import commits, then schedules a push. |
| Deleting | `Project::delete_file` | Removes the file with `fs::remove_file`, and moves the note's children up to its parent. |
| Identity | `check_git_config`, `Outcome::GitConfigRequired`, Project Settings | A commit with no `user.name` or `user.email` fails, and the app opens Project Settings to ask for them. |
| Sync | `overlays/sync.rs` (314 lines), `Backend::git_op`, `Event::GitState` and `GitDone` | The Remote Sync dialog: Connect, Disconnect, Pull, Pull and Rebase, Push, and Reset from Remote. |
| Status bar | `views/mod.rs` | The branch chip with a coloured dot and "↑2" or "↓1", which opens Remote Sync. |
| History | `views/history.rs` (344 lines), `Backend::history` and `history_content` | The History pane (Ctrl+H): a note's commits, and the note as it was at each. Read-only. |
| Closing | `Backend::close_project`, the Busy overlay | Flushes the pending push. Quitting blocks until it is done. |
| Log | The Sync Log panel (`views/mod.rs`) | Shows Git's output, and also MCP activity and import notes. |
| Links | `links.rs` | `[[Note Name]]` finds a note by its name, anywhere in the project. |
| Attachments | `_attachments/` at the top of the project, listed on the note's index entry | A note refers to one as `./_attachments/att-1234.png`. NoteLiner reads it from the project's `_attachments/` folder. |
| Frontmatter | `frontmatter.rs`, `Project::read_file` and `write_file` | Each note's file carries its `id`, `name`, `tags`, `created`, and `updated`, and whatever else you put there by hand in another program, such as a deck's `presentation:` block. The editor never shows it: reading a note strips it, and saving puts it back from the index. A diagram's file carries nothing but the diagram. A setting, Write YAML frontmatter to .md files, turns the writing off. |
| MCP | `mcp.rs` | `list_notes` returns each note's `parentId`, and `create_note` takes one. |
| The Git wrapper | `noteliner-core/src/git.rs` (165 lines) over `laankit-core/src/git.rs` (620 lines) | `NoteGit`: commit, push, pull, the push timer, and the file log. |
| Packaging | `apps/noteliner/native/Cargo.toml`, the Flatpak manifest | The `.deb` and `.rpm` depend on `git`. The Flatpak builds Git as a module and asks for the SSH agent socket. |
| Outside changes | Nowhere | NoteLiner has no check for files changed on disk. Changes from elsewhere arrive only through a pull, which reloads the index. |

## Proposal

### What a project is

A folder, with everything under it:

| On disk | In NoteLiner |
|---|---|
| A sub-folder, at any depth | A folder in the tree |
| A `.md` file | A note |
| A `.mmd` or `.mermaid` file | A diagram |
| `noteliner.json` at the top | The index. Not shown. |
| `_attachments/` in any folder | The attachments of the notes in that folder. Not shown in the tree. |
| `_templates/` at the top | Templates, as today. Not shown in the tree. |
| Any folder or file whose name starts with `.` or `_` | Skipped, with everything in it. This covers `.git`. |
| Any other file (a PDF, an image) | Left alone and not shown |
| A symbolic link to a folder | Not followed |

The folder you open is the top of the tree. It has no row of its own.

### What the index holds

`noteliner.json` no longer says what exists or where. The folders say that. The index holds what they can't:

| Field | Why it isn't in the files |
|---|---|
| `id` | No file carries an id. The id is what a tab, a link, a selection, and an MCP call use for a note or diagram, so a rename or a move doesn't change it. |
| `filename` | The file's path from the project folder, with `/` between the parts, such as `work/meetings/kickoff.md`. It is how an entry is matched to a file. |
| `name` | A file's name is a slug, so the name you typed ("SDLC / ALM" for `sdlc-alm.md`) is kept here. |
| `tags` | For diagrams. For notes they mirror the frontmatter, as today. |
| `attachments` | The list of a note's attachments. |
| `createdAt` and `modifiedAt` | Dates that survive a copy of the folder. |
| The tag colours | They belong to the project, not to a note. |

`parentId` and `order` are removed. The index's version becomes 4.

### Opening a folder

NoteLiner walks the folder and brings the index in line with what it finds. The walk decides what exists, and the index only adds facts to it.

| The walk finds | NoteLiner does |
|---|---|
| A file the index lists at that path | Keeps the entry. |
| A file the index doesn't list, with the same file name as exactly one entry whose file is gone | Treats it as moved: the entry keeps its id, name, tags, and attachments, and gets the new path. |
| Any other file the index doesn't list | Adds an entry: a new id, the name from the file's name, the tags and created date from the frontmatter, and the other dates from the file. |
| An entry whose file is gone and wasn't moved | Removes the entry. Its attachments stay in that folder's `_attachments/`. |
| No `noteliner.json` | Creates one from the walk. |
| An empty folder | Shows it in the tree, empty. |

- Opening writes `noteliner.json` and nothing else. A note that had no frontmatter gets it the first time you edit that note.
- The setup dialog (New Repository or Clone from Remote) is removed. Opening a folder always works.
- New Project is removed. A new project is an empty folder you open.
- The walk stops at 20,000 files and says so, so that opening a home folder by mistake doesn't hang the app.

### The tree

- A folder is a branch that opens and closes. A note or diagram is a leaf.
- Folders come first in each folder, in name order, in every sort mode. Notes and diagrams follow, in the sort you chose.
- The sort modes are Name A-Z, Name Z-A, Last Modified, and Created. User Order is removed, and a saved `filesSortMode` of `user` reads as Name A-Z.
- Dragging a note or folder only moves it to another folder. There is no drag to reorder.
- Which folders are open is remembered with the project's other view state.
- While a tag filter hides notes, a folder with nothing left to show is hidden too. With no filter, empty folders show.
- A search result and a tab's tooltip show the note's folder, since two notes in different folders can share a name.

### Folders

| Action | Where | Does |
|---|---|---|
| New Folder | A button in the Files pane's header, the right-click menu on a folder or on the empty part of the tree, and the palette | Asks for a name, and creates the folder inside the selected folder, or beside the selected note, or at the top with nothing selected. |
| New File and New Diagram | As today | Create the file in the same place New Folder would. |
| Rename | Double-click, or the right-click menu, as for a note | Renames the folder on disk. Every entry under it gets its new path. |
| Move | Drag onto a folder, or Move… in the right-click menu, which lists the folders | Moves the folder or note on disk. Dropping on the empty part of the tree moves it to the top. |
| Delete | The right-click menu, or Delete File with a folder selected | Asks first, with the number of notes, diagrams, and other files inside, then sends the folder and everything in it to the trash. Tabs for the notes inside close. |
| Open in File System | The right-click menu | Opens the folder in the system's file manager. |
| Duplicate and Import | As today | The copy, or the imported note, goes in the same folder as the selection. |

Rules for a folder's name:

- The name on disk is the name you type. Folders aren't slugged, since there is nowhere else to keep the name you typed.
- A name can't be empty, be `.` or `..`, start with `.` or `_`, or hold `/`, `\`, `<`, `>`, `:`, `"`, `|`, `?`, or `*`.
- Two folders in one place can't have the same name, ignoring case.

Rules for moving:

- A folder can't move into itself or a folder under it.
- A note moved into a folder that already has a file of that name gets `-2` on its file name, as a new note with a taken name does today. The name you see doesn't change.
- A folder moved to where a folder of that name exists is refused, with a message.
- A note's tags and open tab survive a move, since its id in the index doesn't change.
- A note's attachments move with it, to the new folder's `_attachments/`. An empty `_attachments/` left behind is removed.

A note's file name is still a slug of its name (`kickoff-notes.md` for "Kickoff Notes"), and renaming a note renames its file within its folder.

### Attachments

- A note's attachments are in `_attachments/` inside the note's own folder. A note at the top of the project uses `_attachments/` at the top, as today.
- The link in the note is `./_attachments/att-1234.png` in every folder, so it never changes when a note moves, and other Markdown programs find the file.
- The index entry lists the attachment's stored name, as today. The folder comes from the note's path.
- Adding an attachment writes it beside the note. The preview, PDF export, and presentations read it from there.
- Moving a note moves its attachments. Moving or renaming a folder carries its `_attachments/` with it, since it is inside.
- Deleting a note sends its attachments to the trash with it.
- A stored name is unique across the project, so two folders never hold different files with one name.

### Frontmatter in the editor

The editor shows a note's file as it is on disk: the `---` block at the top, then the text. What you type in the block is saved as you typed it.

| Field | When you change it in the editor | When it changes elsewhere |
|---|---|---|
| `tags` | The note's tags change, in the Tags pane and the index. | Adding or removing a tag in the app rewrites the line in the editor. |
| `created` | NoteLiner puts it back when it saves. | It doesn't change. |
| `presentation:` | The deck's settings change, as if set in Presentation Settings. | Presentation Settings rewrites the block in the editor. |
| Anything else | Kept as you typed it. NoteLiner doesn't read it. This covers `id`, `name`, and `updated` lines left in a note by NoteLiner 2, which you can remove by hand. | |

- The block NoteLiner writes holds `tags` and `created`. A save changes nothing in a block that has both, so the text doesn't change under you as you type.
- A note's name is not in its file. Renaming a note in the tree changes the index and the file's name, and nothing in the block.
- Only the frontmatter is shown in a new way. The preview, exports, presentations, the word count, the spell check, search, and links all still leave it out.
- A search result or backlink that opens a note still lands on the right line, counting the frontmatter's lines.
- The block is drawn in a dimmer colour than the text, so it reads as the file's details and not as the note. It always shows, with no setting or fold to hide it.
- A change NoteLiner makes to the block in the editor is not an undo step, and doesn't mark the note as edited.
- While the block can't be read as YAML (a half-typed line), the file is saved as typed, the note's tags and deck settings keep their last good values, and the status bar says "Frontmatter can't be read". NoteLiner doesn't rewrite a block it can't read.
- A block you delete comes back on the next save, with the note's tags and created date. A deleted `tags` or `created` line comes back the same way. To remove a note's tags, empty the list or use the Tags pane.
- A note with no frontmatter shows none. The block appears at the top the first time you edit the note, and the view holds its place on the text.
- With Write YAML frontmatter to .md files turned off, NoteLiner adds and changes nothing in the block. It shows and saves what is there.
- A diagram has no frontmatter, and its editor doesn't change.
- The MCP tools `read_note` and `update_note` still read and write the text without the frontmatter. An AI assistant uses `set_tags` and `rename_note` for the rest.

### Older projects

NoteLiner doesn't convert a project that has notes under notes. The one project that had them, `~/noteliner-docs`, is already in the new layout: each parent note is `<Folder>/index.md`, the folder has the note's name, the children are beside it, and the index is at version 4 with no `parentId`.

An index at version 3 or lower still opens. The walk reads the files where they are, `parentId` and `order` are dropped, and the version becomes 4. Notes that were under other notes show at the top, since their files are there.

A note called `index.md` is an ordinary note. It sorts with the other notes of its folder, and it keeps the name it had ("Meeting" in the folder Meeting).

### Saving

Every place that committed now only writes the files. There is no push timer, and closing a project or quitting has nothing to wait for.

### What is removed from the app

| Removed | Notes |
|---|---|
| Remote Sync dialog and the Remote Sync command | `overlays/sync.rs`, `Act::Sync`, `project.sync` |
| The branch chip in the status bar | The MCP chip and the note's details stay |
| History pane, Toggle History, and Ctrl+H | `views/history.rs`, `Act::ToggleHistory`, the History button in the note and diagram toolbars |
| Git identity in New Project and Project Settings | Project Settings keeps the project folder. If nothing else is left in it, it is removed, and its folder line moves to Settings. |
| New Project and Clone from Remote | With the setup dialog. Open Folder does both jobs. |
| "Syncing changes to remote" | The Busy overlay stays for imports |
| The "needs a Git identity" path | `Outcome::GitConfigRequired`, `Event::GitConfigRequired`, `needsGitConfig` |
| A note as the parent of a note | The "child" drop zone on a note, and the note list in Move… |

### What stays, renamed

- The Sync Log panel is renamed Activity Log, for MCP activity and import notes. Its empty text becomes "No activity yet."
- The MCP descriptions that say a tool "auto-commits to git" lose that sentence, and the consent dialog's "you see every operation in the Sync Log" names the Activity Log.

### MCP

| Tool | Change |
|---|---|
| `list_notes` | Returns each note's `folder` (its path from the project, empty at the top) in place of `parentId`. |
| `create_note` | Takes `folder` in place of `parentId`. A folder that doesn't exist is created. |
| `delete_note` | Sends the file to the trash. The line about re-parenting children is removed. |
| `list_folders` (new) | Every folder, with its path and the number of notes in it. |
| `create_folder` (new) | Makes a folder at a path. |
| `move_note` (new) | Moves a note to a folder. |
| `rename_folder`, `delete_folder` (new) | As in the app. `delete_folder` refuses a folder that isn't empty. |
| The project index resource | Its description names `filename` as a path and drops `parentId`. |

Every path a tool takes is checked the way `Project::file_path` checks one today: it can't be absolute or hold `..`, and it can't name a skipped folder.

### Existing projects and Git

- A project that is a Git repository opens as any folder does. NoteLiner doesn't read, change, or delete `.git`, and it shows no notice about it.
- Changes you make in NoteLiner show as uncommitted changes in that repository. You commit and push them yourself if you want to keep using Git.

### Deleted and overwritten notes

With Git gone there is no way back once the editor's undo is lost. Deleted notes, folders, and attachments go to the system's trash in place of being deleted, as a file manager does. A history of a note's earlier versions is not part of this plan.

### Changes made outside NoteLiner

With the folders as the tree, a file manager, a sync tool, or Git used by hand can add, move, and remove notes while NoteLiner has the project open. NoteLiner checks while its window has the focus, every few seconds, and when the window regains the focus.

| Outside change | NoteLiner does |
|---|---|
| A note or folder added | It appears in the tree. |
| A note or folder removed | It leaves the tree. An open tab for a removed note with no unsaved edits closes. One with unsaved edits stays open, marked as deleted, and saving writes the file again. |
| A note or diagram moved to another folder | It is matched by its file name, when exactly one file of that name disappeared, and it keeps its id, name, tags, and tab. If a note's attachments weren't moved with it, NoteLiner moves them to the new folder's `_attachments/`. |
| A note or diagram renamed | It is matched when exactly one file disappeared and one of the same kind and size appeared in the same check. It keeps its id, tags, and tab, and takes its name from the new file name. Otherwise it is a removed file and a new one: a note keeps the tags and created date in its frontmatter and loses its attachment list, and a diagram loses its tags. |
| A folder moved or renamed | Every entry under it gets its new path, matched file by file as above. |
| An open note changed, with no unsaved edits | The note reloads. |
| An open note changed, with unsaved edits | Your edits stay, and a notice offers Reload and Keep Mine. |
| `noteliner.json` changed | The index is read again and brought in line with the folder. |

The check compares each folder's modification time with the last walk, and reads a folder again only when it changed, so a quiet project costs one `stat` per folder.

## Changes

### noteliner-core

| File | Change |
|---|---|
| `git.rs` | Deleted. |
| `lib.rs` | The `git` module is removed, the new `tree` module is added, and the header comment no longer names the Tauri build. |
| `frontmatter.rs` | The fields NoteLiner writes are `tags` and `created`. `mirror_from_entry` no longer sets `id`, `name`, or `updated`, and it leaves lines of those names as it finds them. |
| `tree.rs` (new) | The walk: the folders and files under a project, with the skip rules and the limit. `reconcile(index, walk)`, which returns the new index and what was added, moved, and removed. The folder-name rules. |
| `project.rs` | `Project::new()` takes no `NoteGit`. `open` walks and reconciles for any folder. `init`, `check_git_config`, `get_git_config`, `set_git_config`, and `commit_and_schedule` are removed, with their call sites. `create` takes a folder in place of a parent id. `unique_filename` checks one folder. `rename_file` stays in the note's folder and no longer rewrites the note's frontmatter. `reconcile_frontmatter` no longer rewrites a note when a project opens. `read_file` returns the whole file. `write_file` takes the whole file, parses its frontmatter, takes the tags and deck settings from it, puts back a missing `tags` or `created`, and returns the block if it changed it. `read_body` (new) is the text without frontmatter, for search, export, links, and MCP. `order` is no longer written. `delete_file` and `remove_attachment` use the trash. `attachment_path` and `add_attachment` take the note, and use `_attachments/` in its folder. `move_file` moves the note's attachments. New: `folders`, `create_folder`, `rename_folder`, `move_folder`, `delete_folder`, `move_file`, and `rescan`. `migrate_index` gains the step to version 4, which drops `parentId`. The header comment says a project is a folder. |
| `links.rs` | Unchanged in behaviour. Links still find a note by name anywhere in the project. |
| `templates.rs`, `import/mod.rs` | The commits and push scheduling are removed. An import takes the folder to put the note in. |
| `mcp.rs` | The tools and descriptions above. |
| `Cargo.toml` | The `trash` crate. |
| Tests | The fixtures stop running `git init` and setting an identity. The test that reads a file's Git log after a rename checks the file on disk. New tests are listed below. |

The `Project` methods stay `async`, since the MCP server and the backend share the workspace behind an async lock. Making them plain functions is a separate tidy-up.

### noteliner-native

| File | Change |
|---|---|
| `backend.rs` | Removed: `Op`, `git_op`, `git_state`, `git_config`, `set_git_config`, `init_project`, `history`, `history_content`, the push flush in the two close paths, `Outcome::GitConfigRequired`, and the events `GitState`, `GitDone`, `GitConfig`, `GitConfigRequired`, `History`, and `HistoryContent`. Added: the folder operations and the outside-change check. |
| `model.rs` | `Entry` loses `parent_id` and `order`, and gains `folder`, worked out from `filename`. A `Node` is a folder or an entry. `SortMode::User` is removed. `flat_list`, `children`, and `move_to` work on folders. `apply_drop` only moves into a folder, and `DropPos` is removed. `parent_options` becomes the list of folders. `subtree` is the entries under a folder. |
| `views/tree.rs` | Folder rows, with their own right-click menu (New File, New Diagram, New Folder, Rename, Move…, Open in File System, Delete). The only drop zone is "into" a folder, and the before and after zones on a note are removed. Inline rename for a folder. A drop on the empty part of the tree. |
| `views/sidebar.rs`, `views/panes.rs` | The New Folder button in the Files pane's header. |
| `overlays/mod.rs` | The setup dialog, New Project, and the Git section of Project Settings are removed. New Folder's name dialog, the folder delete confirmation, and Move… listing folders. |
| `overlays/sync.rs`, `views/history.rs` | Deleted. |
| `views/mod.rs` | The branch chip, `sync_label`, and `sync_dot` are removed. The log panel's title and empty text. |
| `views/diagram.rs`, the note toolbar | The History button is removed. |
| `commands.rs` | `project.sync`, `view.toggleHistory`, and New Project are removed. `file.newFolder` is added. Ctrl+H is free. Delete File and Rename act on a selected folder. |
| `app.rs` | The sync state, the History pane's state, the setup flow, and the handlers for the removed events are removed. The selection can be a folder. The note's parsed frontmatter comes from the editor's text, not from a separate read. Tag changes and Presentation Settings rewrite the block in the open editor. A search result's line is offset by the block. The open folders are saved with the project's view state. The reloads on an outside change. |
| `config.rs` | The open folders in the project's view state, by path. `filesSortMode` no longer has `user`, and its default is `name-asc`. |
| `editor.rs`, `diagram.rs` | The note editor holds the whole file. It finds the frontmatter block's range for the dimmer colour, the spell check, and the word count, and it has `set_frontmatter`, which replaces the block without an undo step. The reload path stays and is used by the outside-change check. The comments that name `git:resetToRemote` and a history restore are updated. |
| `views/preview.rs`, `deck.rs`, `spell.rs` | The preview, the slides, and the spell check take the text after the block. |
| `examples/noteliner_demo_data.rs` | Writes the demo project with folders and without a repository. |
| `assets/help-pages.json` | The pages on sync, history, and setting up a repository are removed. A page on folders is added. |
| UI tests | The fixtures lose `NoteGit`. Tests of the Sync dialog, the History pane, the identity prompt, and notes under notes are removed or rewritten. New tests are listed below. |

### Packaging

| File | Change |
|---|---|
| `apps/noteliner/native/Cargo.toml` | `git` leaves the `.deb` and `.rpm` dependencies. The summaries and descriptions lose "with git sync". |
| `packaging/flatpak/com.nathanlaan.noteliner.yml` | The Git module and the SSH agent socket are removed. Network access stays for the update check. The trash needs checking in the sandbox. See Risks. |
| `packaging/linux/com.nathanlaan.noteliner.metainfo.xml` | The summary, the description, and a release entry. |

### Docs

`apps/noteliner/README.md` (the features list, "What a project is", folders, and a short section on using Git yourself), `apps/noteliner/native/README.md`, `ARCHITECTURE.md` (which apps use `laankit-core::git`, and the backend's description), `SECURITY.md` (NoteLiner leaves the Git lines, and its data is a folder), and `docs/packaging.md` (the dependency and Flatpak notes).

The plans in `apps/noteliner/docs/plans/` that build on Git (`plans-remote-pages-deploy.md` in particular) need a look after this is decided. They are not changed here.

## Phases

| Phase | Work | Done when |
|---|---|---|
| P1 | The core without Git: `Project` with no `NoteGit`, no commits, and the trash | `noteliner-core` has no `git` module, and its tests pass without running `git` |
| P2 | Folders in the core: the walk, the reconcile, paths in `filename`, the folder operations, the step to version 4, and the MCP tools | A folder with sub-folders opens with every note in its place, and the core tests pass |
| P3 | The app without Git: the backend, the removed dialogs, panes, chip, and commands, and the renamed log | NoteLiner opens a plain folder, edits and saves, and no `git` process is started |
| P4 | Folders in the app: the tree, New Folder, rename, move, drag, delete, and the UI tests | Every action in the Folders table works, and the files on disk match the tree after each |
| P5 | Frontmatter in the editor: the whole file in the editor, the save path, the rewrites from the app, the dimmer colour, and the tests | A note opens with its block, a tag typed there shows in the Tags pane, and the preview and exports are unchanged |
| P6 | Outside changes: the check, the reloads, and the notice | A note or folder added, moved, or removed by another program shows in the tree, and unsaved edits are never lost without asking |
| P7 | Packaging, help pages, docs, and the version | The packages no longer depend on Git, and the docs describe a folder of notes |

P3 to P6 ship together. Without P6, a project changed elsewhere while it is open can lose those changes.

## Tests

| Layer | Covers |
|---|---|
| Core, the walk | Sub-folders at several depths, empty folders, skipped names, a symbolic link to a folder, the file limit, and paths written with `/` on every system. |
| Core, the reconcile | A listed file, a new file, a removed file, a note and a diagram moved outside NoteLiner and found by their file names, two gone files with one name, which are not matched, a note renamed outside, which comes back with its tags and the file's name, and a folder with no index. Opening changes no file but `noteliner.json`. |
| Core, attachments | An attachment added to a note in a folder lands in that folder's `_attachments/`. Moving the note moves it, and the link still resolves. Deleting the note trashes it. |
| Core, frontmatter | Saving a whole file: tags typed in the block reach the index, a block with `tags` and `created` is saved byte for byte, a removed `tags` or `created` line is put back, `id`, `name`, and `updated` are never written, lines of those names already in a file survive with the other fields, a rename changes nothing in the file, a block that isn't YAML is saved as typed and changes nothing in the index, and a deleted block comes back. With the setting off, nothing is added. `read_body` leaves the block out. |
| Core, folders | Create, rename, move, and delete a folder, with the entries under it following. The name rules. A move into itself is refused. A file name already taken in the target folder gets a suffix. A deleted folder's notes leave the index. |
| Core, older indexes | A version 3 index with `parentId` opens, every note is listed at its file's place, `parentId` is gone, and a second open changes nothing. |
| Core, without Git | Every write (create, save, rename, move, delete, tags, attachments, templates, import) leaves the files and index right with no repository present. A folder that is a repository opens, and its `.git` is byte-for-byte unchanged afterwards. |
| Core, MCP | `create_note` with a folder, `move_note`, the folder tools, and a path with `..` refused. |
| App | No `git` binary on the `PATH`: the app opens a project, edits, and closes. |
| UI (`egui_kittest`) | Opening a plain folder shows its sub-folders with no setup dialog. New Folder from the header, the menu, and the palette. A new note lands in the selected folder. Dragging a note onto a folder moves its file. Renaming a folder keeps its notes' tabs open. Deleting a folder asks, then sends it to the trash (recorded in tests, as opening a folder is). The status bar has no branch chip. The palette has no Remote Sync or Toggle History. A note changed on disk reloads, and one with unsaved edits shows the notice. A folder added on disk appears. A note opens with its frontmatter in the editor. Adding a tag in the Tags pane changes the line in the editor without marking the note edited. A tag typed in the editor shows in the Tags pane. The word count and the preview leave the block out. A search result opens on the right line. The sort list has no User Order. |
| By hand | `~/noteliner-docs` opens with its seven folders, and the attachments in `Meeting/` show. The `.deb` installs on a machine without Git. The Flatpak builds without the Git module, and delete reaches the trash from inside it. A folder of a few thousand notes opens quickly. |

## Risks

| Risk | Handling |
|---|---|
| NoteLiner 2.9.0 opens a project that has folders | It reads the files, but shows every note in one flat list, and renaming a note there moves its file back to the top of the project. It also writes `id`, `name`, and `updated` back into every note when it opens the project. Structure changes wait for 3.0.0. |
| Losing the safety net of a commit on every change | The trash, and the README says plainly that NoteLiner keeps no history. |
| Someone relies on the automatic push as their backup | The release note. No notice is shown in the app (Q6). |
| Outside changes overwritten | P6 ships with P3, P4, and P5. |
| A note or diagram renamed outside NoteLiner isn't matched | No file carries an id. A move is matched by file name, and a rename by the one-for-one match in a single check. A note that isn't matched keeps its tags, which are in its file, and takes its name from the file. A diagram loses its tags. |
| A note's name is only in the index | A project whose `noteliner.json` is lost, and a note copied in from another project, show the file's slug as the name (`sdlc-alm` for "SDLC / ALM"). You rename the note to fix it. |
| Two notes with the same name in different folders | A `[[Name]]` link reaches only one of them, as it does today for two notes with one name. The link picker and search results show the folder. A link that names its folder is out of scope. |
| A note is moved outside NoteLiner without its attachments | NoteLiner moves them when it sees the note in its new place. Until then the links in that note are broken. |
| A very large folder is opened by mistake | The 20,000 file limit, with a message. |
| A sub-folder that has its own `noteliner.json` | It is shown as a plain folder, and its index is not read. Opening that sub-folder on its own still works. |
| A folder renamed while a note inside has a save waiting | A rename or move writes waiting edits first, as switching tabs does. |
| The trash isn't available (a network drive, some Flatpak setups) | Delete then asks "Delete permanently?" before removing anything. |
| File names that differ only by case | The name rules compare without case, so a project made on Linux still works on Windows and macOS. |
| The editor's text changes under you when NoteLiner rewrites the block | A save doesn't change a block that has `tags` and `created`. The block is rewritten only for a tag change or Presentation Settings in the app, only its lines change, and a rewrite is never an undo step. |
| A typing mistake in the block loses tags | A block that can't be read changes nothing in the index, and the status bar says so. |
| A project that relied on User Order | The order is gone after the first open in 3.0.0. `~/noteliner-docs` sorts by name. |
| Hidden uses of Git | The check in P3 is that the app runs with no `git` on the `PATH`. |

## Out of scope

- A history of a note's earlier versions that doesn't use Git.
- Sync of any kind.
- Removing Git from PageLiner and ThreadLiner, or `laankit-core::git` itself.
- Dropping `noteliner.json` so that the files and folders alone are the project.
- A link that names a note's folder, such as `[[work/Kickoff]]`.
- Showing files other than notes and diagrams in the tree.
- Moving NoteLiner's tree onto the shared `laankit_egui::tree` widget.
- A folder tree in BlockLiner, whose Project pane lists only the diagrams at the top of its folder.
- Making the `Project` methods synchronous.
