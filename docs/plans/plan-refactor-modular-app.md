# Plan: One Modular App — Installable NoteLiner and ThreadLiner Modules

Status: Draft for review Working name: **LaanKit** (single Electron app, two installable modules)

## Goal

Collapse NoteLiner and ThreadLiner into a single Electron application whose features are organised as modules the user can install, enable, disable, or uninstall at runtime. The shell looks and behaves like one product. Each module owns a domain (notes / RSS feeds / ...future), shows up in the toolbar when enabled, and can be removed without affecting other modules.

This is a strictly larger change than [the shared-foundation plan](./plan-refactor-common-foundation.md). That plan keeps two apps with a shared UI library. This plan eliminates the second app entirely.

## Vision

Boot the app cold and you see a chrome with no module-specific UI. Settings → Modules has an "Add module" row. Pick "NoteLiner" — a new toolbar group appears, the sidebar becomes available, the editor / preview panes mount on demand. Pick "ThreadLiner" too — a second toolbar group appears below the first, the sidebar gains a second sub-panel for feeds, the entry list and content viewer pane become available. Each module can be unticked and the UI shrinks back accordingly.

## Why this is interesting

- **One install for users who want both.** Today they'd ship two app bundles, two updaters, two icon-in-the-dock entries, two sets of settings, two windows.
- **One menu / chrome.** Title bar, theme, scale, settings, about, sync modal — all shared.
- **Cross-module features.** A note can deep-link to an RSS entry. A feed refresh can drop entries into the current project's notes.
- **Forces module discipline.** Each module is a clean package with a well-defined contract; that is the *real* refactor benefit.

## Why this is hard

- **Layout.** The two apps disagree about what fills the editor pane. NoteLiner's centre is an editor + preview + history + attachments; ThreadLiner's centre is an entry list + content viewer. The shell needs a "workspace" abstraction that one module owns at a time.
- **Storage models.** NoteLiner is a folder of `.md` files with a JSON index + git sync. ThreadLiner is a git-synced JSON store of feeds. Either each module owns its own data directory, or we converge on a single workspace folder that contains both.
- **Lifecycle.** Install → enable → load → IPC handlers register → UI contributions render. Disable / uninstall has to reverse all of that without restart.
- **Security.** Modules execute in the main process and have full privileges. We need either (a) trust = "we maintain all modules" or (b) sandboxing — out of scope for v1.

## Module contract

A module is a directory that exports a `manifest.js`:

```js
// modules/noteliner/manifest.js
export default {
  id: 'noteliner',
  name: 'NoteLiner',
  version: '0.7.1',
  description: 'Outliner-style notes',
  iconUrl: './icon.svg',

  // Main-process entry — runs in the main process when the module is enabled.
  main: './main/index.js',

  // Renderer entry — Svelte components + stores + contributions.
  renderer: './renderer/index.js',

  // Preload entry — adds to the shared preload API.
  preload: './preload/index.js',

  // What this module needs from the shell.
  dependencies: {
    shell: '^1.0.0',
    'workspace-folder': true,        // requires a per-module data dir
    'git-sync': true,                // wants the shared git-sync engine
  },

  // What this module contributes back.
  contributions: {
    workspaces: ['notes'],            // declares it can own the "notes" workspace
    toolbarGroups: ['file', 'view', 'navigation'],
    settingsTabs: ['notes', 'mcp'],
    commands: ['file.new', 'file.delete', /* ... */],
    sidebarPanes: ['files', 'tags', 'outline', 'backlinks'],
    fileTypes: ['.md'],
  },
};
```

The shell loads enabled modules at boot, calls `manifest.main` (which returns a `MainContext` controller), and the renderer registers `manifest.renderer` contributions into the shared registries.

### Main-process API

```js
// modules/noteliner/main/index.js
export default function createMain({ shell, dataDir, log, getWindow, ipcRegistry }) {
  const projectService = new ProjectService(...);
  const gitService = new GitService(...);

  ipcRegistry.handle('noteliner:project:open',  (folder) => projectService.open(folder));
  ipcRegistry.handle('noteliner:file:read',     (id) => projectService.readFile(id));
  // ... all noteliner-specific IPC handlers, namespaced with `noteliner:` prefix.

  return {
    enable()  { /* mount git watcher, MCP server, etc. */ },
    disable() { /* unmount, but don't delete data */ },
    uninstall({ wipeData }) { /* unregister IPC, optionally delete dataDir */ },
  };
}
```

### Renderer API

