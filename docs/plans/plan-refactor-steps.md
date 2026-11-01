# Plan: LaanKit foundation — remaining execution steps

Status: Active. Tracks the work needed to finish executing [`plan-refactor-common-foundation.md`](./plan-refactor-common-foundation.md) inside this monorepo.

## Where we are

Bootstrap and history import are complete:

- `laankit/` monorepo initialized with workspaces (`packages/*`, `apps/*`)
- `apps/noteliner/` imported with full git history under `apps/noteliner/`
- `apps/threadliner/` imported (renamed from `threadline`) with full git history under `apps/threadliner/`
- npm package rename `threadline` → `threadliner` committed
- `npm install` at the root has populated `node_modules/`

Confirm with:

```bash
cd ~/dev/hub/laan/laankit
git log --oneline | head -5
ls apps/                          # noteliner + threadliner
node -p "require('./apps/threadliner/package.json').name"  # → "threadliner"
node -p "require('./apps/noteliner/package.json').name"    # → "noteliner"
```

If anything above is missing, finish the git-import sequence before continuing — see git history in `git log` for what's been done so far.

## Phase map (where each step lives in the bigger plan)

| Step | Phase from foundation plan | Detail spec |
|---|---|---|
| 4. Consolidate root tooling | Phase 0 prep | this doc |
| 5. Upgrade ThreadLiner stack | Phase 0 prep | this doc |
| 6. Apply refresh-UI plan to ThreadLiner | Phase 0 | [`plan-refactor-refresh-ui.md`](./plan-refactor-refresh-ui.md) |
| 6.5. Unify ThreadLiner's dev script with NoteLiner's pattern | Phase 0 prep | this doc |
| 7. Carve out `packages/desktop-ui/` | Phase 1 | [`plan-refactor-common-foundation.md`](./plan-refactor-common-foundation.md) §"Migration plan" |
| 8. Convert NoteLiner to consume library | Phase 2 | foundation plan §"Migration plan" |
| 9. Convert ThreadLiner to consume library | Phase 3 | foundation plan §"Migration plan" |
| 10. Library hardening + v1.0 | Phase 4 | foundation plan §"Migration plan" |

## Step 4 — Consolidate root tooling (light touch)

Goal: shared, version-agnostic project plumbing at the root. **Do not hoist `electron`, `vite`, `svelte`, or `@sveltejs/vite-plugin-svelte` yet** — ThreadLiner and NoteLiner are on different versions until Step 5.

### 4.1 Verify both apps still build

```bash
cd ~/dev/hub/laan/laankit
npm run build:noteliner
npm run build:threadliner
```

Both should produce `apps/<app>/dist/` without error.

### 4.2 Add root-level shared files

- `.editorconfig` — pin indent, line endings across both apps. Suggested:

  ```ini
  root = true

  [*]
  charset = utf-8
  end_of_line = lf
  indent_style = space
  indent_size = 2
  insert_final_newline = true
  trim_trailing_whitespace = true

  [*.md]
  trim_trailing_whitespace = false
  ```

- `.gitattributes` (optional) — normalise line endings:

  ```
  * text=auto eol=lf
  ```

- Root `LICENSE` already in place.

### 4.3 Smoke-test root scripts

```bash
npm run build        # runs `npm run build -ws --if-present` — both apps
npm run dev:noteliner    # Ctrl+C to exit
npm run dev:threadliner  # Ctrl+C to exit
```

### 4.4 Optional: CI workflow

If you want CI now, add `.github/workflows/build.yml`:

```yaml
name: build
on: [push, pull_request]
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: 20
          cache: npm
      - run: npm ci
      - run: npm run build
```

**Done when:** `npm run build` succeeds at the root for both apps, and the shared root files are committed.

**Commit:** `chore: add root tooling files (editorconfig, gitattributes)`

---

## Step 5 — Upgrade ThreadLiner to Svelte 5 + Vite 6 + Electron 41

This is the longest single step. Budget 3–5 days. Best done on a dedicated branch: `git checkout -b step-5/threadliner-stack-upgrade`.

### 5.1 Bump devDependencies

Edit `apps/threadliner/package.json`:

```jsonc
"devDependencies": {
  "@sveltejs/vite-plugin-svelte": "^5.0.0",
  "electron": "^41.2.0",
  "svelte": "^5.0.0",
  "vite": "^6.0.0"
}
```

