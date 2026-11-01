# Plan: Refresh ThreadLiner UI to Match NoteLiner

Status: Draft for review Source app for reference: `apps/noteliner/` (within the LaanKit monorepo) Target app: `apps/threadliner/` (within the LaanKit monorepo)

## Goal

Adopt NoteLiner's more refined visual and interaction patterns into ThreadLiner without changing what ThreadLiner *does*. The result: ThreadLiner keeps its RSS-reader functionality but feels like a sibling app to NoteLiner — same chrome, same toolbar buttons, same modal language, same theme system.

## What gets adopted

1. **Sidebar (vertical) button style** — toolbar icon column
2. **Custom title bar** (opt-in, with associated UI prefs and IPC plumbing)
3. **Git sync UI** — drawer-style modal with status dot + push/pull/rebase/reset
4. **Settings UI layout** — top-tab modal (UI / Sync / Shortcuts)
5. **Settings UI styles** — toggle-option rows, theme list, scale list, restart banner
6. **Other reasonable adoptions** — see §7.

## 1. Vertical toolbar / sidebar button style

**Source:** `apps/noteliner/src/renderer/components/Toolbar.svelte` **Target:** `apps/threadliner/src/renderer/components/Toolbar.svelte`

ThreadLiner already uses a vertical icon column on the left edge. NoteLiner's version is narrower, denser, and styled with explicit semantic CSS variables.

### Adopt

- Width `48px` (from `72px`), `padding: 8px 0`, `gap: 4px`.
- Button: `36 × 36` rounded-`6px` square; size `16px` icon; hover swaps `--bg-button` + `--text-primary`; active state is `outline: 1px solid var(--accent)` with `background: var(--bg-selected)` and `color: var(--accent)`.
- Divider: 24-px wide 1-px line in `var(--border)`.
- Spacer (`flex: 1`) between primary and utility button groups.
- Update dot (small accent-coloured circle in upper-right of About button) when an update is queued — ThreadLiner doesn't ship auto-updates yet; leave the dot CSS in place but only wire it up if `window.api.getAppVersion` later gains an update channel.
- `active` modifier reflects which side-panel/modal is currently open.

### ThreadLiner-specific button mapping

| Group | Buttons (top → bottom) |
|---|---|
| File / feed | Add Feed, Edit Feed (disabled when no feed), Remove Feed (disabled), Refresh Feed (disabled) |
| Divider | |
| Read state | Mark Read, Mark Unread |
| Divider | |
| Tags | Tags modal |
| Spacer | |
| Utility | Sync (active when `syncStatus` is pending), Settings, About |

Keep `dispatch('event')` patterns or migrate to Svelte-5 callback props depending on §0 decision below. Either is fine — visuals are the only must-have.

## 2. Custom title bar + associated settings

**Source files:**

- `apps/noteliner/src/renderer/components/TitleBar.svelte`
- `apps/noteliner/src/main/main.js` (the `frame:` flag, the four `window:*` IPC handlers, the `ui:getPrefs` / `ui:setPrefs` / `app:relaunch` handlers, the `ui-preferences.json` userData file)
- `apps/noteliner/src/main/preload.js` (windowMinimize/Maximize/Close/IsMaximized, onWindowMaximizedChange, getUIPrefs/setUIPrefs, relaunchApp)
- `apps/noteliner/src/renderer/App.svelte` (the `customTitlebar` state + the CSS variable `--titlebar-height` so modal overlays offset correctly)

### Behaviour

- Off by default; user enables in Settings; an inline "Restart now" banner appears below the toggle and only goes away after relaunch (because `frame:` can only be set at `BrowserWindow` construction).
- When enabled, the 32-px bar shows: hamburger (toggles toolbar visibility), app-action group (home / open folder / new file / import — for ThreadLiner: Add Feed / Refresh / Tags), centred app title, then minimize / maximize / close.
- `-webkit-app-region: drag` on the bar, `no-drag` on buttons.
- Maximise button swaps between `fa-window-maximize` and `fa-window-restore` based on `window:isMaximized`. ThreadLiner must subscribe to `onWindowMaximizedChange` so the icon stays in sync.
- The title-bar uses `background: var(--accent)` and `color: var(--accent-on)` — NoteLiner's signature look.

### ThreadLiner customisations

- App title: "ThreadLiner".
- Quick-action buttons in the bar: Add Feed (`fa-plus`), Refresh Feed (disabled when no feed selected), Tags (`fa-tags`).
- The hamburger toggles the *toolbar* (the vertical icon column). That makes ThreadLiner match NoteLiner's keyboard-friendly minimal mode.

