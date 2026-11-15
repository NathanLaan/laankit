# MediaLiner

MediaLiner is a desktop app for browsing and organizing the photos in folders you choose. Your photos stay where they are, as plain files in plain folders: there's no import step and no database, and MediaLiner never changes a photo's contents.

It is a pure-Rust desktop app built on [egui](https://github.com/emilk/egui) over the shared `laankit-*` crates, with no webview and no JavaScript. The shipped binary is `medialiner`. Linux and Windows are the supported platforms.

- Run, build, install: see the [root README](../../README.md).
- The native app in depth: see [`native/README.md`](native/README.md).
- How it fits together: [`ARCHITECTURE.md`](../../ARCHITECTURE.md).

## Features

- Folders: add folders to a list (Ctrl+O). Each is the root of a tree in the Folders pane, with its subfolders below it. Adding a folder that's inside one already listed selects it there; adding the parent of listed folders replaces them.
- Files: the Files pane lists the selected folder's files by name, photos first in colour and other files muted. Include Subfolders (the pane's header button) also lists everything in the folders below.
- Grid: the selected folder's photos as square thumbnails, with a size slider (Ctrl+= and Ctrl+-). Thumbnails are made in the background, the ones on screen first, and cached so a folder opens instantly the next time.
- Preview: double-click a photo, or press Enter, to see it fitted to the window. Left and Right step through the folder, Ctrl+scroll or Ctrl+= zooms, drag pans, 0 fits, 1 shows it at 100%, and Esc goes back to the grid.
- Right-click a photo: Open in Folder (the system file manager, with the file selected where it can be), Show Info, Open With Default App, and Copy Path.
- Info: a pane on the right with the file's details (folder, size, format, dimensions, and dates) and its EXIF data (date taken, camera, lens, exposure, and more). A photo's location is hidden until you choose Show Location.
- Merge: right-click a folder and choose Merge Into…, or drag a folder onto another. Everything in it moves into the folder you choose, subfolders merging with ones of the same name. The dialog shows what will move before anything does. Nothing is overwritten: a clashing name is kept as `name (2).jpg`, and a file identical to one already there is set aside in a hidden `.medialiner-duplicates` folder. Ctrl+Z, or Settings → Merges, undoes it.
- Import: plug in a memory card, a USB drive, or a camera while MediaLiner is open, and the Import dialog opens when the device holds photos or videos. It lists the files with their dates, and the folders in your list: choose one, or make a new folder inside it, and Import copies the files there with a progress bar. Nothing is overwritten: a file that's already there is skipped, and one with the same name and different contents is kept as `name (2).jpg`. Plugging the same device in again offers only what's new. Delete from Device After Import asks once the copy is made, and removes only the files that were copied. Import from Device (Ctrl+Shift+M) opens the dialog at any time, and Undo Last Import removes what the last import copied.
- Changes made outside MediaLiner show up on their own: the selected folder is looked at once a second, and the tree every five seconds.

Photos are JPEG, PNG, WebP, GIF (the first frame), BMP, and TIFF. HEIC, camera RAW, and videos are listed with the other files for now.

## Your data

| Where | What |
|---|---|
| `~/.config/MediaLiner/settings.json` | Your folders, the open folders in the tree, and the selected folder. |
| `~/.config/MediaLiner/ui-preferences.json` | Window and view preferences: panes, thumbnail size, Include Subfolders, and Show Location. |
| `~/.local/share/medialiner/merges/` | A journal of each of the last 10 merges, for undo. |
| `~/.local/share/medialiner/imports/` | The names, sizes, and dates of the files imported from each device, and the last import's journal, for undo. Settings → Import forgets a device. |
| `~/.cache/medialiner/thumbs/` | The thumbnail cache. Settings → Storage shows its size and clears it. |

On Windows, the settings and journals are in `%APPDATA%\MediaLiner` and the cache in `%LOCALAPPDATA%\medialiner\cache`. Nothing is written into your photo folders except what a merge moves there, its duplicates folder, and what an import copies there.

## Devices

| Device | Linux | Windows |
|---|---|---|
| A memory card, a USB drive, or a camera in mass storage mode | Seen once your desktop mounts it under `/media` or `/run/media` | Seen as a drive letter |
| A camera or phone that connects as a media device (MTP or PTP) | Seen when GNOME mounts it (under `$XDG_RUNTIME_DIR/gvfs`) | Read through Windows Portable Devices |

MediaLiner doesn't mount drives itself. A fixed USB hard drive is offered only when it's plugged in while MediaLiner is open.