Match NoteLiner's exact versions (cross-check `apps/noteliner/package.json`). Run `npm install` at the root.

### 5.2 Svelte 4 → 5 component rewrites

For each `.svelte` file under `apps/threadliner/src/renderer/`:

- `export let foo = default;` → `let { foo = default } = $props();`
- `let foo = 0;` (reactive local) → `let foo = $state(0);`
- `$: derived = foo + 1;` → `let derived = $derived(foo + 1);`
- `$: { sideEffect(); }` → `$effect(() => { sideEffect(); });`
- `on:click={handler}` → `onclick={handler}`
- `bind:value={foo}` — unchanged
- `<svelte:component this={X} {...props} />` — unchanged (still valid)
- `createEventDispatcher()` + `dispatch('close')` → callback props (`let { onClose } = $props()`; call site: `<Modal {onClose}/>`)

### 5.3 Store rewrites

Convert `src/renderer/stores/*.js` from `writable` to runed classes *only* for stores that own UI state. Stores backing IPC state (like `sync.js`) can stay `writable` — Svelte 5 still supports them via the `$store` auto-subscription syntax.

Recommended conversions:

- `stores/theme.js` → `theme.svelte.js` with a class instance (will be replaced wholesale in Step 6 anyway, so a minimal port is fine).
- Keep `stores/app.js`, `stores/sync.js` as-is for this step; they're reactive store APIs that Svelte 5 still supports.

### 5.4 Vite 5 → 6 + Electron 28 → 41

Both are mostly drop-in:

- Vite 6: `@sveltejs/vite-plugin-svelte` v5 requires Svelte 5 — already bumped above.
- Electron 41: skim https://www.electronjs.org/docs/latest/breaking-changes for the 28→41 jump. Most likely affected: deprecated `remote` module (ThreadLiner doesn't use it), CSP defaults. Smoke-test the app.

### 5.5 Validate

```bash
cd ~/dev/hub/laan/laankit
npm install
npm run build:threadliner
npm run dev:threadliner
```

Exercise every ThreadLiner feature manually: add a feed, refresh, mark read/unread, sync, settings dialog, tags dialog. Nothing should be worse than before this step.

**Done when:** ThreadLiner runs on the new stack with all current functionality intact. Commit the branch.

**Commit:** `feat(threadliner): upgrade to Svelte 5 + Vite 6 + Electron 41`

### 5.6 Hoist common devDependencies (now that versions match)

Move from each app's `devDependencies` to the root `package.json`:

```jsonc
"devDependencies": {
  "@sveltejs/vite-plugin-svelte": "^5.0.0",
  "electron": "^41.2.0",
  "svelte": "^5.0.0",
  "vite": "^6.0.0"
}
```

Keep app-specific devDependencies (Playwright in NoteLiner, electron-builder configs) in their respective workspaces.

```bash
npm install
npm run build  # verify both apps still build
```

**Commit:** `chore: hoist shared devDependencies to monorepo root`

---

## Step 6 — Apply the refresh-UI plan to ThreadLiner

Branch: `step-6/threadliner-ui-refresh`.

Follow [`plan-refactor-refresh-ui.md`](./plan-refactor-refresh-ui.md) end-to-end. The eight sub-stages in that plan's §7 ("Sequencing") map to commits on this branch:

1. Variable rename + theme store rewrite
2. Adopt shared modal classes
3. Toolbar restyle
4. Settings layout (top tabs)
5. UI scale + zoom shortcuts
6. Custom titlebar
7. Sync UI (drawer modal + status dot)
8. About modal + small polish

Budget: ~2 weeks. Each sub-stage is independently mergeable; ship them to `main` as they land if you'd rather not carry a long-running branch.

**Done when:** ThreadLiner side-by-side with NoteLiner looks like a sibling app. The two visual languages should be indistinguishable.

**Final commit (or merge):** `feat(threadliner): adopt NoteLiner visual language (refresh-ui plan)`

---

## Step 6.5 — Unify ThreadLiner's dev script with NoteLiner's pattern

Goal: bring ThreadLiner's dev workflow in line with NoteLiner's so both apps run via `npm run electron:dev -w <app>`. Eliminates the README-documented asymmetry and sets up the shared dev infra that can move into the library in Step 7.

