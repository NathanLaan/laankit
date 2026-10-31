# Status Bar

Status: Draft for review

## Overview

Add a thin, full-width status bar pinned to the bottom of the NoteLiner window.
It surfaces at-a-glance, mostly-passive information about the current document,
the editor caret, and the project/git state — the kind of context that is
otherwise scattered across the toolbar, the Sync modal, and nowhere at all
(word count, caret column, save state). A few segments double as click targets
into existing UI (e.g. the sync chip opens the Sync modal).

Visibility is a persisted layout toggle, matching the existing
`showLog` / `showOutline` / etc. pattern, with a Toolbar button and a command in
the palette.

## Suggested features

Grouped into three zones (left / center / right). Each segment notes its data
source and whether it is **Recommended** for the first cut or **Optional**
(phase 2 / behind discussion).

### Left zone — document & project context

1. **Current file name** *(Recommended)* — `projectState.selectedFile.name`.
   Cheap, reactive, and orients the user when the sidebar is collapsed.
2. **Save status** *(Recommended)* — `Saved` / `Saving…` / `Unsaved`. NoteLiner
   autosaves on a 500 ms debounce (`Editor.svelte` `scheduleSave`), but there is
   currently **no** visible confirmation that a write happened. This is the
   single most valuable addition for user trust. Requires a small new
   `projectState.saveStatus` field (see Implementation).
3. **Note count** *(Optional)* — total notes in the project,
   `projectState.index.files.length`. Nice-to-have; low cost.

### Center zone — selection / messages

4. **Selection summary** *(Optional)* — when a non-empty selection exists,
   `N selected` (chars, optionally words). Available from the CodeMirror
   selection in `Editor.svelte`. Hidden when the selection is empty.

### Right zone — editor & repo state

5. **Caret position** *(Recommended)* — `Ln X, Col Y`. `cursorLine` is already
   tracked in `projectState`; column is not yet but is a one-line addition in
   the editor's `updateListener`.
6. **Word / character count** *(Recommended)* — derived from
   `projectState.editorContent`. Word count is the headline; char count and an
   approximate reading time (`~N min`, words ÷ 200) are cheap extras.
7. **Git branch + sync chip** *(Recommended, with a caveat)* — current branch
   name plus a colored status dot (synced / ahead / behind / diverged), reusing
   the dot semantics already in `SyncModal.svelte`. Clicking opens the Sync
   modal (`onShowSync`).
   - **Caveat:** `git:getSyncStatus` performs a network `git fetch`, so it must
     **not** be polled from the status bar. Plan: show the cheap, local branch
     name continuously (`git:getBranch`), and reflect the *last known* sync
     state, refreshing it only on the events that already touch the remote
     (open project, push, pull, rebase, reset, and the Sync modal). The chip is
     a click target to open the Sync modal for a live re-check.
8. **MCP indicator** *(Optional)* — a small icon when the MCP server is running
   (`mcp:getStatus` → `running`). Only meaningful for users who enable MCP;
   keep it off by default / show only when running.

### Recommended first cut

Left: **file name** · **save status**
Right: **caret position** · **word count** · **branch + sync chip**

Everything else (note count, selection summary, reading time, MCP) is easy to
layer on once the bar exists.

## Design

Full-width bar at the very bottom, spanning under the left toolbar strip:

```
┌──────────────────────────────────────────────────────────────────────────┐
│ [Toolbar] │  sidebar  │        editor / preview / panels                  │
│           │           │                                                    │
│           │           │                                                    │
├──────────────────────────────────────────────────────────────────────────┤
│ meeting-notes.md   ● Saved        12 selected      Ln 42, Col 8   318 words   ⌥ main ● synced │
└──────────────────────────────────────────────────────────────────────────┘
   └ left zone ───────────────┘   └ center ──┘        └ right zone ───────────────────────────┘
```

- Height ~22–24 px, `font-size` small, single line, no wrap; segments separated
  by a thin divider or spacing. `flex` row with `justify-content: space-between`
  and the center zone as a flex spacer.
- Colors from existing theme variables (`--bg-surface`, `--text-secondary`,
  `--border`, `--accent`); sync dot colors reuse `SyncModal`'s
  `dot-blue` / `dot-orange` / etc.
- Empty/`no project` state: when no project is open (OpenScreen), the bar is
  hidden entirely (it has nothing to show).

## Current state (relevant facts)

- **Layout root:** `App.svelte` `.app-layout` is `display:flex; flex-direction:
  column`. `.app-body` inside it is a flex **row** — the `Toolbar` is a left
  vertical icon strip and `.main-area` fills the rest. Therefore the status bar
  must be the **last child of `.app-layout`** (after `.app-body`), not inside
  `.app-body`, so it spans the full window width.
