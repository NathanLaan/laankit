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
| F5 | Refresh the selected folder and the tree |
| Ctrl+J | Toggle the status bar |
| Ctrl+K, Ctrl+,, Ctrl+Shift+I | Command palette, Settings, About |

Merge Into…, Remove Folder from List, Include Subfolders, Open With Default App, and Copy Path are in the command palette too.

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

The app (`apps/medialiner/native/src`):

| Module | What it does |
|---|---|
| `main` | The window, and a folder on the command line |
| `app` | All UI state; backend events, commands, view actions, dialogs, and the looks at the folder for changes |
| `backend` | The files thread, the picture threads and their queue, and merges on their own thread |
| `textures` | Thumbnails on the GPU (least recently used are freed) and the previews |
| `views` | Toolbar, Folders and Files panes, the grid, the preview, the Info pane, and the status bar |
| `overlays` | Merge Into…, Settings, About |
| `config` | `settings.json` and `ui-preferences.json` |
| `commands` | The commands and shortcuts, and where the arrow keys go |

The Folders tree is `laankit_egui::tree`, a widget with rows the app flattens, chevrons, drag and drop between rows, and a right-click menu.

## Tests

```bash
cargo test -p medialiner-core -p medialiner-native
```

- `medialiner-core`: natural sorting, the folder list's rules, scanning and hidden files, EXIF fields and orientation (from JPEGs made in the test with an EXIF block), thumbnails and the cache, Open in Folder's commands, and merges: clashes, duplicates, nested subfolders, removing or keeping the source, moving by copying (as across drives), cancelling, a failed move, an interrupted journal, and undo.
- The app: headless `egui_kittest` tests (`ui_tests.rs`) over temp folders: adding a folder and moving through the tree with the mouse and keys, the grid and Files pane, the preview, Show Info from the right-click menu, Include Subfolders, photos added and removed outside MediaLiner, a merge through the dialog and Ctrl+Z, removing a folder from the list, and the settings coming back after a restart.

Still to verify by hand: the layout and the look in the light and dark themes (screenshots need a display), drag and drop between folders, Open in Folder on Linux file managers and on Windows, and scrolling a folder of thousands of photos.

## Diagnostics

The shared `LAANKIT_SCREENSHOT`, `LAANKIT_THEME`, and `LAANKIT_SCALE` work as in the other apps. `LAANKIT_OPEN` takes `settings`, `about`, or `palette`:

```bash
WAYLAND_DISPLAY= LAANKIT_SCREENSHOT=/tmp/medialiner.png LAANKIT_SCREENSHOT_EXIT=1 \
  cargo run -p medialiner-native -- ~/Pictures
```

`RUST_LOG=debug` logs photos that couldn't be decoded.
