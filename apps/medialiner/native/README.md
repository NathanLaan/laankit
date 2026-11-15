# MediaLiner Native

MediaLiner's app: an [egui](https://github.com/emilk/egui) UI over `medialiner-core`, with no webview or JavaScript. Linux and Windows are supported; the macOS build comes from the shared packaging and isn't tested.

## Running

```bash
cargo run -p medialiner-native --release
cargo run -p medialiner-native --release -- ~/Pictures    # add and select a folder
cargo test -p medialiner-core -p medialiner-native
```

The package is `medialiner-native`; its binary is `medialiner`, the one the release packages install as `/usr/bin/medialiner`.

- Layout. From the left: the toolbar, the Folders pane (the tree), the Files pane, the grid or preview in the centre, and the Info pane on the right. The panes' widths are remembered, and each can be hidden.
- The arrow keys go where you last clicked. In the Folders tree, Up and Down move between folders and Left and Right collapse and expand. In the Files pane or the grid, they move the photo selection. In the preview, Left and Right step through the photos.
- Window chrome. Settings → UI → Custom Window Titlebar switches between the system frame and LaanKit's title bar (takes a restart).

## Keys

| Keys | Does |
|---|---|
| Ctrl+O | Add a folder |
| Up, Down, Left, Right | Move through the tree, the grid, or the preview (see above) |
| Home, End | The first or last photo |
| Enter, double-click | Open the preview |
| Esc, Backspace, Alt+Left | Back to the grid |
| Page Up, Page Down | The previous or next photo |
| Ctrl+= / Ctrl+- | Thumbnail size in the grid; zoom in the preview |
| 0, 1 | Fit, or 100%, in the preview |
| Ctrl+I | Show or hide the Info pane |
| Ctrl+B, Ctrl+Shift+F | Show or hide the Folders or Files pane |
| Ctrl+Shift+R | Open the selected photo in the file manager |
| Ctrl+Z | Undo the last merge |
| Ctrl+Shift+M | Import from a device |
| F5 | Refresh the selected folder and the tree |
| Ctrl+J | Toggle the status bar |
| Ctrl+K, Ctrl+,, Ctrl+Shift+I | Command palette, Settings, About |

Merge Into…, Remove Folder from List, Include Subfolders, Open With Default App, Copy Path, and Undo Last Import are in the command palette too.

## How it works

- The tree reads a folder's subfolders when you expand it, so a large library opens instantly. Hidden folders (names starting with `.`, and on Windows those with the hidden attribute) aren't shown, and links to folders aren't followed.
- Listings, file details, and merge plans run on one background thread; thumbnails and previews on up to four. The grid asks for the thumbnails on screen each frame, which replaces the waiting ones, so photos scrolled past are never decoded. Previews go first.
- A thumbnail is the photo turned by its EXIF orientation and scaled to 320 pixels on its long side. A JPEG's embedded EXIF thumbnail shows first when it has one, while the real one is made. The cache keys each by a hash of the photo's path, size, and modification time, so a changed photo gets a new one. Cached thumbnails not shown for 90 days are removed at start. About 600 thumbnails are kept on the GPU at once.
- The preview decodes the photo at up to 8192 pixels on a side, and the next and previous photos ahead.
- Open in Folder runs `explorer.exe /select,` on Windows. On Linux it asks the file manager through the `org.freedesktop.FileManager1` D-Bus interface (with `gdbus` or `dbus-send`), and opens the folder itself if that doesn't answer.

### Merging

`merge::plan` works out every move first: where each file goes, which names clash, and which files are identical (same size and bytes) to one already there. `merge::run` then moves the files one at a time:

- Each move is written to the journal (`merges/<time>.jsonl`) before it's made, so even a merge interrupted by a crash can be undone.
- Within a drive a move is a rename. Across drives it's a copy, a check of the copy's size, then a delete of the original, and the copy keeps the original's modification time.
- A name taken since the plan gets a new one at the time of the move. Files that can't move are skipped and listed in the log.
- Empty folders left in the source are removed, and the source itself when Remove once it's empty is on and nothing was skipped.
- Identical files go to `.medialiner-duplicates/<source name>/` in the source's listed folder (the destination's, when the source is itself a listed folder).

`merge::undo` moves each file back in reverse order, puts back the folders the merge removed, and removes the folders it made, if they're empty. A file moved or deleted since, or whose old place is taken, stays where it is. The last 10 journals are kept.

### Importing

A devices thread reads the list of drives and media devices every two seconds and reports what was added or removed; the ones there at the start aren't reported. A new device is listed on a thread of its own, and the Import dialog opens when it holds a photo or a video. A device that is, holds, or is inside a folder in your list isn't offered. If another dialog is open, a notice shows and the Import dialog opens when that one closes.