- **Layout state + persistence:** `layout` is a `$state` object seeded from
  `DEFAULT_LAYOUT` (`App.svelte:35`). Changes are debounce-saved per project via
  `window.api.saveWindowState(...)` (`App.svelte:94`) and restored on project
  open (`App.svelte:321`). Adding a `showStatusBar` field to `DEFAULT_LAYOUT`
  is all that is needed for persistence to round-trip.
- **Toggle pattern:** each panel has a `handleToggleX` that flips `layout.showX`,
  a Toolbar `ToolbarButton`, and a registered command. Status bar follows the
  same shape.
- **Editor state available now:** `projectState.editorContent` (full text),
  `projectState.cursorLine` (already maintained in the `updateListener` at
  `Editor.svelte:137`). Column and selection length are **not** yet exposed.
- **Save flow:** `Editor.svelte` `scheduleSave` (line 152) writes after 500 ms
  with no exposed status. No `saveStatus` field exists yet.
- **Git:** `git:getBranch` (local, cheap) and `git:getSyncStatus` (does a
  network fetch — expensive). Sync dot semantics live in `SyncModal.svelte`
  (`dot-blue` = ahead, `dot-orange` = behind, etc.).
- **No existing status bar** in the app or in the shared `@laankit/desktop-ui`
  package — this is net-new. (Consider whether it should live in the shared
  package so ThreadLiner can adopt it later; see Open questions.)

## Implementation steps

1. **New component** `apps/noteliner/src/renderer/components/StatusBar.svelte`
   - Props: `onShowSync` (click target for the sync chip), and any branch/sync
     state needed (or read directly from a small store/getter).
   - Reads `projectState` for file name, caret, counts, save status.
   - Pure presentation + a couple of click handlers; no business logic.

2. **App.svelte — placement & wiring**
   - Render `{#if projectState.isOpen && layout.showStatusBar}<StatusBar … />{/if}`
     as the **last child of `.app-layout`**, after the `.app-body` block
     (~line 1008).
   - Add `showStatusBar: true` to `DEFAULT_LAYOUT`.
   - Add `handleToggleStatusBar` (`layout.showStatusBar = !layout.showStatusBar`).
   - Register a `toggle-statusbar` command + keyboard shortcut alongside the
     other panel toggles.

3. **Toolbar.svelte** — add a `ToolbarButton` with an `onToggleStatusBar` prop
   and `statusBarVisible` active-state, mirroring the Log/Outline toggles. Wire
   the prop through from `App.svelte`.

4. **Caret column** — in `Editor.svelte` `updateListener` (line 137), set
   `projectState.cursorCol = pos - line.from + 1` next to the existing
   `cursorLine` assignment. Add `cursorCol = $state(1)` to `ProjectState`.

5. **Save status** — add `saveStatus = $state('saved')` to `ProjectState`
   (`'saved' | 'saving' | 'unsaved'`). In `Editor.svelte`:
   set `'unsaved'` when `update.docChanged` fires, `'saving'` when the write
   begins, `'saved'` on success. Reset to `'saved'` on file select.

6. **Word/char counts** — derived in `StatusBar.svelte` from
   `projectState.editorContent` (regex word split + `.length`). Cheap enough to
   compute reactively; memoize with `$derived` if needed.

7. **Branch + sync chip** — `StatusBar.svelte` shows branch from `git:getBranch`
   (fetched on project open / branch change). Sync dot reflects a shared
   "last known sync status" value updated by the existing sync actions
   (push/pull/rebase/reset/Sync modal); clicking the chip calls `onShowSync`.
   *No background fetch from the status bar.*

8. **Selection summary (optional)** — track selection length in the editor's
   `updateListener` into `projectState` and render `N selected` when > 0.

9. **Styling** — scoped styles in `StatusBar.svelte` using theme variables;
   reuse `SyncModal` dot classes (extract to a shared snippet if convenient).

## Open questions (for reviewer)

1. **Shared vs. app-local?** Should `StatusBar` live in `@laankit/desktop-ui` so
   ThreadLiner can adopt it later (consistent with the "sibling apps" direction
   in `docs/plans/plan-refactor-refresh-ui.md`), or stay NoteLiner-local for
   now and be promoted later?
2. **First-cut segment set** — confirm the recommended set (file name, save
   status, caret, word count, branch+sync) and which optionals to include.
3. **Sync freshness** — is "last known sync status, refreshed only on
   remote actions" acceptable, or do you want a manual refresh affordance on the
   chip itself (e.g. click = re-fetch rather than open modal)?
4. **Default visibility** — status bar shown by default (`showStatusBar: true`)
   or opt-in?
5. **Reading time / char count** — include alongside word count, or keep the
   right zone minimal?
```