### Why

- ThreadLiner's current `dev` script (`vite build && vite build --watch & electron .`) rebuilds to disk and requires a manual `Ctrl+R` in the Electron window after every renderer change. NoteLiner's `scripts/dev.js` runs the Vite dev server with HMR — instant updates, no reload.
- After Steps 5 + 6, the two apps share the same stack and visual language. Dev orchestration is the last meaningful inconsistency before Step 7 turns the shared bits into a library.

### 6.5.1 Port `scripts/dev.js` to ThreadLiner

Copy `apps/noteliner/scripts/dev.js` → `apps/threadliner/scripts/dev.js`. Adjustments:

- Drop the `--class=NoteLiner` Linux WM hint (or replace with `--class=ThreadLiner` if desired).
- Vite port: NoteLiner uses 5250 (set in its `vite.config.mjs` `server.port` and hardcoded into `main.js`). ThreadLiner's vite config has no `server` block today — set it to 5251 so both apps can run simultaneously without a port collision.

### 6.5.2 Teach ThreadLiner's main process to load the dev server

`apps/threadliner/src/main/main.js` currently has:

```js
const indexPath = path.join(__dirname, '../../dist/renderer/index.html');
mainWindow.loadFile(indexPath);
```

Switch to NoteLiner's pattern: gate on `NODE_ENV === 'development'` (set by `scripts/dev.js` when it spawns Electron) and `loadURL` the dev server URL in dev, `loadFile` the built `index.html` otherwise.

```js
const isDev = process.env.NODE_ENV === 'development';
if (isDev) {
  mainWindow.loadURL('http://localhost:5251');
} else {
  mainWindow.loadFile(path.join(__dirname, '../../dist/renderer/index.html'));
}
```

### 6.5.3 Update ThreadLiner's `package.json` scripts

```jsonc
"scripts": {
  "dev":          "vite",
  "build":        "vite build",
  "electron:dev": "node scripts/dev.js",
  "start":        "electron ."
}
```

Drop the old `dev:renderer` and `dev:electron` — they're redundant once `electron:dev` exists.

### 6.5.4 Add a Vite server config to ThreadLiner

`apps/threadliner/vite.config.mjs`:

```js
server: {
  port: 5251,
  strictPort: true,
},
```

`strictPort` is important — if 5251 is busy the run should fail loudly instead of silently falling back to a different port that `main.js` doesn't know about.

### 6.5.5 Optional: add root shortcuts

```jsonc
"electron:noteliner":   "npm run electron:dev -w noteliner",
"electron:threadliner": "npm run electron:dev -w threadliner"
```

Keep `dev:noteliner` / `dev:threadliner` as the renderer-only escape hatch (handy for iterating on Svelte components without Electron overhead).

### 6.5.6 Update README

Both apps now have identical dev instructions modulo app name. Collapse the per-app sections in `README.md`'s "Run in development" into one block.

**Done when:** `npm run electron:dev -w threadliner` launches Vite on 5251 + Electron in one command, with HMR working on save. Both apps can run simultaneously without port collision.

**Commit:** `chore(threadliner): unify dev script with noteliner pattern`

---

## Step 7 — Carve out `packages/desktop-ui/`

Branch: `step-7/desktop-ui-library`.

### 7.1 Skeleton

```bash
cd ~/dev/hub/laan/laankit
mkdir -p packages/desktop-ui/src/{theme,components,stores,electron-host,preload,styles}
mkdir -p packages/desktop-ui/examples/playground
```

### 7.2 `packages/desktop-ui/package.json`

```jsonc
{
  "name": "@laankit/desktop-ui",
  "version": "0.1.0",
  "private": true,
  "type": "module",
  "svelte": "./src/index.js",
  "exports": {
    ".":                 { "svelte": "./src/index.js" },
    "./styles":          "./src/styles/global.css",
    "./theme":           "./src/theme/index.js",
    "./components":      "./src/components/index.js",
    "./command-palette": "./src/command-palette/index.js",
    "./electron-host":   "./src/electron-host/index.js",
    "./preload":         "./src/preload/index.js"
  },
  "peerDependencies": {
    "svelte":   "^5.0.0",
    "electron": ">=28"
  }
}
```

### 7.3 Copy shared code from NoteLiner

Source paths refer to `apps/noteliner/src/renderer/`.

