# Plan: LaanKit — Shared UI Foundation for NoteLiner and ThreadLiner

Status: Active — execution tracked in [`plan-refactor-steps.md`](./plan-refactor-steps.md)
Monorepo: `~/dev/hub/laan/laankit/` (apps under `apps/`, shared packages under `packages/`)

## Goal

Pull the visual + interaction primitives that NoteLiner and ThreadLiner have
in common into a single reusable library. Each app keeps its own
identity, main-process code, and domain logic — only the shared chrome,
theme system, and Electron IPC glue is consolidated.

The library is small enough to install in either app without forcing a
mono-repo, but designed so that a mono-repo (npm workspaces / pnpm) is the
natural home for it if both apps are ever cloned together.

## Naming

The platform is **LaanKit** — the dock where our "liner"-suffixed desktop
apps live. This library, **`@laankit/desktop-ui`**, is the shared UI kit
that powers LaanKit's apps: **NoteLiner** and **ThreadLiner** (renamed
from "Threadline" to match the `-liner` suffix). The package only ever
ships internally; it is not published to npm.

Brand-style guide for this plan:

- **LaanKit** — the platform / monorepo / umbrella product
- **`@laankit/...`** — the npm scope for shared packages
- **NoteLiner**, **ThreadLiner** — the two apps (the "liners" that dock
  at LaanKit)

## Scope: what goes in the foundation

### In-scope (today)

| Area | What |
|---|---|
| Theme system | Variable definitions, theme registry, UI-scale registry, theme-state store, IPC-backed persistence helpers |
| Global CSS | `box-sizing` reset, scrollbar styles, `.modal-overlay`, `.modal-overlay-compact`, `.modal`, `.modal-compact`, `.modal-header`, `.modal-body`, `.pane-header` family |
| Svelte components | `TitleBar`, `Toolbar`, `ToolbarButton`, `Modal`, `ConfirmModal`, `SettingsShell` (tab strip + body), `AboutModal`, `ThemeSettingsTab`, `KeyboardShortcutsTab` |
| Hooks / stores | `themeState`, `uiPrefsState`, `windowState` (maximized observer), `commandRegistry` (optional, see below) |
| Electron helpers | A small `electron-ui-host` module the main process imports to register the standard window/UI-prefs IPC handlers (`window:minimize`, `window:maximize`, `window:close`, `window:isMaximized`, `ui:getPrefs`, `ui:setPrefs`, `app:relaunch`) |
| Preload helpers | A `definePreloadApi(...)` helper that returns the set of methods the renderer side of the library expects (`windowMinimize`, `getUIPrefs`, etc.). Apps spread their own preload alongside it. |
| Icons | Re-export Font Awesome free as the standard set so both apps use the same version. |

### Out of scope

- Domain components (FileTree, OutlinePane, BacklinksPane, EntryList,
  ContentViewer, FeedParser, ProjectService, McpService, GitService).
- Sync UI — the user-facing modal is similar, but the underlying sync
  *engine* differs (manual git in NoteLiner, auto-debounced in ThreadLiner).
  We export a generic `SyncModal` whose props accept a status descriptor +
  action callbacks, and each app supplies its own engine.
- Auto-updater (NoteLiner only, for now).
- MCP server (NoteLiner only).
- Command palette is included as a *separate optional sub-export*
  (`@laankit/desktop-ui/command-palette`) because it pulls in a fuzzy-match
  dependency ThreadLiner doesn't otherwise need.

## Repository layout

Two viable structures.

### Option A: separate repo + npm install via git URL

```
@laankit/desktop-ui (own repo, at github.com/NathanLaan/laankit-ui)
├── package.json     "type": "module", exports map below
├── src/
│   ├── theme/
│   ├── components/
│   ├── stores/
│   ├── electron-host/
│   └── styles/
├── dist/            (prebuilt; checked in or generated on install)
└── README.md

noteliner/
threadlinerr/
  package.json:
    "dependencies": { "@laankit/desktop-ui": "github:NathanLaan/laankit-ui#v0.1.0" }
```

