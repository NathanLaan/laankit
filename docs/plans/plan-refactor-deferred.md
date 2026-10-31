# Plan: LaanKit foundation — deferred and follow-up items

Status: Active. Tracks the work that wasn't part of the
[`plan-refactor-steps.md`](./plan-refactor-steps.md) execution path but
either blocks calling the foundation effort "done" or was explicitly
deferred to a later pass.

## Where we are

Steps 4–10 from `plan-refactor-steps.md` are complete:

- Monorepo with `apps/noteliner`, `apps/threadliner`, `packages/desktop-ui`,
  and `packages/desktop-ui/examples/playground`.
- Both apps consume `@laankit/desktop-ui` for chrome and theming.
- Library has a Playwright snapshot harness scaffolded.
- Library API documented; `package.json` `exports` map is the SoT.

Confirm with:

```bash
cd ~/dev/hub/laan/laankit
npm run build         # all workspaces green
ls packages/desktop-ui/tests/visual.spec.js  # snapshot harness exists
```

## Phase map

| Phase | Theme | Status |
|---|---|---|
| A | Blocking completion (Step 10 finish-up) | Pending user |
| B | Small polish (caught in passing during 4–10) | Pending |
| C | Explicit deferrals from `plan-refactor-refresh-ui.md` §11 | Pending |
| D | Future direction | Evaluate after A–C |

## Phase A — Blocking completion

These four items are what stand between "Step 10 mechanically done" and
"foundation effort declared complete." None take more than a few minutes.

### A.1 Generate snapshot baselines

The Playwright harness is wired up but no baseline images exist yet.
Run once from a workstation that can actually launch Electron:

```bash
npm run test:update -w @laankit/desktop-ui
```

Commit the resulting PNGs under
`packages/desktop-ui/tests/visual.spec.js-snapshots/`. Subsequent
`npm test -w @laankit/desktop-ui` runs compare against them with a 0.2%
pixel-diff tolerance.

**Done when:** `tests/visual.spec.js-snapshots/` exists in git with one
PNG per test (7 files at current matrix size).

**Commit:** `test(desktop-ui): pin visual regression baselines`

### A.2 Verify the NoteLiner Playwright suite still passes

Step 8 swapped NoteLiner's TitleBar, AboutModal, Toolbar, SettingsModal,
CommandPalette, theme store, global stylesheet, and the window/UI-prefs
IPC surface for library equivalents. The existing e2e suite under
`apps/noteliner/tests/e2e/` was last green pre-Step 8. Run it:

```bash
npm test -w noteliner
```

Any failures live in NoteLiner consumption code (not the library) and
get fixed there. The library is at the same revision the playground
tests are pinned against, so it's the right reference.

**Done when:** `npm test -w noteliner` is green on the post-Step-8 head.

**Commit:** any fix-forward needed; no commit if it's already passing.

### A.3 Fix the `productName` typo in ThreadLiner

`apps/threadliner/package.json` still says:

```json
"productName": "Threadline"
```

The npm package + everywhere else in this repo is **ThreadLiner**.
electron-builder uses `productName` for installer titles, app bundle
names, etc.; leaving it wrong means the next packaged build ships with
the old name. One-line fix.

**Done when:** `productName` reads `"ThreadLiner"` and a quick
`npm run build:threadliner` is green.

**Commit:** `fix(threadliner): correct productName spelling`

### A.4 Tag `desktop-ui-v1.0.0`

Per the README's "Cutting a release" section:

```bash
git tag -a desktop-ui-v1.0.0 -m "@laankit/desktop-ui 1.0.0"
git push origin desktop-ui-v1.0.0
```

Bump `packages/desktop-ui/package.json` `version` from `0.1.0` → `1.0.0`
in the same commit you tag, so the version field matches the tag.

**Done when:** the annotated tag exists locally and on origin (if you
push to GitHub).

**Commit:** `release(desktop-ui): tag v1.0.0`

## Phase B — Small polish

Items I noticed while doing 4–10 that are small enough to land
individually. None affect correctness; each is one commit.

