# NoteLiner

NoteLiner is a single-user, outliner-style note-taking app: a folder of Markdown notes and diagrams, in sub-folders, with syntax highlighting, live preview, tags, wikilinks, backlinks, slides, and PDF export. A project is a plain folder that you open and edit.

It is a pure-Rust desktop app built on [egui](https://github.com/emilk/egui) over the shared `laankit-*` crates, with no webview and no JavaScript. The shipped binary is `noteliner`.

> BETA software. Written for personal use; it meets specific needs and may not meet yours.

- Run, build, install: see the [root README](../../README.md).
- The native app in depth (running, diagnostics, architecture, feature parity, editor, slides, spell check, PDF export): see [`native/README.md`](native/README.md).
- How it fits together: [`ARCHITECTURE.md`](../../ARCHITECTURE.md).

## Features

- Folders: the folders on disk are the tree. You can add, rename, move, and delete folders, and drag notes between them
- Changes made by other programs (a file manager, a sync tool, Git used by hand) show up while the project is open
- Tabs: each file you open gets a tab above the document, with its own undo history, cursor, and scroll. Undo and Redo on the left toolbar act on the active tab, and are off when it has nothing to undo. Ctrl+W closes a tab, Ctrl+Shift+W reopens the tab you closed last with its history, and Ctrl+Tab and Ctrl+Shift+Tab switch tabs. The tabs come back when you reopen the project
- Diagrams: BlockLiner's diagram editor inside a project. A diagram is a `.mmd` file beside your notes, with BlockLiner's tools in the toolbar above it and a Properties panel to its right. Shapes can be moved in front of or behind one another (Move to Front, Move Up, Move Down, Move to Back). It can be tagged, linked with `[[wikilinks]]`, moved to another folder, and exported as PNG, SVG, or Mermaid
- Markdown editor with syntax highlighting, list editing, multiple cursors, folding, find/replace, and `[[wikilink]]` completion. It shows a note's whole file, with its frontmatter at the top
- Live Markdown preview with math (`$…$`, `$$…$$`) typeset by Typst
- Tags, backlinks, and full-text search panes
- File attachments (paste, drag-and-drop, or file picker) with image thumbnails
- Reusable note templates with `{{title}}`/`{{date}}` placeholder substitution
- Presentations: author slides inside a note and present full screen, with a speaker view. A presentation has a slide theme for each NoteLiner theme, in the same colours. Help (F1, or the Help button on the left toolbar) is a window you can keep open beside your work, and its Presentation Formatting page lists what you can write on a slide
- PDF and HTML export (PDF rendered in-process with Typst)
- Deleted notes, folders, and attachments go to the system's trash
- Optional MCP server to expose the open project to AI assistants over a local-only socket (see below)
- Built-in themes and keyboard shortcuts for every action

## What a project is

A project is a folder, with everything under it:

| On disk | In NoteLiner |
|---|---|
| A sub-folder, at any depth | A folder in the tree |
| A `.md` file | A note |
| A `.mmd` or `.mermaid` file | A diagram: a Mermaid flowchart in BlockLiner's format, so the same file opens in BlockLiner |
| `noteliner.json` at the top | The index. Not shown. |
| `_attachments/` in any folder | The attachments of the notes in that folder (images, PDFs, and other files; 30 MB each). Not shown in the tree. |
| `_templates/` at the top | Reusable note skeletons (plain `.md` files). Not shown in the tree. |
| Any folder or file whose name starts with `.` or `_` | Skipped, with everything in it. This covers `.git`. |
| Any other file (a PDF, an image) | Left alone and not shown |
| A symbolic link to a folder | Not followed |

Any folder opens. Opening writes `noteliner.json` and nothing else. A new project is an empty folder you open.

The folders say what exists and where. `noteliner.json` holds what the files can't:

| In the index | Why |
|---|---|
| An id for each note and diagram | Tabs, links, and the MCP tools use it, so a rename or a move doesn't change it. No file carries it. |
| Each note's name | A file's name is a slug (`sdlc-alm.md` for "SDLC / ALM"), so the name you typed is kept here. |
| A diagram's tags | Nothing but the diagram is written into its file. |
| A note's attachments | The list of files attached to it. |
| The tag colours | They belong to the project. |

Settings live in the platform config folder (`~/.config/NoteLiner` on Linux).

## Folders

- In every folder, its folders come first, in name order. Its notes and diagrams follow, sorted by Name A-Z, Name Z-A, Last Modified, or Created.
- New Item (Ctrl+N) makes a note, a presentation, a diagram, or a folder in one dialog. New Folder in the Files pane's header, in the right-click menu of a folder or of the empty part of the tree, and in the Command Palette opens it with Folder chosen. The dialog starts with the selected folder, or the folder of the selected note, or the top with nothing selected, and you can pick another. With Presentation chosen you also pick the slide theme, which starts as the NoteLiner theme you are using.
- Drag a note or a folder onto a folder to move it, or onto the empty part of the tree to move it to the top. Move... in the right-click menu lists the folders.
- A note's file name is a slug of its name (`kickoff-notes.md` for "Kickoff Notes"). A folder's name is its name on disk.
- A note's attachments are in `_attachments/` inside the note's own folder, and they move with the note. The link in the note is `./_attachments/att-1234.png` in every folder.
- Delete sends a note, a folder with everything in it, or an attachment to the system's trash. If the trash can't take it, NoteLiner asks before deleting it for good.

A folder's name can't be empty, be `.` or `..`, start with `.` or `_`, or hold `/`, `\`, `<`, `>`, `:`, `"`, `|`, `?`, or `*`. Two folders in one place can't have the same name, ignoring case.

## Frontmatter

A note's file starts with a YAML block that holds its tags and the date it was created:

```markdown
---
tags:
  - work
created: '2026-11-15T19:31:33.906Z'
---
# Meeting Notes
```

The editor shows the block at the top of the note, in a dimmer colour, and you can edit it there.

| Field | When you change it in the editor | When it changes elsewhere |
|---|---|---|
| `tags` | The note's tags change, in the Tags pane and the index. | Adding or removing a tag in the app rewrites the line in the editor. |
| `created` | NoteLiner puts it back when it saves, if it is removed. | It doesn't change. |
| `presentation:` | The deck's settings change. | Presentation Settings rewrites the block in the editor. |
| Anything else | Kept as you typed it. NoteLiner doesn't read it. | |

- A block with `tags` and `created` is saved as typed. A block you delete comes back on the next save.
- The preview, exports, presentations, the word count, the spell check, search, and links all leave the block out.
- While the block can't be read as YAML (a half-typed line), the file is saved as typed, the note's tags keep their last good values, and the status bar says "Frontmatter can't be read".
- A note with no frontmatter gets it the first time you edit the note, not when the folder is opened.
- A note's name is not in its file. You rename a note in the tree.
- With Write YAML frontmatter to .md files turned off in Settings, NoteLiner adds and changes nothing in the block.
- `id`, `name`, and `updated` lines written by NoteLiner 2 are kept as any other field. You can remove them by hand.

## Changes made outside NoteLiner

A file manager, a sync tool, or Git used by hand can add, move, and remove notes while NoteLiner has the project open. NoteLiner checks while its window has the focus, every few seconds, and when the window regains the focus.

| Outside change | NoteLiner does |
|---|---|
| A note or folder added | It appears in the tree. A new note takes its name from its file name. |
| A note or folder removed | It leaves the tree, and its tab closes. A removed note with unsaved edits is written again. |
| A note or diagram moved to another folder | It keeps its name, tags, and tab. Its attachments are moved after it if they were left behind. |
| A note or diagram renamed | It keeps its tags and tab, and takes its name from the new file name. |
| An open note changed, with no unsaved edits | The note reloads, and a toast says so. |
| An open note changed, with unsaved edits | Your edits stay, and a dialog offers Reload and Keep Mine. A note in a background tab asks when you return to its tab. |

A save looks at the note's file first. If another program changed it since NoteLiner last read or wrote it, nothing is written and the same dialog asks. A file that was written again with the same content is not a change. If you close the tab before answering, your edits are written and the Log pane says so. A diagram does not ask: your version is kept, with a line in the Log pane.

A moved file is matched by its file name, and a renamed one by its kind and size, when exactly one file disappeared and one appeared. A file that can't be matched is a removed file and a new one: a note keeps the tags in its frontmatter, and a diagram loses its tags.

## Using Git yourself

NoteLiner runs no `git` command, keeps no history of a note's earlier versions, and does not sync.

- A project that is a Git repository opens as any folder does. NoteLiner doesn't read, change, or delete `.git`.
- Changes you make in NoteLiner show as uncommitted changes in that repository. You commit and push them yourself.
- A pull, a checkout, or a merge while the project is open is picked up like any other change made outside NoteLiner.

## Templates

Templates are reusable note skeletons stored as plain Markdown in the project's `_templates/` directory. They are not notes, so they never appear in the tree, search, or backlinks.

When a project has at least one template, the New Item dialog shows a *Template* dropdown. Save as Template (Command Palette) saves the selected note's name and body as `_templates/<name>.md`. These placeholders are substituted when a note is created from a template (any other `{{…}}` is left untouched):

| Placeholder  | Replaced with                 | Example              |
|--------------|-------------------------------|----------------------|
| `{{title}}`  | The new note's name           | `Meeting Notes`      |
| `{{date}}`   | Current date (`YYYY-MM-DD`)   | `2026-05-31`         |
| `{{time}}`   | Current time (`HH:MM`, 24h)   | `14:30`              |
| `{{datetime}}` | Current date and time       | `2026-05-31 14:30`   |

## MCP server

NoteLiner can expose the currently-open project to AI assistants (Claude Code, Claude Desktop, Cursor, …) over the [Model Context Protocol](https://modelcontextprotocol.io). It is off by default and local-only: no TCP port is opened. NoteLiner listens on a user-scoped Unix domain socket, and clients connect through the built-in bridge (`noteliner --mcp-bridge`).

Enable it in Settings → MCP; the first time you do, a one-screen walkthrough explains the setup and offers a copy-paste client snippet (always use the path it shows, since it differs between dev and packaged installs). The server runs only while a project is open.

#### Tools

Reads: `list_notes`, `list_folders`, `read_note`, `search`, `get_backlinks`, `list_attachments`, `list_tags`; writes: `create_note`, `update_note`, `delete_note`, `rename_note`, `move_note`, `set_tags`, `add_attachment`, `remove_attachment`, `create_folder`, `rename_folder`, `delete_folder`. It also exposes `noteliner://` resource URIs and a handful of prompts (`daily_note`, `meeting_note`, `summarize_note`, `link_suggestions`).

The same tools work on diagrams. `list_notes` gives each entry's `kind` (`note`, `deck`, or `diagram`), `create_note` takes `kind: "diagram"`, and `read_note` and `update_note` read and write a diagram's Mermaid text. `update_note` refuses text that BlockLiner can't read, so a diagram always opens.

| Tool | Folders |
|---|---|
| `list_notes` | Gives each note's `folder`: its path from the project folder, empty at the top. |
| `create_note` | Takes `folder`. A folder that doesn't exist is created. |
| `move_note` | Moves a note or diagram to a folder, with its attachments. |
| `list_folders` | Every folder, with its path and the number of notes in it. |
| `create_folder`, `rename_folder` | As in the app. |
| `delete_folder` | Deletes an empty folder. A folder with anything in it is refused. |
| `delete_note` | Sends the file and its attachments to the trash. |

`read_note` and `update_note` read and write a note's text without its frontmatter. Every folder path is checked: it can't be absolute, hold `..`, or name a folder NoteLiner skips.

#### Safety

Optional confirm-before-write (*Allow once* / *for the session* / *Deny*), per-tool allow/deny, single-project scope, a `chmod 0600` socket, and a 10 MB body cap on writes. Every MCP call is logged to the Activity Log, prefixed `[MCP]`. The server needs a Unix socket, so it is not yet available on Windows. See [`native/README.md`](native/README.md) for the bridge details.

## Plans

`docs/plans/` holds plans for features not built yet (or only partly built). Completed plans are removed; they remain in git history.
