# MediaLiner: Implementation Plan

Status: implemented (2026-11-09), MediaLiner 0.1.0, M1 to M7, not yet checked by eye. MediaLiner is a new LaanKit app for browsing and organizing photos in folders you choose. It's built so video can be added later.

## Summary

MediaLiner is the sixth LaanKit app, built the same way as the others: pure `Rust`, egui/eframe over the shared `laankit-egui` shell, and a backend crate with no UI (`medialiner-core`). Your photos stay where they are, as plain files in plain folders. MediaLiner has no database and no import step, and it never changes a photo's contents.

- You add folders to a list. Each one is the root of a tree in the left sidebar, with its subfolders below it.
- Selecting a folder lists its files in a Files pane to the right of the tree, and shows a grid of its photos in the centre.
- Selecting a photo, in the grid or in the Files list, opens it in a preview in the centre.
- Right-clicking a photo offers Open in Folder (the file in the system file manager) and Show Info (a pane on the right with the file's details and EXIF data).
- Any folder in the tree can be merged into another: everything in it moves into the other folder.
- Linux and Windows are the targets. The macOS build comes from the shared packaging and isn't tested (Q9).

Naming follows the other apps: the product is MediaLiner, the binary and package are `medialiner`, the Cargo packages are `medialiner-native` and `medialiner-core`, and the app id is `com.medialiner.app`.

## Decisions and changes during implementation

- Open questions: all ten answered with the plan's assumptions.
- Settings are in two files, as in the other apps: `settings.json` (the folders, the open folders in the tree, and the selected folder) and `ui-preferences.json` (panes, thumbnail size, Include Subfolders, Show Location, status bar, and title bar), not one `config.json`.
- Cached thumbnails are JPEG (PNG when the photo has transparency), not WebP: the `image` crate only writes lossless WebP, which is larger. They're in `thumbs/<first two hex digits>/<hash>.jpg`.
- The arrow keys in the tree select each folder as they reach it; there's no separate Enter step.
- The tree widget (`laankit_egui::tree`) has chevrons, drag and drop between rows, and a right-click menu, but not NoteLiner's renaming in place. NoteLiner keeps its own tree for now.
- A bar above the grid holds the folder's name and path and the thumbnail size slider; in the preview it holds Back, the photo's name and place ("3 of 120"), the zoom, and Fit and 100%. The toolbar has Add Folder and the three pane toggles.
- Dragging a folder onto another opens Merge Into… with that destination chosen. Nothing follows the pointer while dragging; the folder under it is highlighted.
- The toast after a merge says Ctrl+Z undoes it (toasts have no buttons). Ctrl+Z undoes this session's last merge; Settings → Merges undoes any of the last 10.
- Identical files go to `.medialiner-duplicates/<source name>/` in the source's listed folder, or the destination's when the source is itself a listed folder.
- A merge moves hidden files too, since it moves everything. Empty folders left in the source are removed even when the source is kept.
- Merging away a listed folder removes it from the list. Undoing that merge brings the folder back on disk but not to the list; add it again.
- With Include Subfolders on, the selected folder's subtree is looked at every five seconds, not every second.
- Open in Folder on macOS uses `open -R`, untested.
- `medialiner <folder>` on the command line adds and selects the folder.
- The desktop entry doesn't claim folders (no MIME types), so MediaLiner doesn't become a folder handler.
- The icon is a placeholder (two stacked photos), rendered from `apps/medialiner/assets/icon.svg`.
- Not checked by eye: screenshots need a display, which the sandbox used for this work doesn't allow. It's listed under "Still to verify by hand" in the app's native README, with drag and drop, Open in Folder on real file managers and Windows, and a folder of thousands of photos.

## Layout

```
┌──────────────────────────────────────────────────────────────────────────┐
│ Title bar / toolbar: Add Folder · Back · Thumbnail Size · Info · Palette │
├────────────┬─────────────┬──────────────────────────────┬────────────────┤
│ Folders    │ Files       │  Grid of the folder's photos │ Info           │
│ ▾ Photos   │ IMG_001.jpg │  ┌────┐ ┌────┐ ┌────┐ ┌────┐ │ IMG_002.jpg    │
│   ▾ 2025   │ IMG_002.jpg │  │    │ │    │ │    │ │    │ │ 4032 × 3024    │
│     Trip   │ IMG_003.png │  └────┘ └────┘ └────┘ └────┘ │ 3.2 MB, JPEG   │
│     Misc   │ notes.txt   │  ┌────┐ ┌────┐               │ Taken 2025-…   │
│ ▸ Phone    │             │  │    │ │    │               │ Camera …       │
│            │             │  └────┘ └────┘               │                │
│            │             │      or the photo preview    │                │
├────────────┴─────────────┴──────────────────────────────┴────────────────┤
│ Status: 124 photos · 3 other files · Trip · 2.1 GB                        │
└──────────────────────────────────────────────────────────────────────────┘
```

| Area | What it shows | Resizable | Can hide |
|---|---|---|---|
| Folders (left) | A tree of the folders you added and their subfolders | Width | Toggle Folders, Ctrl+B |
| Files (left of centre) | The selected folder's files, by name | Width | Toggle Files, Ctrl+Shift+F |
| Centre | The grid of photos, or one photo's preview | Fills the rest | No |
| Info (right) | The selected photo's details | Width | Show Info, Ctrl+I, and its close button |
| Status bar | Counts and sizes for the selected folder, or the selected photo's name and size | No | No |

The Folders and Files panes are two side-by-side `egui::Panel::left` panels, not the vertical pane stack, since the request puts them next to each other. Each remembers its width, as egui panels do.

## Folders

### Adding and removing

- Add Folder (toolbar, palette, Ctrl+O) opens the system folder picker through `laankit_egui::file_dialog::FileDialogs`, which keeps the window responsive on Linux.
- A folder is added once. Adding a folder that's inside one you already added selects it in the tree instead. Adding a parent of one already added replaces the child in the list, since the parent's tree includes it.
- Right-clicking a root in the tree offers Remove from List. This only removes it from MediaLiner's list. Nothing on disk changes.
- A root that's missing at start (an unplugged drive) stays in the list, greyed out with a "not found" hint, and comes back when it's there again.

### The tree

- Each root shows by its folder name, with a muted hint of its parent path when two roots share a name (as BlockLiner's Files pane does).
- Subfolders are read when you expand a folder, not all at once, so a large photo library opens instantly. The expanded folders are remembered across restarts.
- Hidden folders (names starting with `.`, and folders with the Windows hidden attribute) aren't shown (Q7).
- Folders are sorted by name, with numbers in their natural order ("Day 2" before "Day 10").
- Arrow keys move through the tree, Left and Right collapse and expand, and Enter selects.
- Right-clicking a folder offers Merge Into…, Open in Folder, and Refresh, plus Remove from List on a root.

The tree is a new widget in `laankit-egui` (`tree.rs`), a generalized version of NoteLiner's `views/tree.rs` without its notes-specific parts (drag to reorder and renaming in place). NoteLiner can move to it later. That move isn't part of this plan.

## Files pane

- It lists the files in the selected folder itself, sorted by name. Subfolders aren't listed here, since the tree shows them.
- Photos show a small file-type icon and their name. Other files (`.txt`, `.xmp`, videos for now) show muted, so you can see everything a merge would move (Q5).
- Clicking a photo opens its preview. The selected photo is highlighted, and the list scrolls to keep it in view as you step through photos in the preview.
- The right-click menu is the same as the grid's (see Right-click menu).
- An "Include Subfolders" toggle in the pane's header lists, and shows in the grid, the photos in every folder below the selected one too, with each row's folder as a muted hint. It's off by default (Q4).

## Grid

- Square thumbnails in rows that wrap to the width of the centre area. Thumbnail Size (toolbar slider, Ctrl+Plus and Ctrl+Minus) sets their size from 96 to 320 points, and is remembered.
- The image fills its square without stretching (cropped to the square, Q6). The file name shows below each thumbnail, cut short with an ellipsis.
- Only the rows on screen are laid out (`ScrollArea::show_rows`), so a folder of 10,000 photos scrolls smoothly.
- Click selects a photo, double-click or Enter opens its preview. Arrow keys move the selection. The selection is shared with the Files pane and the Info pane.
- Thumbnails that aren't ready yet show a muted placeholder. A file that can't be read shows a broken-image icon and stays in the grid.

### Thumbnails

| Step | Where | Notes |
|---|---|---|
| Read the file, apply its EXIF orientation, and scale it to 320 px on the long side | A thread pool in the backend (`std::thread` workers, 4 or the number of cores, whichever is fewer) | Requests for photos on screen go first. Requests for photos no longer on screen are dropped. |
| Save the thumbnail to disk | The thumbnail cache, `<cache>/medialiner/thumbs/<hash>.webp` | The key is a hash of the path, size, and modification time, so a changed photo gets a new thumbnail. |
| Upload to the GPU | The UI thread, a few per frame | Textures are kept in an LRU of about 600 thumbnails and freed when they drop out. |

- JPEG files with an embedded EXIF thumbnail of at least 160 px use it for a first quick image, replaced by the full one when it's ready.
- Settings → Storage shows the cache's size and offers Clear Thumbnail Cache. Cached thumbnails older than 90 days that haven't been shown are removed at start.
- `laankit-core::paths` gains `cache_dir(product)`: `~/.cache/<app>` on Linux (`$XDG_CACHE_HOME` when set) and `%LOCALAPPDATA%\<App>\cache` on Windows.

### Formats

| Format | Supported | Through |
|---|---|---|
| JPEG, PNG, WebP, GIF (first frame), BMP, TIFF | Yes | The `image` crate, with `bmp` and `tiff` added to MediaLiner's features |
| HEIC and HEIF (iPhone photos) | No, listed as other files | Needs `libheif`, a C library (Q3) |
| Camera RAW (CR2, NEF, ARW, DNG) | No, listed as other files | Q3 |
| SVG | No | Not photos |
| Video (MP4, MOV, MKV) | No, listed as other files | See Video later |

A file is a photo by its extension, then confirmed by reading its header when it's decoded.

## Preview

- Opening a photo replaces the grid with the photo, fitted to the centre area. The full image is decoded on a worker thread. The thumbnail shows, scaled up, until it's ready.
- Images larger than 8192 px on a side (GPU texture limits) are scaled down to fit when decoded.
- Left and Right, or Page Up and Page Down, step to the previous and next photo in the folder. The next and previous photos are decoded ahead.
- Zoom: Fit (0), 100% (1), Ctrl+Plus and Ctrl+Minus, and Ctrl+scroll around the pointer. Drag to pan when zoomed in.
- Back (Esc, Backspace, or the toolbar's Back button) returns to the grid, scrolled to the photo you were on.
- The Files pane and Info pane stay as they are, and follow the photo you step to.

## Right-click menu

On a photo in the grid, the preview, or the Files pane:

| Item | What it does |
|---|---|
| Open in Folder | Opens the system file manager at the file, with the file selected where the platform allows. |
| Show Info | Selects the photo and opens the Info pane on the right. |
| Open With Default App | Opens the file in the system's default app for it (`open::that_detached`). |
| Copy Path | Copies the file's full path. |

Open in Folder, by platform:

| Platform | How | File selected |
|---|---|---|
| Windows | `explorer.exe /select,<path>` | Yes |
| Linux | The `org.freedesktop.FileManager1.ShowItems` D-Bus call (Files, Dolphin, Nemo, Thunar, and others), sent with `gdbus` or `dbus-send` when one is installed | Yes, when the file manager supports it |
| Linux, no D-Bus answer | The parent folder through `open::that_detached`, as BlockLiner's Open Containing Folder | No |

Running `gdbus` keeps MediaLiner free of a D-Bus library. If it isn't installed, the folder still opens.

## Info pane

- It shows the selected photo, in the grid or the preview, and updates as the selection moves. With nothing selected it says "No photo selected".
- File: name, folder (with Open in Folder), size, format, dimensions, and the created and modified dates.
- Photo, from EXIF when there is one: date taken, camera make and model, lens, focal length, aperture, shutter speed, ISO, flash, and orientation.
- Location, from EXIF GPS: latitude and longitude, with Copy and Open in Maps (an OpenStreetMap link in the browser). It's hidden until you choose Show Location, since a location is personal and the pane can be on screen while others are watching.
- Each value can be selected and copied.
- EXIF is read with `kamadak-exif`, a pure Rust crate, on a worker thread. It's read from JPEG, TIFF, PNG, and WebP files.

## Merging folders

Merge moves everything in one folder (the source) into another (the destination). It's the only thing MediaLiner does that changes files on disk, so it's built to be safe and undoable.

### Choosing

- Right-click a folder, then Merge Into…. A dialog shows the source and a tree of your folders to choose the destination from.
- Drag a folder in the tree onto another folder (later, in M7).
- A folder can't be merged into itself, into a folder inside it, or into its own parent, where nothing would move. Those choices are greyed out.
- The source and destination can be under different roots and on different drives.

### Before it runs

The dialog lists what will happen before anything moves:

| Line | Example |
|---|---|
| What moves | 214 photos, 6 other files, and 3 subfolders (2.1 GB) |
| Name clashes | 4 files have the same name as a file already in Trip |
| What happens to clashes | Kept both: the moved file is renamed `IMG_001 (2).jpg` |
| The source folder | Removed once it's empty (a checkbox, on by default) |

Subfolders merge the same way: a subfolder with a name that already exists in the destination merges into it, and doesn't get a second copy.

Clashes are never overwritten. Two files with the same name and identical contents (same size and a matching hash) are a duplicate, not a clash: the source copy is moved to a `.medialiner-duplicates` folder inside the source's root, so nothing is lost, and the dialog counts them separately (Q1).

### Running

- The merge runs in the backend with a progress bar and a Cancel button. A cancelled merge stops after the current file. What moved stays moved, and Undo Merge puts it back.
- A move within one drive is a rename. Across drives it's a copy, a check that the copy is the same size, then a delete of the original. A file is never deleted before its copy is complete.
- Each move is written to a journal, `<data>/medialiner/merges/<time>.jsonl`, as it happens, so an interrupted merge (a crash or power loss) can still be undone.
- Files that can't move (in use, or no permission) are skipped and listed when the merge ends. The source folder then isn't removed.

### Undo

- Undo Merge (the toast after a merge, the palette, and Ctrl+Z right after) moves every file back, using the journal, and recreates the source folder.
- The last 10 merges can be undone from Settings → Merges, newest first. A merge can't be undone once a file it moved has been moved or deleted since. The files that still can go back.

## Commands and shortcuts

| Command | Shortcut |
|---|---|
| Add Folder | Ctrl+O |
| Back to Grid | Esc, Backspace |
| Previous Photo, Next Photo | Left, Right, Page Up, Page Down |
| Open Preview | Enter, double-click |
| Zoom In, Zoom Out | Ctrl+Plus, Ctrl+Minus (the thumbnail size in the grid) |
| Fit, 100% | 0, 1 (preview) |
| Show Info | Ctrl+I |
| Toggle Folders Pane | Ctrl+B |
| Toggle Files Pane | Ctrl+Shift+F |
| Merge Into… | palette |
| Undo Merge | Ctrl+Z, palette |
| Open in Folder | Ctrl+Shift+R |
| Refresh | F5 |
| Command Palette, Settings, About | As in the other apps |

## Saved state

| What | Where |
|---|---|
| Settings: the folder list, expanded folders, the selected folder, thumbnail size, Include Subfolders, Show Location, and the theme | `<config>/MediaLiner/config.json`, through `laankit-core::json` |
| Pane widths, window size and position | eframe's `app.ron`, as the other apps |
| Thumbnails | `<cache>/medialiner/thumbs` |
| Merge journals | `<data>/medialiner/merges` |

Nothing is ever written into your photo folders, apart from what a merge moves there and the duplicates folder.

## Watching for changes

- The selected folder is checked once a second while the window has focus, by comparing its listing (names, sizes, and modification times), as BlockLiner checks its folder. New photos appear in the grid, removed ones go, and changed ones get a new thumbnail.
- Expanded tree folders are checked every five seconds for added or removed subfolders.
- F5 (Refresh) re-reads the selected folder and its subtree straight away.
- A file system watcher (the `notify` crate) would be quicker, but on Linux it needs one inotify watch per folder and runs out on large trees. Polling only what's on screen keeps it simple (Q8).

## Architecture

```
apps/medialiner/native   the app: window, panes, grid, preview, dialogs
        │
        ├─ laankit-egui     shell, panes, palette, settings, the new tree widget
        └─ medialiner-core  folders, scanning, thumbnails, EXIF, merging
                │
                └─ laankit-core  paths (with the new cache_dir), json
```

### `medialiner-core`

| Module | What it does |
|---|---|
| `library` | The list of root folders: adding (with the parent and child rules), removing, and whether each is there. |
| `scan` | Lists a folder's subfolders and files (one level, or the whole subtree for Include Subfolders), with natural sorting, hidden-file rules, and media kinds. |
| `media` | `MediaKind { Photo, Video, Other }` from a file's extension and header. Video is detected but not shown yet. |
| `thumbs` | Decoding, EXIF orientation, scaling, the on-disk cache and its keys, and cache cleanup. Returns RGBA buffers, not egui types. |
| `exif` | Reads the Info pane's fields into a plain struct. |
| `merge` | Plans a merge (moves, clashes, duplicates, and totals), runs it with progress and cancel, writes the journal, and undoes it. |
| `reveal` | Open in Folder for each platform. |

The core knows nothing about egui, so it's tested on its own, as the other cores are.

### The app

| Module | What it does |
|---|---|
| `main` | Window, icon, app id, and the persistence path. |
| `app` | All UI state: the selection, the open preview, the panes, and the overlays. Handles backend events, commands, and view actions. |
| `backend` | The thumbnail and preview worker pool with its priority queue, folder checks, EXIF reads, and merges, replying with `Event`s over a channel. |
| `textures` | The thumbnail texture LRU and the preview texture. |
| `views` | Folders tree, Files pane, grid, preview, Info pane, toolbar, and status bar. |
| `overlays` | The merge dialog and its progress, settings, and about. |
| `commands` | The command registry and shortcuts. |
| `config` | Settings, read and written through `laankit-core::json`. |

## Video later

Nothing in this plan builds video, but the plan leaves room for it:

- `MediaKind` already tells videos apart. Videos are listed in the Files pane as other files and moved by merges.
- Thumbnails and preview go through a `Decoder` trait in `thumbs`, so a video decoder can be added beside the image one.
- The likely route is the `ffmpeg` command for poster frames and the system's default player for playback, since decoding video in pure Rust isn't practical. Playing video inside MediaLiner would need GStreamer or libmpv, a bigger decision for its own plan.

## Testing

| Layer | What | How |
|---|---|---|
| Core | Adding roots (duplicates, parents, children), scanning, natural sort, hidden files, media kinds | Unit tests over `tempfile` folders |
| Core | Thumbnails: orientation, scaling, cache keys, a changed file getting a new key, cleanup | Images generated with the `image` crate, and small fixture JPEGs with EXIF orientation 6 and 8 |
| Core | EXIF fields | A few small fixture JPEGs |
| Core | Merge: plain moves, nested subfolders merging, clashes renamed, duplicates set aside, the source removed or kept, refused choices, cancel, a failed move, journal replay after an interruption, and undo | `tempfile` folders. Moving across drives is tested by forcing the copy path. |
| App | Adding a folder, expanding the tree, selecting a folder (Files list and grid), opening and stepping through the preview, Back, the right-click menu, Show Info, the merge dialog through to undo, and settings coming back after a restart | `egui_kittest` UI tests, headless, as the other apps |

Open in Folder is tested by checking the command it would run, not by running it.

## Workspace changes

- `Cargo.toml`: add `apps/medialiner/native` to the members, `medialiner-core` to the workspace dependencies, and `kamadak-exif` (the `image` features MediaLiner adds go in its own `Cargo.toml`).
- `crates/medialiner-core` and `apps/medialiner/native`, with packaging metadata copied from PrintLiner's (`.deb`, `.rpm`, AppImage, and NSIS).
- `crates/laankit-core/src/paths.rs`: `cache_dir`.
- `crates/laankit-egui/src/tree.rs`: the tree widget, added to the gallery example.
- `apps/medialiner/assets/icon.png`, then `cargo run -p xtask -- icons` (Q10).
- `packaging/linux/com.medialiner.app.desktop`, `.metainfo.xml`, and `.appimage.desktop.hbs`.
- `.github/workflows/build.yml`: add `medialiner-core` and `medialiner-native` to the clippy and test lists.
- `scripts/set-version.sh` and `scripts/package-native.sh`: add `medialiner`.
- Docs: `apps/medialiner/README.md`, `apps/medialiner/native/README.md`, the root `README.md`, `ARCHITECTURE.md` (six apps, the new core, the cache folder), `SECURITY.md` (merges are the only change to your files, the journal, and location data), and `docs/packaging.md`.

## Phases

| Phase | What | Done when |
|---|---|---|
| M1 | `medialiner-core`: `library`, `scan`, `media`, and `cache_dir` in `laankit-core` | Core tests pass |
| M2 | The app shell: window, the tree widget, the Folders and Files panes, Add Folder, settings saved and restored, and the packaging and CI entries | You can add folders, browse the tree, and see a folder's files |
| M3 | The grid with thumbnails: worker pool, priority queue, disk cache, and texture LRU | A folder of a few thousand photos scrolls smoothly, and opens instantly the second time |
| M4 | Preview: fit, zoom, pan, stepping, decoding ahead, and Back | You can look through a folder's photos one by one |
| M5 | The right-click menu, Open in Folder on Linux and Windows, and the Info pane with EXIF | Both menu items work on Linux and Windows |
| M6 | Merging: plan, dialog, run with progress, journal, and undo | The merge tests pass, and a merge across two drives works and undoes |
| M7 | Folder checks for changes, Include Subfolders, dragging a folder onto another to merge, the docs, and a check by eye in light and dark themes on both platforms | Ready for a 0.1.0 release |

M1 to M4 make a useful read-only browser on their own. Merging comes after it, once browsing is solid, since it's the part that moves your files.

## Risks

| Risk | What it would mean | Mitigation |
|---|---|---|
| Decoding large JPEGs is slow (a 24 MP photo takes 100 to 300 ms) | Slow thumbnails on the first visit to a folder | Embedded EXIF thumbnails first, the disk cache, the priority queue, and the `image` crate's JPEG decoder at reduced scale where it allows. If that isn't enough, `zune-jpeg`'s scaled decode. |
| GPU memory with many textures | Crashes or slowdowns on low-end machines | The texture LRU, and thumbnails capped at 320 px |
| A merge interrupted part way | Files split between two folders | The journal, written before each move, and Undo Merge from it |
| Moves across drives | Slow, and fail part way more often | Copy, check, then delete, one file at a time, with progress and cancel |
| No HEIC support | iPhone libraries are mostly HEIC | Shown as other files for now. Q3 decides. |
| Open in Folder differs between Linux file managers | The folder opens without the file selected | The fallback still opens the folder |

## Out of scope

- Editing photos (crop, rotate, or adjust) and writing EXIF or rotation back to files.
- Renaming, deleting, copying, or moving single files, apart from merges (a later plan).
- Tags, ratings, albums, search, faces, maps, or any database.
- Video thumbnails and playback (see Video later).
- Slideshows, printing, and sharing.
- Network folders that aren't mounted as a local drive.

## Open questions

The plan's assumption is given for each, so the work can go ahead on it unless you choose otherwise.

| | Question | Assumption |
|---|---|---|
| Q1 | When a merge finds two files with the same name, what should happen? | Keep both, renaming the moved one `name (2).jpg`. Identical files go to a duplicates folder instead of being kept twice. |
| Q2 | Should the source folder be removed after a merge empties it? | Yes, by default, with a checkbox to keep it. |
| Q3 | Should HEIC/HEIF and camera RAW be supported? | Not in 0.1.0. HEIC needs `libheif`, a C library on Linux and a bundled DLL on Windows, which the other apps avoid. It can be its own plan. |
| Q4 | "Browse through photos in that folder and all sub-folders": should selecting a folder show the photos in its subfolders too? | No by default. The tree is how you reach subfolders, and an Include Subfolders toggle shows them all in one grid. |
| Q5 | Should the Files pane list files that aren't photos? | Yes, muted, since a merge moves them too. |
| Q6 | Should grid thumbnails be cropped to squares, or show the whole photo? | Cropped to squares, the usual for photo grids. Whole photos could be a setting. |
| Q7 | Should hidden folders and files show? | No. A setting could show them. |
| Q8 | Polling the folder on screen, or a file system watcher? | Polling, as BlockLiner does. |
| Q9 | Should macOS be tested and supported? | No. It builds from the shared packaging, but only Linux and Windows are tested. |
| Q10 | The icon: do you have one, or should a placeholder be made? | A placeholder in the LaanKit style, to replace later. |