### B.1 Reload in-memory state after `git:resetToRemote` (ThreadLiner)

`apps/threadliner/src/renderer/components/SyncModal.svelte`'s
`handleResetFromRemote` calls `window.api.gitResetToRemote()` which
overwrites the on-disk data dir with the remote contents — but the
renderer's `feeds` / `entries` / `tags` stores still hold the pre-reset
data. Visible symptom: user resets, modal closes, sidebar still shows
the old feeds until app restart.

Fix: after `gitResetToRemote()` succeeds, dispatch `loadFeeds()`,
`loadTags()`, and `selectFeed(null)` from `stores/app.js`. Roughly:

```js
import { loadFeeds, loadTags, selectFeed } from '../stores/app.js';

async function handleResetFromRemote() {
  // ...existing code...
  try {
    await window.api.gitResetToRemote();
    selectFeed(null);
    await loadFeeds();
    await loadTags();
    await refreshStatus();
  } catch (err) { ... }
}
```

**Done when:** reset is followed by a visible refresh of the feeds list
without restart.

**Commit:** `fix(threadliner): refresh in-memory state after reset from remote`

### B.2 Optional theme allowlist for ThreadLiner

The library exposes 6 themes (light / dark / midnight / darkPurple /
lightPurple / lightMulberry). ThreadLiner had 3 (light / dark /
midnight) before Step 9; the new options work fine but expose theming
ThreadLiner didn't ship before.

If you'd rather keep ThreadLiner at 3, add an allowlist option to the
library's `themeState.configure({ themes: [...] })`. Sketch:

```js
// packages/desktop-ui/src/theme/index.svelte.js
configure({ appId, themes } = {}) {
  if (appId) this.appId = appId;
  if (Array.isArray(themes)) this.themeAllowlist = themes;
}

get list() {
  const ids = this.themeAllowlist || Object.keys(THEMES);
  return ids
    .filter((id) => THEMES[id])
    .map((id) => ({ id, name: THEMES[id].name }));
}
```

Then in `apps/threadliner/src/renderer/main.js`:

```js
themeState.init({
  appId: 'threadliner',
  themes: ['light', 'dark', 'midnight'],
});
```

**Done when:** ThreadLiner's Settings → UI → Theme list shows three
themes; NoteLiner still shows six.

**Commit:** `feat(desktop-ui): add theme allowlist; pin threadliner to 3`

### B.3 Bundle the preload to drop the `sandbox: false` requirement

**Status: done.** esbuild bundles each app's preload into
`dist/preload.cjs` via a `bundle:preload` npm script (run as a `prebuild`
hook and from each `scripts/dev.js` orchestrator before Electron starts).
All three apps now run with Electron's default sandbox enabled. Library
preload header + README updated to describe the bundling pattern as the
primary path with `sandbox: false` as a prototyping fallback.

## Phase C — Explicit deferrals from `plan-refactor-refresh-ui.md` §11

**Status: done.** All three items landed. Summaries kept below as a paper
trail; jump to the source files for the current shape.

### C.1 Auto-updater for ThreadLiner — done

Ported NoteLiner's `update.svelte.js` store verbatim to
`apps/threadliner/src/renderer/stores/`. Added `electron-updater` to
deps. `apps/threadliner/src/main/main.js` now has the autoUpdater
plumbing (`getAutoUpdater`, `sendUpdateState`, `initAutoUpdater`) and
the four IPC handlers (`update:getState`, `update:checkNow`,
`update:downloadNow`, `update:installNow`). Preload exposes
`getUpdateState`, `checkForUpdates`, `downloadUpdate`, `installUpdate`,
`onUpdateState`. `App.svelte` passes `updateState` into `<AboutModal>`.

`app.isPackaged` gates the startup check, so the dev experience is
unchanged. When a packaged build runs, the About modal shows the
Check / Download / Install buttons fed by electron-updater.

The actual update feed (`build.publish` in electron-builder config) is
not wired — there's no ThreadLiner release channel yet. Add when ready
to ship.