| From | To |
|---|---|
| `styles/global.css` | `packages/desktop-ui/src/styles/global.css` |
| `stores/theme.svelte.js` | `packages/desktop-ui/src/theme/index.js` |
| `stores/commands.svelte.js` | `packages/desktop-ui/src/command-palette/registry.js` |
| `components/TitleBar.svelte` | `packages/desktop-ui/src/components/TitleBar.svelte` |
| `components/Toolbar.svelte` (chrome parts) | `packages/desktop-ui/src/components/Toolbar.svelte` + `ToolbarButton.svelte` |
| `components/AboutModal.svelte` | `packages/desktop-ui/src/components/AboutModal.svelte` |
| `components/CommandPalette.svelte` | `packages/desktop-ui/src/command-palette/CommandPalette.svelte` |
| `components/SettingsModal.svelte` (split) | `SettingsShell.svelte` + per-tab components |

For NoteLiner-specific tabs (MCP, write-frontmatter, etc.), **don't** move them into the library — they stay in `apps/noteliner/` and register into `SettingsShell` as custom tabs.

Main-process bits from `apps/noteliner/src/main/main.js`:

- Window IPC handlers (`window:minimize`, `:maximize`, `:close`, `:isMaximized`)
- UI prefs file I/O (`ui:getPrefs`, `ui:setPrefs`)
- App relaunch handler (`app:relaunch`)
- `frame: !uiPrefs.customTitlebar` helper

These move into `packages/desktop-ui/src/electron-host/index.js` as exported helper functions per the foundation plan's §"Electron-host helpers".

Preload bits from `apps/noteliner/src/main/preload.js`:

- `windowMinimize`, `windowMaximize`, `windowClose`, `windowIsMaximized`
- `onWindowMaximizedChange`
- `getUIPrefs`, `setUIPrefs`
- `relaunchApp`

Move to `packages/desktop-ui/src/preload/index.js` as `exposeWindowApi()` and `exposeUIPrefsApi()` factories.

### 7.4 Build the playground

`packages/desktop-ui/examples/playground/` is a standalone Electron + Vite app that imports `@laankit/desktop-ui` and renders each component under each theme × scale. It exists to exercise the library without needing either consumer app.

Minimum viable playground:

- `package.json` declaring `electron` + `vite` + workspace dep on `@laankit/desktop-ui`
- A single window that renders TitleBar, Toolbar, AboutModal, SettingsShell with the built-in tabs, and a theme switcher

### 7.5 Validate

```bash
cd ~/dev/hub/laan/laankit
npm install                                       # links the new workspace
npm run dev -w @laankit/desktop-ui/examples/playground
```

Cycle every theme and scale; confirm every exported component renders.

**Done when:** the playground app exercises every library export without crashing, with no dependency on `apps/noteliner/` or `apps/threadliner/`.

**Commit (or merge):** `feat(desktop-ui): initial library carve-out from NoteLiner`

---

## Step 8 — Convert NoteLiner to consume the library

Branch: `step-8/noteliner-on-desktop-ui`.

### 8.1 Add dependency

`apps/noteliner/package.json`:

```jsonc
"dependencies": {
  "@laankit/desktop-ui": "*",
  // ... rest unchanged
}
```

`npm install` at root will link the workspace.

### 8.2 Replace local imports

In `apps/noteliner/src/renderer/`:

```js
// Before
import TitleBar from './components/TitleBar.svelte';
import { themeState } from './stores/theme.svelte.js';
import './styles/global.css';

// After
import { TitleBar } from '@laankit/desktop-ui/components';
import { themeState } from '@laankit/desktop-ui/theme';
import '@laankit/desktop-ui/styles';
```

Delete the now-duplicated local files. Domain components (FileTree, OutlinePane, TagsPane, Editor, Preview, etc.) stay local.

### 8.3 Replace local IPC handlers in main

In `apps/noteliner/src/main/main.js`:

```js
import { registerWindowHandlers, registerUIPrefsHandlers, registerRelaunchHandler, applyFrameFromPrefs }
  from '@laankit/desktop-ui/electron-host';

// Replace ~120 lines of inline handlers with:
registerWindowHandlers({ getWindow: () => mainWindow });
registerUIPrefsHandlers({
  prefsPath: path.join(app.getPath('userData'), 'ui-preferences.json'),
  defaults: { customTitlebar: false, writeFrontmatter: true, mcpEnabled: false, mcpConfirmWrites: false, mcpDisabledTools: [] },
});
registerRelaunchHandler();
// And when creating BrowserWindow:
const opts = applyFrameFromPrefs({ width: 1200, height: 800, ... }, uiPrefs);
```