- `import::Source` is a device's files: `FolderSource` for a drive or a folder the desktop mounted, and on Windows `portable::PortableSource` for a media device. Listing leaves out hidden files and folders, `System Volume Information`, `$RECYCLE.BIN`, `LOST.DIR`, and the camera's own index files (`.thm`, `.ctg`, `.bdm`, `.cpi`, `.mpl`, `.bnp`, `.inp`, `.int`, and `.ind`).
- `import::run` copies into one folder, the device's own folders flattened. A file that's there with the same size and bytes is skipped. Otherwise the copy is written to a hidden `.name.pid.part` file, checked against the original's size, given the original's modification time, and put in place without replacing anything. Cancel stops within a file and removes the part file. When a copy fails and the device has gone, the rest are reported and not tried.
- `import::finish` adds the copied and already-there files to the device's record (`imports/device-<hash>.json`, keyed by the volume's UUID on Linux and serial number on Windows) and writes the journal (`imports/last-import.json`).
- With Delete from Device After Import, the import thread waits for your answer after the copy. Delete removes only files this import copied whose copies are still in place, and marks the journal so the import can't be undone.
- `import::undo` removes the journal's files that haven't changed since, and takes them out of the record so they count as new.
- The free space at the destination is checked in the dialog before Import is enabled.

## Architecture

`medialiner-core` (`crates/medialiner-core`) knows nothing about egui:

| Module | What it does |
|---|---|
| `library` | The folder list: adding (inside and around listed folders), removing, and which folder a path is in |
| `scan` | Subfolders and files, sorted naturally, without hidden ones; whole subtrees for Include Subfolders |
| `media` | Photo, video, or other, by extension |
| `natural` | Natural sorting ("Day 2" before "Day 10") |
| `thumbs` | Decoding the right way up, thumbnails, embedded EXIF thumbnails, and the cache |
| `info` | The Info pane's details and EXIF data, and sizes for people |
| `merge` | Plans, runs, journals, and undoes merges |
| `reveal` | Open in Folder on each platform |
| `devices` | The drives and media devices plugged in, on each platform |
| `import` | A device's files, the copy, the record of what has been imported, the journal, and undo |
| `portable` | Windows only: drive letters, and media devices through Windows Portable Devices |

The app (`apps/medialiner/native/src`):

| Module | What it does |
|---|---|
| `main` | The window, and a folder on the command line |
| `app` | All UI state; backend events, commands, view actions, dialogs, and the looks at the folder for changes |
| `backend` | The files thread, the picture threads and their queue, merges on their own thread, the devices thread, and device scans and imports on their own threads |
| `textures` | Thumbnails on the GPU (least recently used are freed) and the previews |
| `views` | Toolbar, Folders and Files panes, the grid, the preview, the Info pane, and the status bar |
| `overlays` | Import Photos, Merge Into…, Settings, About |
| `config` | `settings.json` and `ui-preferences.json` |
| `commands` | The commands and shortcuts, and where the arrow keys go |

The Folders tree is `laankit_egui::tree`, a widget with rows the app flattens, chevrons, drag and drop between rows, and a right-click menu.

## Tests

```bash
cargo test -p medialiner-core -p medialiner-native
```

- `medialiner-core`: `/proc/self/mountinfo` and media device names from fixed text, importing (what's listed from a card, clashes and files already there, cancelling, a failed copy, a device removed part way, the record, undo, and deleting from the device), natural sorting, the folder list's rules, scanning and hidden files, EXIF fields and orientation (from JPEGs made in the test with an EXIF block), thumbnails and the cache, Open in Folder's commands, and merges: clashes, duplicates, nested subfolders, removing or keeping the source, moving by copying (as across drives), cancelling, a failed move, an interrupted journal, and undo.
- The app: headless `egui_kittest` tests (`ui_tests.rs`) over temp folders: adding a folder and moving through the tree with the mouse and keys, the grid and Files pane, the preview, Show Info from the right-click menu, Include Subfolders, photos added and removed outside MediaLiner, a merge through the dialog and Ctrl+Z, removing a folder from the list, and the settings coming back after a restart. `ui_tests_import.rs` covers importing, with a folder made up as a camera's card standing in for the device: the dialog opening for a card, the list of files, New Folder, the copy, Undo Last Import, the same card offering only new photos, the question before deleting from the device, a stick of documents and a listed folder opening nothing, the setting off, a device waiting for another dialog to close, and unplugging.

Still to verify by hand: importing from a real card, drive, and camera on Linux and on Windows (the tests use a folder in place of a device, so noticing a real device being plugged in, and everything in `portable` on Windows, has only been compiled, not run), the layout and the look in the light and dark themes (screenshots need a display), drag and drop between folders, Open in Folder on Linux file managers and on Windows, and scrolling a folder of thousands of photos.

## Diagnostics

The shared `LAANKIT_SCREENSHOT`, `LAANKIT_THEME`, and `LAANKIT_SCALE` work as in the other apps. `LAANKIT_OPEN` takes `settings`, `about`, `palette`, or `import`. `LAANKIT_DEVICE=<folder>` treats a folder as a device that has just been plugged in, so you can try importing without a card:

```bash
WAYLAND_DISPLAY= LAANKIT_SCREENSHOT=/tmp/medialiner.png LAANKIT_SCREENSHOT_EXIT=1 \
  cargo run -p medialiner-native -- ~/Pictures
```

`RUST_LOG=debug` logs photos that couldn't be decoded.