### IPC additions to `src/main/preload.js`

```js
windowMinimize: () => ipcRenderer.invoke('window:minimize'),
windowMaximize: () => ipcRenderer.invoke('window:maximize'),
windowClose: () => ipcRenderer.invoke('window:close'),
windowIsMaximized: () => ipcRenderer.invoke('window:isMaximized'),
onWindowMaximizedChange: (cb) => {
  const handler = (_e, v) => cb(v);
  ipcRenderer.on('window:maximized-change', handler);
  return () => ipcRenderer.removeListener('window:maximized-change', handler);
},
getUIPrefs: () => ipcRenderer.invoke('ui:getPrefs'),
setUIPrefs: (p) => ipcRenderer.invoke('ui:setPrefs', p),
relaunchApp: () => ipcRenderer.invoke('app:relaunch'),
```

### `src/main/main.js` additions

- Load `ui-preferences.json` (separate file from `config.json` because it is device-local UI state, not a synced setting). Defaults: `{ customTitlebar: false }`.
- `frame: !uiPrefs.customTitlebar` when constructing `BrowserWindow`.
- Wire `mainWindow.on('maximize'/'unmaximize')` → broadcast `window:maximized-change` to the renderer.
- `app:relaunch` → `app.relaunch(); app.exit(0);`.

## 3. Git sync UI

**Source:** `apps/noteliner/src/renderer/components/SyncModal.svelte` (drawer style)

### Adopt

- Modal anchored to bottom of the window, `width: 65%`, `height: 50%`, `min-width: 624px`, `min-height: 380px`, with the slide-up animation from the shared `.modal` class.
- Sections: Remote URL row (input + disconnect icon-button); Branch row; Status row (coloured dot + message + refresh icon-button); error message box; bottom action row (Pull / Pull & Rebase / Push / Reset from Remote on the left, Close on the right).
- Status states with dot colours:
  - `synced` → green
  - `ahead` → blue
  - `behind` → orange
  - `diverged` → red
  - `no-upstream` → grey
  - `error` → red
- Disconnect → compact confirm modal. Reset from Remote → compact confirm modal with red warning banner.

### Reconciling with ThreadLiner's auto-sync model

ThreadLiner already has a *push-pending* sync manager: it auto-commits after local edits and pushes after a debounce window. NoteLiner's UI is more manual. Two ways to reconcile:

- **Option A — Hybrid (recommended).** Keep the auto-sync engine, but expose the same NoteLiner controls. Adds:
  - `git:getRemoteUrl` / `git:setRemoteUrl` / `git:removeRemote`
  - `git:getBranch`
  - `git:getSyncStatus` (computes ahead/behind by parsing `git rev-list --left-right --count @{u}...HEAD`)
  - `git:pull`, `git:pullRebase`, `git:push`, `git:resetToRemote`
  - Existing `forcePush` / `forcePull` IPC handlers stay; the sync log moves to a separate "Activity" tab in the Settings → Sync section so the main sync modal stays clean.

- **Option B — Replace.** Drop auto-sync, push only when the user clicks. Faster to implement but loses ThreadLiner's headline feature.

Plan assumes **Option A**.

### ThreadLiner-specific changes

- Branch row → display branch from `gitSync.getCurrentBranch()` (new helper in `src/main/git-sync.js`).
- "Last sync" + sync log → move to Settings → Sync tab.
- Status polling can re-use the existing 2-second poll loop in `stores/sync.js`; just translate states to the NoteLiner vocabulary.

## 4. Settings UI layout

**Source:** `apps/noteliner/src/renderer/components/SettingsModal.svelte`

### Adopt

