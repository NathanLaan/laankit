# ThreadLiner Native

ThreadLiner in pure Rust: an [egui](https://github.com/emilk/egui) UI over `threadliner-core`,
with no webview and no JavaScript. It's the Phase 2 pilot of plan Option C
([`docs/plans/plan-native-rust-egui.md`](../../../docs/plans/plan-native-rust-egui.md)) and runs on
Linux, Windows and macOS.

It reads and writes exactly what the Electron and Tauri builds do: the same
`~/.config/ThreadLiner` (`config.json`, `ui-preferences.json`) and the same git-synced data folder
(`data/feeds.json`, `data/tags.json`, `data/feeds/<id>/entries.json`,
`configuration/settings.json`), through the same `threadliner-core` store and sync engine. You can
switch between the builds at any time.

## Running

```bash
cargo run -p threadliner-native --release     # or: npm run native:threadliner
cargo test -p threadliner-native              # needs `git` on the PATH, like the app
```

The package is `threadliner-native`; its binary is `threadliner`, the one the release packages
install as `/usr/bin/threadliner` (the native build replaced the Tauri and Electron ones; see
[`docs/packaging.md`](../../../docs/packaging.md)).

- **First run** shows the setup dialog: pick (or create) the data folder and optionally a git remote.
- **Window chrome.** Settings → UI → Custom Window Titlebar switches between the system frame and
  Marina's title bar (shared with the Tauri build's `ui-preferences.json`; takes a restart).
- **Command palette.** Ctrl+Shift+P: every command, plus the feeds by name.

### Trying it without touching your setup

```bash
export XDG_CONFIG_HOME=$(mktemp -d)
cargo run -p threadliner-native --example threadliner_demo_data -- /tmp/tl-data \
  https://blog.rust-lang.org/feed.xml https://jvns.ca/atom.xml
cargo run -p threadliner-native --release
```

`threadliner_demo_data` creates a git-backed data folder with those feeds and two tags, and a `config.json`
under the temp `XDG_CONFIG_HOME` (Linux). On Windows the config folder is `%APPDATA%\ThreadLiner`
(set `APPDATA` to redirect it) and on macOS `~/Library/Application Support/ThreadLiner`, as
`marina_core::paths` mirrors Electron's `userData`.

## Architecture

| Module      | What it does                                                                                    |
|-------------|-------------------------------------------------------------------------------------------------|
| `main`      | The eframe window (1200 × 800, at least 800 × 500), icon, undecorated when the custom titlebar is on |
| `app`       | Owns everything; routes backend events, commands, pane actions and dialog results (`App.svelte` + `stores/*`) |
| `backend`   | A tokio runtime around `threadliner-core`: the Tauri host's channels, the poller, notifications; results come back as `Event`s |
| `config`    | `config.json` and `ui-preferences.json` in `~/.config/ThreadLiner`                              |
| `model`     | Typed feeds, entries and tags from the store's rows; the Svelte date formats                     |
| `commands`  | The shortcuts of `handleGlobalKeydown`, as marina-egui commands (shortcuts, palette, Settings list) |
| `views`     | Sidebar, entry list (virtualized), article viewer (marina-richtext), setup dialog, loading screen |
| `overlays`  | Add/Edit Feed, Remove Feed, Tags (+ Add/Edit/Delete Tag), Settings, Remote Sync, Help, About      |
| `ui_tests`  | egui_kittest: setup → add feed → read → mark unread → tag, against a local HTTP feed; settings file shape |

Nothing in `threadliner-core` knows about egui; everything UI-specific is in this crate or in
`marina-egui` / `marina-richtext`.

## Parity with the Svelte build

Every IPC channel of `src/main/preload.js` (as the Tauri host implements them) and the renderer's
features.