Pros: each app installs a versioned tag; no monorepo wiring; no impact on
build pipelines. Cons: every change requires a tag-bump cycle.

### Option B: monorepo with npm workspaces

```
laankit/                       (new umbrella repo, github.com/NathanLaan/laankit)
├── package.json              (workspaces: ["packages/*", "apps/*"])
├── packages/
│   └── desktop-ui/
└── apps/
    ├── noteliner/            (current noteliner repo, moved in)
    └── threadliner/           (current threadliner repo, moved in)
```

Pros: one PR can update the library + both consumers; native `npm install`
links the workspace; consistent toolchain (one `electron`, one `svelte`,
one `vite`). Cons: rewrites git history if we want to preserve it
(`git subtree`/`git filter-repo`), forces both apps onto a single
electron/svelte version, breaks anyone who has the apps checked out
elsewhere.

**Recommendation:** Option B — but only because we already maintain both
apps as a pair. A workspace shrinks build configuration and keeps
shared-code changes atomic. Migration steps are in §6.

## Package exports

```jsonc
// packages/desktop-ui/package.json
{
  "name": "@laankit/desktop-ui",
  "version": "0.1.0",
  "type": "module",
  "svelte": "./src/index.js",
  "exports": {
    ".":                  { "svelte": "./src/index.js" },
    "./styles":           "./src/styles/global.css",
    "./theme":            "./src/theme/index.js",
    "./components":       "./src/components/index.js",
    "./command-palette":  "./src/command-palette/index.js",
    "./electron-host":    "./src/electron-host/index.js",
    "./preload":          "./src/preload/index.js"
  },
  "peerDependencies": {
    "svelte": "^5.0.0",
    "electron": ">=28"
  }
}
```

### `import { ... }` shape

```js
// renderer (Svelte 5)
import { themeState, uiPrefsState }           from '@laankit/desktop-ui/theme';
import { TitleBar, Toolbar, Modal, AboutModal, SettingsShell, ThemeTab, KeyboardShortcutsTab } from '@laankit/desktop-ui/components';
import { CommandPalette, commandRegistry }    from '@laankit/desktop-ui/command-palette';
import '@laankit/desktop-ui/styles';

// main process
import { registerWindowHandlers, registerUIPrefsHandlers } from '@laankit/desktop-ui/electron-host';
registerWindowHandlers({ getWindow: () => mainWindow });
registerUIPrefsHandlers({ prefsPath: path.join(app.getPath('userData'), 'ui-preferences.json'), defaults: {...} });

// preload
import { exposeWindowApi, exposeUIPrefsApi } from '@laankit/desktop-ui/preload';
contextBridge.exposeInMainWorld('api', {
  ...exposeWindowApi(),
  ...exposeUIPrefsApi(),
  // app-specific:
  addFeed: (...) => ipcRenderer.invoke('feed:add', ...),
});
```

## Component contracts (sketch)

### `<TitleBar appName slot=actions />`

```svelte
<TitleBar appName="ThreadLiner" onToggleToolbar={...} toolbarVisible={...}>
  <svelte:fragment slot="actions">
    <ToolbarButton icon="fa-plus" title="Add Feed" onclick={...} />
    <ToolbarButton icon="fa-sync-alt" title="Refresh" onclick={...} disabled={...} />
  </svelte:fragment>
</TitleBar>
```

The standard min/max/close trio is rendered by the library; apps customise
the middle action group via slot.

### `<Toolbar />`

Same slot-based pattern. The library exports `ToolbarButton`,
`ToolbarDivider`, `ToolbarSpacer` so each app can compose its own set.

### `<SettingsShell tabs={...} bind:active />`

Renders the top-tab strip + body. Each tab is `{ id, label, component }`.
Library ships `ThemeTab`, `UIScaleTab`, `KeyboardShortcutsTab`,
`CustomTitlebarTab`. Apps add their own (`SyncTab`, `MCPTab`,
`FeedDefaultsTab`).

### `<Modal />`, `<ConfirmModal />`

Generic wrapping primitives that handle Esc/click-outside, focus trap,
slide-up animation, and `--titlebar-height` offset.