### 8.4 Validate

```bash
npm run dev:noteliner
```

- Theme switching works
- UI scale works
- Custom titlebar toggle works (with restart banner)
- All sidebar panes render
- Sync modal, settings modal, about modal all render identically to before
- Playwright suite passes:
  ```bash
  npm test -w noteliner
  ```

Visual-diff against pre-step-8 screenshots — any drift gets fixed in the *library*, not in NoteLiner.

**Done when:** NoteLiner runs identically to before, with library imports replacing every local copy.

**Commit (or merge):** `refactor(noteliner): consume @laankit/desktop-ui for chrome and theming`

---

## Step 9 — Convert ThreadLiner to consume the library

Branch: `step-9/threadliner-on-desktop-ui`.

Same routine as Step 8. ThreadLiner's custom tabs (Sync activity, future Feed Defaults) register as `SettingsShell` contributions.

### 9.1 Add dependency

```jsonc
// apps/threadliner/package.json
"dependencies": {
  "@laankit/desktop-ui": "*",
  // ...
}
```

### 9.2 Replace local imports + IPC handlers

Mirror Step 8.2 + 8.3 for ThreadLiner.

### 9.3 Validate

```bash
npm run dev:threadliner
```

Exercise every feature; pixel-compare against the post-refresh-UI build.

**Done when:** ThreadLiner runs identically to its post-Step-6 state, with library imports replacing every local copy.

**Commit (or merge):** `refactor(threadliner): consume @laankit/desktop-ui for chrome and theming`

---

## Step 10 — Library hardening + v1.0

### 10.1 Snapshot tests in the playground

Per component × theme × scale. Tools:

- Playwright (already in NoteLiner; lift to root)
- `await expect(page).toHaveScreenshot()` for each cell of the matrix

Pin the matrix in `packages/desktop-ui/tests/visual.spec.js`.

### 10.2 Document supported versions

`packages/desktop-ui/README.md`:

- Required Svelte version
- Required Electron version range
- Required Node version
- API stability promise

### 10.3 API freeze

Lock the public surface area. Anything not in `src/index.js` or the `exports` map is internal and may change.

### 10.4 Tag v1.0

```bash
cd ~/dev/hub/laan/laankit
git tag -a desktop-ui-v1.0.0 -m "@laankit/desktop-ui 1.0.0"
git push origin desktop-ui-v1.0.0   # if pushing to GitHub
```

**Done when:** the playground snapshot tests pass, the README documents constraints, and the tag exists.

---

## Pause points (where to stop and still have value)

| After step | What you have |
|---|---|
| 4 | Working monorepo, common tooling, both apps build green |
| 5 | ThreadLiner on the same stack as NoteLiner; ready for visual refresh |
| 6 | ThreadLiner visually matches NoteLiner; library work optional |
| 6.5 | Both apps share the same dev workflow (`electron:dev` + HMR) |
| 7 | Library buildable in isolation, not yet consumed |
| 9 | Both apps thinner; library proven in production |
| 10 | Stable, tagged library; foundation effort complete |

The most natural "ship and rest" points are after Step 6 (visual unification done; users see the win) and after Step 9 (refactor done; maintenance gets cheaper).

## When to stop entirely

If after Step 5 + Step 6 you decide the library carve-out isn't worth the cost (e.g., you no longer plan to maintain both apps actively), stop there. You'll have a unified visual language across both apps without the maintenance contract of a shared library. That's a legitimate end-state for this work.

## References

- [`plan-refactor-summary.md`](./plan-refactor-summary.md) — three-plan comparison + recommendation
- [`plan-refactor-refresh-ui.md`](./plan-refactor-refresh-ui.md) — detailed spec for Step 6
- [`plan-refactor-common-foundation.md`](./plan-refactor-common-foundation.md) — detailed spec for Steps 7–10
- [`plan-refactor-modular-app.md`](./plan-refactor-modular-app.md) — future direction, evaluate after Step 10