- Top-tab strip directly under the modal header (replaces ThreadLiner's left-tab vertical rail). Tab styling: `padding: 10px 16px`, font-size 13, active tab gets `color: var(--accent)` + 2-px bottom border in `var(--accent)`.
- `.setting-group` block pattern: small-caps uppercase label (11px / weight 600 / letter-spacing 0.05em / `var(--text-muted)`), then the control, then optional `.setting-help` text.
- `.toggle-option` button pattern: 100% wide row with check/uncheck icon on the left, label on the right; active state lights up with `var(--bg-selected)` + `outline: 1px solid var(--accent)`.
- `.theme-option` pattern: similar to toggle but radio-style.
- `.scale-list` pattern: horizontal pill row of `--bg-button` buttons.
- `.restart-banner` style for pending restart-required prefs.
- Modal footer: right-aligned OK button styled as the accent pill.

### Tabs for ThreadLiner

| Tab | Settings |
|---|---|
| UI | Theme list (currently 3 themes), UI Scale list, Custom Window Titlebar toggle (with restart banner) |
| Sync | Sync Wait Time select, last-sync timestamp, sync log (move from SyncModal) |
| Shortcuts | Static list (small for now: arrow nav, refresh, mark read/unread, etc.) |

## 5. Settings UI styles (theme + scale system)

**Source:** `apps/noteliner/src/renderer/styles/global.css` + `apps/noteliner/src/renderer/stores/theme.svelte.js`

NoteLiner uses *JS-applied inline CSS variables* on `documentElement`, with ~30 semantic variables per theme. ThreadLiner currently uses `[data-theme="…"]` selectors with ~12 variables.

### Migration

1. **Rename variables.** Map ThreadLiner's `--color-*` set to NoteLiner's semantic set. Suggested mapping:

   | ThreadLiner | NoteLiner |
   |---|---|
   | `--color-bg` | `--bg-base` |
   | `--color-surface` | `--bg-surface` |
   | `--color-surface-hover` | `--bg-button-hover` |
   | `--color-surface-active` | `--bg-selected` |
   | `--color-border` | `--border` |
   | `--color-text` | `--text-primary` |
   | `--color-text-muted` | `--text-muted` |
   | `--color-accent` | `--accent` |
   | `--color-accent-hover` | `--accent-hover` |
   | `--color-danger` | (new constant — keep) |
   | `--color-toolbar-bg` | `--bg-overlay` |
   | `--color-sidebar-bg` | `--bg-surface` (or new `--bg-sidebar`) |

   Add the missing NoteLiner variables (input, scrollbar, code, modal-overlay, accent-on, text-secondary/tertiary/faint, bg-button, bg-drag-over, bg-item-hover) by extrapolating from ThreadLiner's current palette so the three ThreadLiner themes still look right.

2. **Replace `applyTheme(value)`** in `stores/theme.js`:

   ```js
   function applyTheme(themeId) {
     const vars = THEMES[themeId].vars;
     const root = document.documentElement;
     for (const [k, v] of Object.entries(vars)) {
       root.style.setProperty(k, v);
     }
   }
   ```

3. **Add UI scale.** Port `applyScale`, `zoomIn`, `zoomOut`, `zoomReset` and the `UI_SCALES` array verbatim. Add `--ui-zoom`, `--ui-zoom-height`, `--ui-zoom-width` to `:root`. Wrap the app shell with `zoom: var(--ui-zoom, 1); height: var(--ui-zoom-height, 100vh)`. Keyboard bindings `Ctrl+=`, `Ctrl+-`, `Ctrl+0`.

4. **Persist** in localStorage (`threadliner-theme`, `threadliner-scale`) *and* mirror to settings via `window.api.setSetting` so the value survives data-dir migration. Read priority on boot: localStorage → `window.api.getSetting('theme')` → default.

5. **Adopt the shared modal styles** from `global.css`:
   - `.modal-overlay` (the full-screen padded slide-up overlay)
   - `.modal-overlay-compact` + `.modal-compact` (centred small dialogs)
   - `.modal-header` with accent background
   - `.modal-body`
   - `.pane-header` / `.pane-title` / `.pane-header-actions` / `.pane-header-btn`

6. **Apply `--titlebar-height` offset** to modal overlays so they don't overlap the custom title bar when enabled.

## 6. Other UI / layout to adopt

### 6.1 About modal (drawer style)

`apps/noteliner/src/renderer/components/AboutModal.svelte` — replace ThreadLiner's About. Drop the auto-update sections (ThreadLiner doesn't have an auto-updater yet); keep the icon / version / description / repo link / OK button structure and the bottom-anchored 50% × 50% drawer style. Use ThreadLiner's existing icon.

### 6.2 Setup flow (`OpenScreen` analogue)

NoteLiner has an `OpenScreen` that fills the editor area when no project is open. ThreadLiner's `SetupDialog` is full-screen and only runs once. Keep ThreadLiner's behaviour, but restyle to match NoteLiner's modal vocabulary (`.modal-body`, `.setting-group`, `.toggle-option`).

### 6.3 Sidebar styling (the feed list)

Even though ThreadLiner's sidebar isn't paneable, adopt:
- `.pane-header` look (small-caps label + close X) for the "Feeds" and "Tags" section headers.
- Item hover/selected colours from `--bg-item-hover` and `--bg-selected`.
- Unread badge using `var(--accent)` + `var(--accent-on)` instead of hard-coded white-on-accent.