### `<AboutModal appName version repoUrl iconSrc updateState? />`

Drawer-style by default. `updateState` is an optional prop — when omitted,
the update UI doesn't render. Lets ThreadLiner use the same component
without dragging in `electron-updater`.

## Theme system

The library owns the variable names. Apps:

1. Optionally add their own themes by passing additional entries to
   `themeState.registerTheme(id, def)` at boot.
2. Choose the default theme: `themeState.setDefault('midnight')`.
3. Pick which themes show in the picker:
   `themeState.setVisible(['midnight','dark','light'])`.

ThreadLiner keeps `light`, `dark`, `midnight`. NoteLiner adds `dark-purple`,
`light-purple`, `light-mulberry`. All apps share the same variable surface,
so a component styled with `var(--bg-button)` looks right under every
registered theme.

### Persistence

`themeState.init()` accepts a persistence adapter:

```js
themeState.init({
  load:   () => window.api.getSetting('uiTheme'),
  save:   (id) => window.api.setSetting('uiTheme', id),
  localStorageKey: 'app-theme',
});
```

The library reads localStorage first (sync, no flash), then reconciles
with the adapter on mount.

## CSS layering

Two-layer system:

1. **Variables.** Defined per-theme on `documentElement` by the theme
   store. Apps can override any variable by setting it on a deeper
   selector — e.g., ThreadLiner can pin `--accent` to its brand colour by
   default while NoteLiner uses the user's pick.
2. **Component CSS.** Each library component ships scoped CSS. The shared
   `global.css` only defines reset + scrollbar + the modal/pane primitives
   that need to be globally addressable.

Apps must `import '@laankit/desktop-ui/styles'` once in `main.js` before
mounting.

## Electron-host helpers

Goal: each app's `main.js` should reduce by ~120 lines.

```js
// packages/desktop-ui/src/electron-host/index.js
export function registerWindowHandlers({ getWindow }) {
  ipcMain.handle('window:minimize',    () => getWindow()?.minimize());
  ipcMain.handle('window:maximize',    () => { /* toggle */ });
  ipcMain.handle('window:close',       () => getWindow()?.close());
  ipcMain.handle('window:isMaximized', () => !!getWindow()?.isMaximized());

  app.on('browser-window-created', (_e, win) => {
    win.on('maximize',   () => win.webContents.send('window:maximized-change', true));
    win.on('unmaximize', () => win.webContents.send('window:maximized-change', false));
  });
}

export function registerUIPrefsHandlers({ prefsPath, defaults }) {
  let prefs = load();
  ipcMain.handle('ui:getPrefs', () => prefs);
  ipcMain.handle('ui:setPrefs', (_e, patch) => {
    prefs = { ...prefs, ...patch };
    fs.writeFileSync(prefsPath, JSON.stringify(prefs, null, 2));
    return prefs;
  });
  function load() { try { return { ...defaults, ...JSON.parse(fs.readFileSync(prefsPath, 'utf-8')) }; } catch { return { ...defaults }; } }
}

export function registerRelaunchHandler() {
  ipcMain.handle('app:relaunch', () => { app.relaunch(); app.exit(0); });
}

export function applyFrameFromPrefs(opts, prefs) {
  return { ...opts, frame: !prefs.customTitlebar };
}
```

## What stays in each app

Everything domain-specific: ProjectService, GitService, McpService,
FeedParser, DataStore, SyncManager, route registration in App.svelte,
window-state persistence keyed to the app's notion of "open project".

Each app keeps its own:

- `package.json` (depends on `@laankit/desktop-ui`)
- `vite.config.mjs`
- `electron-builder.yml`
- `assets/` (icons, splash, etc.)
- `src/main/main.js` (composed of library + app code)
- `src/main/preload.js`
- `src/renderer/App.svelte`
- `src/renderer/components/` (domain components only — chrome moves to library)

## Migration plan

Phased so the apps stay shippable throughout.

### Phase 0 — Convergence prep (NoteLiner = source of truth)

The library's API mirrors NoteLiner's current code. Phase 0 makes
ThreadLiner ready to consume it.