| Channel or feature                                         | Status          | Notes                                                                 |
|------------------------------------------------------------|-----------------|-----------------------------------------------------------------------|
| `setup:isComplete`, `setup:init`                           | Done            | Init, clone or adopt a repo, as the Tauri host                        |
| `setup:openFolderDialog`                                   | Done            | `rfd` (XDG portal on Linux, native dialogs on Windows/macOS)          |
| `feed:getAll`, `feed:add`, `feed:edit`, `feed:remove`      | Done            | Remove asks first, with the Svelte wording                            |
| `feed:refresh`                                             | Done            | F5, toolbar, title bar                                                |
| `entry:getByFeed`                                          | Done            | Newest first; the list is virtualized                                 |
| `entry:markRead` / `markUnread` / `markAllRead` / `markAllUnread` | Done     | Opening an entry marks it read; Ctrl+M / Ctrl+Shift+M act on the entry or the feed |
| `tag:getAll` / `add` / `edit` / `remove` / `assign` / `unassign` | Done      | Tags dialog plus the sidebar toggles                                 |
| `settings:get`, `settings:set`                             | Done            | Poll interval, notifications, sync wait time; theme and scale are mirrored like `themeState.set` |
| `poller:pollNow`, `feeds:updated`                          | Done            | Poller as in the Tauri host; "Check All Feeds Now" in the palette (the Svelte UI has no button) |
| New-entry notifications                                    | Done            | `notify-rust` (D-Bus on Linux, native on Windows/macOS); toggle in Settings → Feeds |
| `sync:getStatus`, `sync:getLog`                            | Done            | Polled every 2 s; the Sync button tints while a push is queued; Settings → Sync shows the log |
| `sync:forcePush`, `sync:forcePull`                         | Done            | "Sync Now" / "Pull from Remote" in the palette (the Svelte UI never calls them) |
| `sync:getConfig`                                           | Not needed      | The Svelte Sync modal loads it but never shows it                     |
| `git:getRemoteUrl` / `setRemoteUrl` / `removeRemote` / `getBranch` / `getSyncStatus` | Done | Remote Sync dialog: connect, disconnect, status dot and text |
| `git:pull` / `pullRebase` / `push` / `pushUpstream` / `resetToRemote` | Done | With the Svelte confirmations; the store reloads after pulls and resets |
| `help:open`                                                | Partial         | Same content, as an in-app dialog rather than a second window         |
| About and `update:*`                                       | Done            | `marina_core::updates`; ThreadLiner publishes no release repo, so it reports "latest", as the Tauri build does |
| `ui:getPrefs` / `ui:setPrefs` (custom titlebar)            | Done            | Restart now relaunches the binary                                     |
| Themes and UI scale                                        | Done            | Light, Dark, Midnight (ThreadLiner's allow-list); Ctrl+= / - / 0 and Ctrl+wheel |
| Keyboard shortcuts                                         | Done            | All of `handleGlobalKeydown`, ignored while typing; zoom also works with a dialog open |
| Command palette                                            | Native addition | Ctrl+Shift+P; commands plus feeds                                    |
| Resizable sidebar and entry list                           | Done            | 180–500 and 220–600 px, as the Svelte limits                          |
| Article view                                               | Partial         | marina-richtext with ThreadLiner's DOMPurify allow-list. The header stays above the scrolling body; selection is per paragraph; inline SVG, iframes and embeds are dropped (see `crates/marina-richtext/README.md`) |
| Window size and position                                   | Partial         | Remembered by eframe in its own storage, not the Tauri window-state file |
| Single instance                                            | Not yet         | A second launch opens a second window                                 |
| Error toasts                                               | Done            | marina-egui toasts, red for errors                                    |

## Diagnostics

| Variable                       | Effect                                                              |
|--------------------------------|---------------------------------------------------------------------|
| `MARINA_SCREENSHOT=<png>`      | Saves a screenshot once the window has settled (`MARINA_SCREENSHOT_EXIT=1` closes afterwards) |
| `MARINA_THEME`, `MARINA_SCALE` | Override the theme (`light`, `dark`, `midnight`) and scale          |
| `MARINA_FEED=<n>`, `MARINA_ENTRY=<n>` | Select the nth feed and entry once loaded                    |
| `MARINA_OPEN=<dialog>`         | `add`, `tags`, `settings[:ui\|feeds\|sync\|shortcuts]`, `sync`, `about`, `help` or `palette` |

On GNOME Wayland the compositor stops painting while the session is locked, so screenshots never
fire; use `WAYLAND_DISPLAY=` (XWayland) or an unlocked session.

## Still to verify by hand

- Windows and macOS: folder dialog, notifications, undecorated window chrome, keyboard shortcuts on
  non-US layouts.
- Linux: notifications under GNOME/KDE, the folder dialog without an XDG portal (minimal window managers).
- Connect, pull, push and reset against a real remote (the tests cover a local repo only).