Defer NoteLiner's drag-to-reorder paneable layout to a follow-up — it is overkill for an RSS reader with two static panes.

### 6.4 Error toast

Restyle the existing top-error toast to use `var(--danger, #e06070)` and match the `.modal-compact` border radius / shadow.

### 6.5 Font Awesome icon set

Already used in both apps. Standardise on the icons NoteLiner uses for common verbs (`fa-cloud-arrow-up` for push, `fa-cloud-arrow-down` for pull, `fa-rotate-left` for reset, `fa-arrows-rotate` for refresh status, `fa-circle-info` for about, `fa-circle-question` for help, `fa-gear` for settings, `fa-bars` for hamburger).

## 7. Sequencing

Eight stages, in dependency order. Each can ship behind a working build.

1. **Variable rename + theme store rewrite (§5.1–5.2).** Pure CSS / store churn; ship green tests before touching components.
2. **Adopt shared modal classes (§5.5).** Pull `global.css` modal/pane styles in; restyle existing modals component-by-component.
3. **Toolbar restyle (§1).** Resize buttons, add divider/spacer, swap to active-state outline.
4. **Settings layout (§4).** Rebuild SettingsModal as top-tabs; carry over existing controls.
5. **UI scale + zoom shortcuts (§5.3).** Add controls in Settings → UI.
6. **Custom titlebar (§2).** New IPC handlers, new component, prefs file, restart banner.
7. **Sync UI (§3).** New status/branch IPC handlers in `git-sync.js`, refactor SyncModal to drawer style + status dot, move log to Settings.
8. **About modal + small polish (§6).**

## 8. Decisions to make before starting

### 0. Svelte 4 vs Svelte 5

NoteLiner uses Svelte 5 runes (`$state`, `$props`, `$derived`, `$effect`). ThreadLiner is on Svelte 4 (`writable`, `$store`, `createEventDispatcher`). Three options:

- **Keep Svelte 4.** Port markup + styles; rewrite Svelte-5 idioms back to Svelte 4 (writables for theme, callbacks via `createEventDispatcher`). Lowest risk, highest porting cost per component.
- **Upgrade to Svelte 5.** Highest one-time cost (rewrite stores + components) but lets us paste NoteLiner code with minimal edits and keeps the two codebases convergent.
- **Mixed.** Svelte 4 with a polyfill-style adapter — not recommended, produces a hybrid that is harder to maintain than either pure choice.

Recommendation: **upgrade to Svelte 5** as step 0 of this plan. The plan above assumes that decision; if we stay on Svelte 4, every component section needs an extra translation pass.

### 0a. Sync model

Pick Option A or B from §3 before §7-step-7. Recommendation: Option A.

## 9. Files touched (estimate)

- New: `src/renderer/components/TitleBar.svelte`, `src/renderer/stores/uiPrefs.js`.
- Rewritten: `Toolbar.svelte`, `SettingsModal.svelte`, `SyncModal.svelte`, `AboutModal.svelte`, `Sidebar.svelte` (styles only), `stores/theme.js`, `styles/global.css`.
- Modified: `App.svelte` (mount TitleBar, expose `--titlebar-height`), `main/main.js` (frame flag, window IPC, prefs file, relaunch), `main/preload.js` (new methods), `main/git-sync.js` (status/branch/remote helpers).
- Unchanged: `feed-parser.js`, `data-store.js`, `sync-manager.js` (auto-commit engine unchanged in Option A).

## 10. Risks

- **Custom titlebar + Linux.** `frame: false` on some Linux WMs disables window-snapping. NoteLiner already runs on Linux; verify by smoke-testing on the same window manager.
- **`zoom` CSS property + Electron.** Works in Chromium but is non-standard. Acceptable for desktop; if we ever ship a web build, swap to `transform: scale()` + bounds math.
- **`-webkit-app-region: drag` over interactive elements.** Children of a drag region intercept clicks unless explicitly tagged `no-drag`. NoteLiner already deals with this — copy the pattern exactly.
- **Sync IPC surface change.** Renaming/adding handlers is a breaking change for any open ThreadLiner instance — bump the IPC version comment block and document the migration in the release notes.

## 11. Out of scope

- Command palette (`Ctrl+K`)
- Paneable / reorderable sidebar
- MCP server
- Auto-updater
- Help window

Each of these can be a follow-up once the surface refresh is in.