```js
// modules/noteliner/renderer/index.js
import FileTree from './components/FileTree.svelte';
import Editor   from './components/Editor.svelte';
import Preview  from './components/Preview.svelte';

export default function registerModule({ shell }) {
  shell.registerWorkspace({
    id: 'notes',
    label: 'Notes',
    icon: 'fa-file-lines',
    component: NotesWorkspace,        // a Svelte component that hosts editor/preview/history
    onActivate: () => shell.commandRegistry.activateGroup('notes'),
  });

  shell.registerToolbarGroup({
    id: 'notes.file',
    workspace: 'notes',
    buttons: [
      { id: 'file.new',    icon: 'fa-file-circle-plus', shortcut: 'Ctrl+N', run: () => ... },
      { id: 'file.import', icon: 'fa-file-import',     shortcut: 'Ctrl+Shift+I', run: () => ... },
    ],
  });

  shell.registerSidebarPane({ id: 'notes.files',    workspace: 'notes', component: FilesPane,   defaultHeight: 200 });
  shell.registerSidebarPane({ id: 'notes.tags',     workspace: 'notes', component: TagsPane,    defaultHeight: 120 });
  shell.registerSidebarPane({ id: 'notes.outline',  workspace: 'notes', component: OutlinePane, defaultHeight: 150 });
  shell.registerSidebarPane({ id: 'notes.backlinks',workspace: 'notes', component: BacklinksPane,defaultHeight: 180 });

  shell.registerSettingsTab({ id: 'notes',          label: 'Notes', component: NotesSettings });
  shell.registerSettingsTab({ id: 'noteliner.mcp',  label: 'MCP',   component: McpSettings });

  shell.registerSyncEngine({
    id: 'notes-git',
    workspace: 'notes',
    push:    () => window.api.invoke('noteliner:git:push'),
    pull:    () => window.api.invoke('noteliner:git:pull'),
    status:  () => window.api.invoke('noteliner:git:getSyncStatus'),
  });
}
```

## Shell architecture

```
apps/laankit/
├── src/main/
│   ├── shell.js                    bootstraps Electron + module host
│   ├── module-host.js              install/enable/disable lifecycle, IPC namespacing
│   ├── module-registry.js          on-disk list of installed modules
│   └── workspace-manager.js        which workspace is currently active
├── src/renderer/
│   ├── App.svelte                  shell chrome (titlebar, toolbar, sidebar, body, statusbar)
│   ├── shell-context.svelte.js     contribution registries (commands, panes, toolbar groups, ...)
│   ├── WorkspaceHost.svelte        renders the active workspace's component
│   └── components/
│       ├── ModuleManager.svelte    install / enable / disable / uninstall UI
│       ├── SettingsModal.svelte    aggregates tabs from all modules
│       └── ... (chrome from desktop-ui library)
├── src/preload/
│   └── preload.js                  loads enabled modules' preload contributions
└── modules/
    ├── noteliner/                  full noteliner-as-a-module
    └── threadliner/                full threadliner-as-a-module
```

## UX walkthroughs

### Cold boot, no modules

- Title bar: "LaanKit"
- Toolbar: only the utility group (settings, about, help)
- Sidebar: empty placeholder
- Body: "No workspace active. Open Settings → Modules to add one."

### NoteLiner module installed and enabled

- Toolbar gains the notes file/view groups.
- Sidebar gains the four notes panes (files, tags, outline, backlinks), toggleable individually.
- Body shows the editor + preview workspace.
- Settings gains a "Notes" tab and an "MCP" tab.

### Both modules enabled

- Sidebar shows panes from whichever workspace is active. A workspace switcher (small tab strip above the sidebar? toolbar group? command palette?) flips between Notes and Feeds.
- Toolbar groups for the inactive workspace are visually de-emphasised (greyed) but still clickable — clicking a Feeds button switches the active workspace to Feeds first.
- Theme/scale/sync controls are shared.

### Disabling a module

- The module's IPC handlers unregister.
- Sidebar panes / toolbar groups / settings tabs disappear from the UI.
- Data folder is retained; re-enabling re-attaches.
- Active workspace falls back to the next-most-recently-used one.

### Uninstalling

Two prompts:
1. "Disable and remove from the shell?" → Yes.
2. "Also delete data folder at `<path>`?" → defaults to No.

After uninstall, the module package is removed from `modules-installed.json` and (optionally) from disk.

## Data model

```
~/.config/laankit/
├── settings.json             theme, scale, customTitlebar, lastWorkspace
├── modules-installed.json    [{ id, version, enabled, dataDir }]
├── module-data/
│   ├── noteliner/<some-project-or-vault>
│   └── threadliner/feeds-store/
└── window-state.json
```

Each module gets one or more data directories under `module-data/<id>/`. Modules may declare additional dirs in their manifest.

## IPC discipline

Every module-introduced IPC channel must be prefixed with its module id (`noteliner:`, `threadliner:`). The shell rejects unprefixed handlers. Shell-owned channels (`window:*`, `ui:*`, `app:*`, `theme:*`, `shell:*`) are reserved.