### C.2 Paneable / reorderable sidebar — done

Extracted `PaneHost` to `@laankit/desktop-ui/panels`. The library owns
the layout chrome (header bar, drag-to-reorder, drag-to-resize,
ResizeObserver-based height clamping); consumers provide pane content
via `{ id, title, height, render: snippet, headerExtra?, closable? }`
descriptors plus an `order` array and callbacks for resize/reorder/close.

NoteLiner's `Sidebar.svelte` shrank from 454 to ~210 lines; pane
content moved into local snippets, layout flows through `<PaneHost>`.
ThreadLiner's sidebar is unchanged (Feeds + Tags is structurally
simple; doesn't benefit from PaneHost).

### C.3 Help window pattern — done (option B)

Extracted `createSecondaryWindow` / `getSecondaryWindow` /
`closeSecondaryWindow` to `@laankit/desktop-ui/secondary-window`. The
helper owns the singleton "focus if open, otherwise create" registry,
sandbox-compatible webPreferences defaults, dev/prod URL switching,
and 'closed' cleanup.

NoteLiner's `createHelpWindow` migrated to use the helper (consumes
its own dogfood). ThreadLiner gained a help window: new `help.html`,
`help.js`, `HelpApp.svelte` (ThreadLiner-flavoured help with
Getting-started, Tags, Sync, Settings sections), `vite.config.mjs`
updated with a second rollup entry, `help:open` IPC handler, preload
binding, and a Help button at the bottom of the toolbar
(`fa-circle-question`).

## Phase D — Future direction

### D.1 Evaluate `plan-refactor-modular-app.md`

That plan exists in `docs/plans/` as the "evaluate after Step 10" item
in the foundation plan's references section. I haven't read it in
detail. With Phase A done and the library tagged, that's the right
moment to read it cold and decide whether it's worth pursuing.

The summary plan (`plan-refactor-summary.md`) compares the three
plans (foundation, refresh-UI, modular-app) — start there.

**Done when:** you've made a yes/no/modify decision on modular-app and
either deleted the plan, archived it, or promoted it to active.

## Awareness items (no action required)

Things to know about but not necessarily fix:

- **The two `.cjs` files in the library.** `electron-host/index.cjs` and
  `preload/index.cjs` are CommonJS because they run in Electron's main /
  preload contexts which use `require()`. The package is otherwise
  `"type": "module"`. Working as intended.
- **Custom `app:relaunch` handlers in both apps + the playground.** The
  library's `registerRelaunchHandler` does `app.relaunch(); app.exit(0);`
  which tears down `scripts/dev.js` along with Electron, killing Vite.
  Each app registers a window-recreate handler instead. The library's
  helper still works for packaged builds but is unused in this repo.
- **`themeState.hydrateFromSettings` + auto-mirror-to-settings on `set`.**
  ThreadLiner uses this (git-synced data dir). NoteLiner doesn't expose
  `window.api.setSetting`, so the optional-chain guards skip cleanly.
  Library-side both methods exist generically; no consumer-side opt-in
  needed.

## Pause points

| After phase | What you have |
|---|---|
| A | Foundation effort fully done; v1.0 tagged |
| B | Polish swept; both apps feel finished |
| C | Feature parity across the two apps to whatever degree you choose |
| D | Decision made on the next phase of architectural work |

The natural "ship and rest" point is **after Phase A**. Phases B–D are
opt-in and can land in any order — they don't depend on each other.

## References

- [`plan-refactor-steps.md`](./plan-refactor-steps.md) — completed execution plan (Steps 4–10)
- [`plan-refactor-common-foundation.md`](./plan-refactor-common-foundation.md) — library design (now implemented)
- [`plan-refactor-refresh-ui.md`](./plan-refactor-refresh-ui.md) — ThreadLiner visual refresh (now implemented); §11 lists what was deferred
- [`plan-refactor-modular-app.md`](./plan-refactor-modular-app.md) — future direction; evaluate in Phase D
- [`plan-refactor-summary.md`](./plan-refactor-summary.md) — three-plan comparison