1. Adopt the [`plan-refactor-refresh-ui.md`](./plan-refactor-refresh-ui.md)
   work in ThreadLiner, including the Svelte 5 upgrade. This brings
   ThreadLiner's component shapes in line with what the library will
   eventually export.
2. **Result:** ThreadLiner + NoteLiner have very similar files that don't
   share code yet but look alike. Both apps still ship independently.

### Phase 1 — Carve out the library (no consumer changes yet)

3. Create `packages/desktop-ui/` (in whichever repo layout we picked).
4. Copy from NoteLiner:
   - `styles/global.css` → library
   - `stores/theme.svelte.js` → library (rename to `theme/index.js`)
   - `stores/commands.svelte.js` → library `command-palette/`
   - `components/TitleBar.svelte`, `Toolbar.svelte` parts that aren't
     domain-coupled → library
   - `components/SettingsModal.svelte` → split into `SettingsShell` +
     tabs; the MCP tab stays in NoteLiner as a custom tab.
   - `components/AboutModal.svelte` → library (with optional
     `updateState`).
   - Window/UI-prefs/relaunch IPC handlers from `main.js` → library
     `electron-host/`.
   - Preload helpers → library `preload/`.
5. Add a small `examples/playground/` Electron app inside the library so
   we can render the components without either consumer mounted.

### Phase 2 — Convert NoteLiner to the library

6. Replace NoteLiner's local copies with imports from `@laankit/desktop-ui`.
7. The MCP tab, GitService, ProjectService stay local; everything else is
   library-provided.
8. Diff the chrome before/after: any visual drift gets fixed in the
   library, not in NoteLiner.

### Phase 3 — Convert ThreadLiner to the library

9. Replace ThreadLiner's local copies with the same imports.
10. ThreadLiner's `SettingsShell` registers its own `SyncTab` and
    `FeedDefaultsTab` (custom tabs).
11. The sync engine stays in `src/main/sync-manager.js`; only the
    *modal* (status dot + action row) is library-provided. ThreadLiner
    passes a status descriptor and action callbacks.

### Phase 4 — Library hardening

12. Snapshot tests via the playground app for each component × each theme
    × each scale.
13. Document supported Svelte / Electron versions.
14. Lock the public API; cut `@laankit/desktop-ui v1.0.0`.

## Risks / costs

- **Svelte upgrade is a hard prerequisite.** The library is Svelte 5
  only. ThreadLiner must finish the refresh-UI plan (Svelte 5 upgrade
  included) before it can consume the library.
- **Variable rename ripple.** Every component that reads `--color-bg` etc.
  needs to migrate to the library's variable set.
- **Workspace churn.** If we pick monorepo (Option B), git history of the
  current repos either gets rewritten (`git subtree split` + `git
  filter-repo`) or merged with `--allow-unrelated-histories`. Neither is
  destructive but both have side effects for anyone with a fork.
- **Library version drift.** If the workspace approach is *not* taken,
  every library change demands a tag, a bump in two consumers, and two
  app-level QA passes.
- **Test coverage.** NoteLiner has Playwright tests that pin behaviour;
  ThreadLiner has none. Adopting the library in ThreadLiner lacks safety
  net unless we also adopt NoteLiner's test harness (worth it).

## Effort estimate

| Phase | Calendar |
|---|---|
| 0 | already covered by `plan-refactor-refresh-ui.md` |
| 1 | 1–2 weeks (library carve-out + playground) |
| 2 | 3–5 days (NoteLiner cutover) |
| 3 | 3–5 days (ThreadLiner cutover) |
| 4 | 1 week (hardening + v1) |

Total: **~3–4 weeks of focused work**, assuming Phase 0 is done.

## Non-goals

- We are not building a generic Electron UI kit. The library exists to
  serve exactly two apps. If a third app appears, we'll re-evaluate.
- We are not vendoring Font Awesome. Both apps already depend on it; the
  library just declares it as a peer dependency.
- We are not building a design-token pipeline. Variables are hand-edited
  in JS; tokens are simple key/value pairs.