## Module distribution

For v1: modules ship *in-tree* with the app — installing means flipping a flag in `modules-installed.json`. There is no out-of-tree marketplace, no signed packages, no third-party modules. This keeps the security model simple ("everything we ship is trusted") while still exercising the full module API for our own code.

For v2 (out of scope): consider out-of-tree modules loaded from a `~/.laankit/modules/` directory. That introduces a code-signing requirement and a permission model — both substantial.

## Migration from existing apps

### Step-by-step

1. **Foundation first.** Land `@laankit/desktop-ui` per the [shared-foundation plan](./plan-refactor-common-foundation.md). The shell is built on top of that library.
2. **Create the shell skeleton.** `apps/laankit/` with the chrome from `@laankit/desktop-ui` and the module host stubbed.
3. **Define the contribution registries.** Toolbar groups, sidebar panes, workspaces, settings tabs, commands, sync engines, IPC namespacing.
4. **Port NoteLiner into `modules/noteliner/`.**
   - Move main-process services under the module's `main/`.
   - Rewrite `App.svelte`'s mounting logic as `registerModule(shell)` calls.
   - Namespace every IPC channel.
   - Persist data under `module-data/noteliner/`.
5. **Smoke-test as a single-module app.** Should be feature-parity with current NoteLiner.
6. **Port ThreadLiner into `modules/threadliner/`.**
   - Same routine. Workspace id: `feeds`.
   - Sync engine adapts the existing auto-push model to the shell's sync registry contract.
7. **Add the Module Manager UI.** Settings → Modules: list installed modules, enable/disable toggles, uninstall, "Add module" picker that in v1 only shows in-tree modules.
8. **Cross-module touches (optional, v1.x).**
   - Notes can `[[feed:<id>]]`-link into an RSS entry.
   - Command palette shows commands from both modules.
   - Theme stays shared.
9. **Deprecate** the standalone NoteLiner and ThreadLiner binaries. Ship the unified bundle.

### Backwards-compatibility for existing users

- Existing NoteLiner users have `~/.config/Electron/...` data. On first LaanKit boot with the NoteLiner module enabled, prompt to import recent projects (we already store these as JSON pointers — no data copying needed).
- Existing ThreadLiner users have a `dataDir` config. Same prompt to import.
- We do *not* move user data — we just point the module at the existing location.

## Open design questions

These need a decision before implementation, in roughly this order.

| # | Question | Default proposal |
|---|---|---|
| 1 | Single window or multi-window? | Single. Multi-window can come later via `shell.openWorkspaceInWindow(id)`. |
| 2 | Workspace switcher placement? | Toolbar top-of-list as a workspace pill; command palette `Ctrl+P`. |
| 3 | Can two workspaces show side-by-side? | Not in v1. One active at a time. |
| 4 | Where do module preferences live? | In module data dir, namespaced. Shared prefs (theme/scale) live in shell prefs. |
| 5 | Does sync run per-module or per-workspace? | Per workspace. Each workspace declares a sync engine. |
| 6 | How do modules ship Font Awesome icons? | Shell ships FA; modules reference icon names only. |
| 7 | Are module IDs reserved? | Yes, in-tree IDs are reserved (`noteliner`, `threadliner`, `git-sync`). |

## Risks

- **Scope.** This is a 2–3 month project, not a 2–3 week one. The shared-foundation plan delivers ~70% of the visual unification benefit at ~25% of the cost.
- **Coupling.** Modules sharing chrome but not internals is hard to get right — the registry contracts are the load-bearing piece. Get them wrong and the shell becomes a worse version of each app.
- **Performance.** Two modules in one process = two file watchers, two git engines, possibly an MCP server. The current single-app footprints add up.
- **Branding.** "NoteLiner" and "ThreadLiner" are recognisable. The unified shell needs its own identity, or the modules need to remain marketed as separate things even though they share an installer.
- **Reversibility.** Once consumers migrate to the unified app, going back to two standalone apps is awkward (data migration, two installers again). Worth doing the foundation work first and only committing to the modular app once the shared library is healthy.

## Effort estimate

| Stage | Calendar |
|---|---|
| Foundation library (prerequisite) | 3–4 weeks |
| Shell skeleton + module host | 2–3 weeks |
| NoteLiner-as-module port | 2 weeks |
| ThreadLiner-as-module port | 1–2 weeks |
| Module manager UI + polish | 1 week |
| QA + migration story | 1 week |

Total: **~10–13 weeks of focused work**, conditional on the foundation landing first.

## Out of scope

- Out-of-tree / third-party modules
- Sandboxed modules
- Module signing / verification
- Cross-module API beyond what each module's manifest declares
- A plugin marketplace
- Module hot-reload during development (nice-to-have, not v1)
