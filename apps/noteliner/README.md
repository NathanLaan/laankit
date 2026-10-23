# NoteLiner

NoteLiner is a single-user, outliner-style note-taking app: a hierarchical tree
of Markdown notes with syntax highlighting, live preview, tags, wikilinks,
backlinks, slides and PDF export, where every change is committed to git and
pushed to an optional remote for cross-device sync.

It is a pure-Rust desktop app built on [egui](https://github.com/emilk/egui) over
the shared `marina-*` crates — no webview, no JavaScript. The shipped binary is
`noteliner`.

> **BETA software.** Written for personal use; it meets specific needs and may
> not meet yours.

- **Run, build, install:** see the [root README](../../README.md).
- **The native app in depth** (running, diagnostics, architecture, feature
  parity, editor, slides, spell check, PDF export): see
  [`native/README.md`](native/README.md).
- **How it fits together:** [`ARCHITECTURE.md`](../../ARCHITECTURE.md).

## Features

- Hierarchical note tree with drag-and-drop reordering and re-parenting
- Markdown editor with syntax highlighting, list editing, multiple cursors,
  folding, find/replace and `[[wikilink]]` completion
- Live Markdown preview with math (`$…$`, `$$…$$`) typeset by Typst
- Tags, backlinks and full-text search panes
- File attachments (paste, drag-and-drop or file picker) with image thumbnails
- Reusable note templates with `{{title}}`/`{{date}}` placeholder substitution
- Presentations: author slides inside a note and present full screen, with a
  speaker view
- PDF and HTML export (PDF rendered in-process with Typst)
- Automatic git commit on every change with debounced push to a remote
- Optional MCP server — expose the open project to AI assistants over a
  local-only socket (see below)
- Built-in themes and keyboard shortcuts for every action

## Data model

A **Project** is a folder on disk containing:

1. A local git repository, optionally linked to a remote on GitHub or GitLab.
2. `noteliner.json` — the index: every note, its place in the hierarchy, tags
   and attachments.
3. Markdown files (`.md`) in the project root.
4. `_attachments/` — files attached to notes (images, PDFs, …; 30 MB cap).
5. `_templates/` (optional) — reusable note skeletons (plain `.md` files).

Changes are committed locally whenever a note is added, removed or modified, and
pushed to the remote on a debounce timer. Settings and data live in the
platform config/data folders (`~/.config/NoteLiner` on Linux), shared across
every build Marina has ever shipped, so projects open unchanged.

## Templates

Templates are reusable note skeletons stored as plain Markdown in the project's
`_templates/` directory. They are **not** notes — they never appear in the tree,
search or backlinks — but they are committed to git alongside the project.

When a project has at least one template, the **New File** dialog shows a
*Template* dropdown. **Save as Template** (Command Palette) saves the selected
note's name and body as `_templates/<name>.md`. These placeholders are
substituted when a note is created from a template (any other `{{…}}` is left
untouched):

| Placeholder  | Replaced with                 | Example              |
|--------------|-------------------------------|----------------------|
| `{{title}}`  | The new note's name           | `Meeting Notes`      |
| `{{date}}`   | Current date (`YYYY-MM-DD`)   | `2026-05-31`         |
| `{{time}}`   | Current time (`HH:MM`, 24h)   | `14:30`              |
| `{{datetime}}` | Current date and time       | `2026-05-31 14:30`   |

## MCP server

NoteLiner can expose the currently-open project to AI assistants (Claude Code,
Claude Desktop, Cursor, …) over the
[Model Context Protocol](https://modelcontextprotocol.io). It is **off by
default** and **local-only**: no TCP port is opened. NoteLiner listens on a
user-scoped Unix domain socket, and clients connect through the built-in bridge
(`noteliner --mcp-bridge`).

Enable it in **Settings → MCP**; the first time you do, a one-screen walkthrough
explains the setup and offers a copy-paste client snippet (always use the path
it shows — it differs between dev and packaged installs). The server runs only
while a project is open.

**Tools** — reads: `list_notes`, `read_note`, `search`, `get_backlinks`,
`list_attachments`, `list_tags`; writes: `create_note`, `update_note`,
`delete_note`, `rename_note`, `set_tags`, `add_attachment`, `remove_attachment`.
It also exposes `noteliner://` resource URIs and a handful of prompts
(`daily_note`, `meeting_note`, `summarize_note`, `link_suggestions`).

**Safety** — optional confirm-before-write (*Allow once* / *for the session* /
*Deny*), per-tool allow/deny, single-project scope, a `chmod 0600` socket, and a
10 MB body cap on writes. Every MCP call is logged to the Log panel, prefixed
`[MCP]`. The server needs a Unix socket, so it is not yet available on Windows.
See [`native/README.md`](native/README.md) for the bridge details.

## Historical plans

`docs/plans/` holds the planning and design notes written during development,
including the completed phases under `docs/plans/completed/`. They are kept as a
record and are not a guide to the current build.
