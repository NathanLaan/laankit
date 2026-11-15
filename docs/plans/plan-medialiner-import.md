# MediaLiner: Import from a Device Plan

Status: implemented (2026-11-14), MediaLiner 0.2.0. See "Decisions" and "Changes during implementation". When you plug in a memory card, a USB drive, or a camera while MediaLiner is open, an Import dialog offers to copy its photos into one of your folders, with a progress bar.

## Summary

- MediaLiner notices a new removable drive that holds photos and opens an Import dialog.
- The dialog shows the device, how many photos it holds, and the tree of the folders in your list. You pick a folder, or make a new one inside any of them, and choose Import.
- Photos are copied, never moved, unless you tick Delete from Device After Import. Nothing in the destination is overwritten, and a photo that is already there is skipped.
- A progress bar shows files and bytes done, with Cancel. When it finishes, the destination folder is selected in the grid.
- Import from Device is also a command, for a device that was plugged in before the app started.
- Memory cards, USB drives, and cameras that show up as a drive work on Linux and Windows from the first phase. Cameras and phones that connect as a media device (MTP or PTP) are not drives and need separate work on each platform. See "Kinds of device".
- The work is in five phases. P1 to P3 give the whole feature for drives.

## Decisions

| # | Question | Answer |
|---|---|---|
| Q1 | Copy or move? | Copying is the default. Delete from Device is an option, off by default, and it asks only after the files are copied. |
| Q2 | Folders by date? | No. Files go straight into the selected folder. |
| Q3 | Import videos, RAW, and HEIC? | Yes. |
| Q4 | Which drives open the dialog? | Any removable drive. |
| Q5 | Media devices (MTP)? | Supported on Windows. |
| Q6 | Mount drives on Linux? | No. |
| Q7 | Thumbnails to pick photos? | No. The dialog lists the file names with their dates. |
| Q8 | Undo Last Import? | Yes. |
| Q9 | Shortcut? | Ctrl+Shift+M. |
| Q10 | Version? | 0.2.0. |

## Changes during implementation

| Planned | Built |
|---|---|
| A plan worked out before anything is copied | Each file is decided as it's reached: whether it's already there can only be told by reading the device, which a media device makes slow. The dialog says how many files and bytes will be copied, and the result says how many were already there. |
| Cancel stops after the current file | Cancel stops within the file being copied and removes its partial copy, so a large video doesn't hold it up. |
| A toast with an Import button when another dialog is open | The toast names the device and what it holds, and the Import dialog opens when the other dialog closes. Toasts have no buttons. |
| "Don't offer for this device" | Not built. The setting turns the offer off for every device. |
| Arrow keys in the dialog's folder tree | Not built. The tree takes clicks, as the Merge dialog's does. |
| The record doubles as the undo journal | Two files: `device-<hash>.json` for each device's record, and `last-import.json` for the last import's journal. |
| Any removable drive (Q4) | The dialog opens for a removable drive that holds at least one photo or video. A drive with only other files, such as documents, opens nothing; Import from Device still lists it. |
| Delete from device | It deletes only the files this import copied, and only those whose copies are still in the folder. Files skipped as already there stay on the device. |
| `windows-sys` | The `windows` crate, which has the Portable Devices API. `libc` on Linux for the free space. |
| Media devices on Linux (P4) | Built, since this machine runs GNOME: folders named `mtp:` or `gphoto2:` under `$XDG_RUNTIME_DIR/gvfs`. |
| A drive's key on Linux | The file system's UUID from `/dev/disk/by-uuid`, or the volume's label when it has none. |
| Nothing | `LAANKIT_DEVICE=<folder>` treats a folder as a device that has just been plugged in, for trying the dialog without a card. |

What was checked, and what wasn't:

| Part | Checked by |
|---|---|
| The copy, clashes, the record, undo, deleting, cancelling, a device removed part way | Core tests, on folders |
| The dialog, New Folder, the question before deleting, the offer, waiting for another dialog, unplugging | Headless UI tests, with a folder standing in for a device |
| The dialog's look | One screenshot in the dark theme, with `LAANKIT_DEVICE` |
| Noticing a real drive on Linux | The parsing of `/proc/self/mountinfo` is tested on fixed text. No real drive was plugged in. |
| Media devices under GNOME | The naming is tested. No real device was plugged in. |
| Everything on Windows: drive letters, labels, free space, and all of the Portable Devices code | Compiled and linted for Windows from Linux. Never run. |

## Today

| Part | Where | Does |
|---|---|---|
| Folders | `medialiner-core::library`, the Folders pane | Your list of folders, each the root of a tree. |
| Moving files safely | `medialiner-core::merge` | Plans every move first, never overwrites, renames a clash to `name (2).jpg`, sets an identical file aside, copies and checks the size across drives, keeps the modification time, journals each move, and undoes from the journal. |
| A dialog with a folder tree, a plan, and progress | `overlays/merge.rs` | The Merge dialog: pick a destination in the tree, see what will happen, then a progress bar with Cancel. |
| Background work | `backend.rs` | A files thread, picture threads, and one thread per merge with progress events and a cancel flag. |
| Devices | Nothing | MediaLiner doesn't know about drives. A card is reachable only by adding its folder with Add Folder. |
| File kinds | `medialiner-core::media` | Photos (JPEG, PNG, WebP, GIF, BMP, TIFF), videos (recognized, not shown), and other files, which include HEIC and camera RAW. |

## Kinds of device

| Device | How it appears | Linux | Windows |
|---|---|---|---|
| Memory card in a reader, USB stick, USB hard drive | A drive with a file system | Mounted by the desktop under `/media/<user>/` or `/run/media/<user>/` | A drive letter |
| Camera set to mass storage mode | The same as a card | The same | The same |
| Camera or phone in MTP or PTP mode | A media device, with no file system and no drive letter | GNOME mounts it as a folder under `/run/user/<uid>/gvfs/`. Other desktops don't give it a path. | Listed in Explorer but reachable only through the Windows Portable Devices API. |

- The first row covers every card and most cameras, either through a card reader or the camera's mass storage setting. It needs no new library on either platform.
- Phones connect only as media devices, and so do many recent cameras.
- On Linux under GNOME, a media device already looks like a folder, so it costs little more than drives (P4).
- On Windows, a media device needs the Portable Devices API through the `windows` crate: listing, reading, and copying are all different calls from file access. That is the largest single piece of this plan (P5), and it is Windows-only code that can't be tested on Linux.
- Talking to cameras directly over USB from Rust was considered and left out: the operating system's own driver already holds the device on both platforms.

## Proposal

### Noticing a device

- A watcher thread looks at the list of mounted drives every two seconds while the app is open.

| Platform | Drives come from | A drive counts as removable when |
|---|---|---|
| Linux | `/proc/self/mountinfo` | It is mounted under `/media/<user>/` or `/run/media/<user>/`, where the desktop puts drives you plug in |
| Windows | `GetLogicalDrives` | `GetDriveTypeW` says removable, or it is a fixed drive that appeared after the app started |

- A drive that is new since the last look is scanned in the background for photos.
- The dialog opens only when the drive holds something to import: at least one photo or video anywhere on it. A USB stick of documents opens nothing.
- A drive that was already there when the app started opens nothing. Import from Device lists it.
- A folder already in your list (a card you added with Add Folder) opens nothing.
- On Linux the desktop must mount the drive. GNOME, KDE, and most others do this when you plug it in. MediaLiner doesn't mount drives itself. See Q6.
- If another dialog is open, or a merge or import is running, a toast shows "SANDISK 32: 412 photos (9.4 GB). Import from Device (Ctrl+Shift+M) copies them.", and the Import dialog opens when the other one closes.
- A second device plugged in while the dialog is open is added to the dialog's device drop-down.
- Unplugging the device closes the dialog if no import is running. During an import, the import stops with what it has copied and reports the rest as not copied.
- Settings → Import has Offer to Import When a Device Is Plugged In, on by default.

### The Import dialog

```
┌ Import Photos ────────────────────────────────────────────┐
│ From   SANDISK 32                                           │
│        412 photos and 18 videos (9.4 GB)                    │
│        (•) New since the last import (96)   ( ) All (430)   │
│        ┌─────────────────────────────────────────────────┐  │
│        │ DSC05456.JPG                   2026-11-14 09:30 │  │
│        │ DSC05457.JPG                   2026-11-14 09:31 │  │
│        └─────────────────────────────────────────────────┘  │
│ To     ▾ Pictures                                           │
│          ▸ 2025                                             │
│          ▾ 2026                                             │
│              November         ← selected                    │
│        [ New Folder… ]                                      │
│                                                             │
│ [x] Include videos                                          │
│ [x] Include RAW and other files                             │
│ [ ] Delete from device after import (asks first)            │
│                                                             │
│ 96 files (2.1 GB) into /home/you/Pictures/2026/November     │
│                                   [ Cancel ]  [ Import ]    │
└─────────────────────────────────────────────────────────────┘
```

- From: the device's name (its volume label, or "USB Drive (E:)" without one) and what was found. The count fills in while the scan runs. With more than one device, the name is a drop-down.
- The list: the name of each file that will be copied, with its date. A file's place on the device shows when you point at it.
- All or New: New leaves out files this device has already had imported, by the record in "What has been imported". New is the default when there is a record, and All otherwise.
- To: the same tree as the Folders pane, with every folder in your list as a root. The dialog opens with the folder that is selected in the app.
- New Folder: adds a folder inside the selected one, with its name ready to edit. Enter creates it and selects it. A name that exists, is empty, or has a character the file system refuses shows the reason under the field. You can make a folder inside one you just made.
- Include videos, and Include RAW and other files: cards hold files MediaLiner doesn't show yet. Both are on by default so nothing is left behind on the card. The camera's own index and housekeeping files are never imported (`.THM`, `.CTG`, `.BDM`, `.CPI`, `.MPL`, `.BNP`, `.INP`, `.INT`, `.IND`, and hidden files).
- Delete from device after import: off by default, and never remembered. When it's on, the dialog asks "Delete the 96 copied files from SANDISK 32?" once the copy is made, with Delete from Device and Keep on Device. Only files this import copied, and whose copies are still in the folder, are deleted.
- The line above the buttons says what Import will do. Import is disabled until the scan is done and a folder is chosen.
- If your list has no folders, the dialog shows Add Folder in place of the tree.

### Importing

- Each file is decided as it's reached:

| Case | What happens |
|---|---|
| No file of that name in the destination | Copied |
| A file of that name with the same contents | Skipped as already there |
| A file of that name with different contents | Copied as `name (2).jpg` |
| Two files on the device with the same name (cameras restart numbering in each `DCIM` subfolder) | The second is copied as `name (2).jpg` |

- Each file is copied to a hidden `.name.<pid>.part` file, its size is checked against the original, its modification time is set to the original's, and it is renamed into place. A cancelled or failed copy leaves no partial file.
- The source folders on the device are flattened: `DCIM/100MSDCF/DSC05456.JPG` goes to the chosen folder, not to `DCIM/100MSDCF` inside it.
- The dialog turns into the progress view: a bar, "38 of 96, 840 MB of 2.1 GB", the current file's name, and Cancel. Cancel stops within the current file, removes its partial copy, and keeps what was copied.
- When it ends, the dialog shows the result: copied, already there, and any that failed with the reason for each. Done closes it and selects the destination in the grid.
- The free space on the destination is checked before the first copy. Too little shows "Not enough space: 2.1 GB needed, 1.3 GB free" and Import stays disabled.
- The grid's own once-a-second check shows the new photos as they arrive.

### What has been imported

- After an import, a record is written to `~/.local/share/medialiner/imports/device-<hash of the device key>.json`: each imported file's path on the device, size, and modification time.
- The device key is the volume's serial number or UUID, so the same card is known in any reader.
- "New since the last import" is every file on the device that isn't in the record.
- The last import's journal is `imports/last-import.json`. Undo Last Import removes the files the import copied, leaving any that have changed since, and takes them out of the record so they count as new. Undo is not offered for an import that deleted from the device.
- Settings → Import lists the devices with a record and has Forget for each.

### Commands and settings

| Command | Shortcut | When |
|---|---|---|
| Import from Device… | Ctrl+Shift+M | Always. With no device present, the dialog says so and waits for one. |
| Undo Last Import | none | The last import can be undone |

| Setting | Where | Default |
|---|---|---|
| Offer to import when a device is plugged in | `settings.json`, `offerImport` | On |
| Include videos | `ui-preferences.json`, `importVideos` | On |
| Include RAW and other files | `ui-preferences.json`, `importOthers` | On |

The toolbar gains an Import button below Add Folder.

## Changes

### medialiner-core

| File | Change |
|---|---|
| `devices.rs` (new) | `Volume { name, key, location, removable }`, `volumes()` for each platform, `parse_mountinfo(text)` and `gvfs_name` kept apart so they are tested with fixed text, and `added(before, after)`. |
| `import.rs` (new) | The `Source` trait and `FolderSource`, `Options`, `choose`, `run(source, items, dest, cancel, progress) -> Outcome`, `finish`, `Record` (load, save, contains), `records`, `forget`, `delete_copied`, `last`, `undo`, and `free_space`. |
| `portable.rs` (new, Windows only) | Drive letters with their labels and serial numbers, free space, the list of media devices, and `PortableSource`: listing, reading, and deleting through Windows Portable Devices. |
| `merge.rs` | The clash naming is shared with `import`. |
| `Cargo.toml` | The `windows` crate for Windows only (already in the workspace's lock file through other crates), and `libc` for Unix only, for the free space. |

### medialiner-native

| File | Change |
|---|---|
| `backend.rs` | The watcher thread, the device scan, the import thread, and `Event::{DeviceAdded, DeviceRemoved, Devices, DeviceScanning, DeviceScanned, ImportProgress, ImportDone, ImportDeleting, ImportDeleted, ImportUndone, ImportRecords}`. Tests inject devices in place of the watcher. |
| `overlays/import.rs` (new) | The dialog: the device row, the folder tree, New Folder, the options, the progress view, and the result. |
| `app.rs` | Opening the dialog or the toast on a device, the import state, selecting the destination afterwards, and Undo Last Import. |
| `commands.rs`, `views.rs`, `config.rs`, `overlays/settings.rs` | The two commands, the toolbar button, the three settings, `Paths::imports_dir`, and Settings → Import. |

### Docs

`apps/medialiner/README.md` (an Import feature, and the imports folder in Your data), `apps/medialiner/native/README.md`, `ARCHITECTURE.md` (the `medialiner-core` paragraph), and `SECURITY.md` (MediaLiner reads devices you plug in, writes to one only when you choose Delete from Device, and keeps a record of imported file names).

## Phases

| Phase | Work | Done when |
|---|---|---|
| P1 | `import.rs`: listing, run with the safe copy, cancel, and free space | Core tests pass |
| P2 | `devices.rs`, the watcher, the Import dialog with the folder tree and New Folder, progress, the result, and the command | Plugging in a card on Linux and on Windows opens the dialog, and an import copies its photos |
| P3 | The record, New since the last import, Undo Last Import, and Settings → Import | Plugging the same card in again offers only the new photos |
| P4 | Media devices on Linux: folders under `/run/user/<uid>/gvfs/` whose names start with `mtp:` or `gphoto2:` are watched as devices | A phone or camera in MTP mode opens the dialog under GNOME |
| P5 | Media devices on Windows through the Portable Devices API | A phone or camera in MTP mode opens the dialog on Windows |

## Tests

| Layer | Covers |
|---|---|
| Core, devices | `parse_mountinfo` on fixed text: drives under `/media/<user>/` and `/run/media/<user>/`, escaped spaces in names, and system mounts left out. `added` finding the new one. |
| Core, import | Scanning a made-up card (`DCIM` with two numbered folders, a video, a RAW file, housekeeping files, and hidden files). Each row of the cases table. No `.part` file after a cancel or a failed copy. Modification times kept. Delete from device only after a checked copy. Too little free space. The record, New, and undo leaving changed files. |
| UI (`egui_kittest`) | An injected device opens the dialog. A device with no photos opens nothing. New Folder creates and selects a folder, and refuses a name that exists. Import copies, shows progress, and selects the destination. A device removed in the middle ends the import with a report. The toast shows when another dialog is open. The setting off opens nothing. |
| By hand | A real card and a real camera on Linux and on Windows, since CI has neither. |

## Risks

| Risk | Effect | What the plan does |
|---|---|---|
| A card is pulled out during an import | Half-copied files | The `.part` file and the size check: nothing partial is left, and the report lists what wasn't copied |
| Delete from device removes the only copy | Lost photos | Off by default, never remembered, and each delete follows a checked copy of that file |
| The Linux desktop doesn't mount drives | Nothing happens when a card is plugged in | Import from Device says no device was found and explains. See Q6. |
| A fixed USB hard drive full of photos opens the dialog | An unwanted prompt | The setting turns the offer off |
| Windows media devices are far more work than drives | P5 slips or is dropped | It is last, and everything before it stands without it |

## Out of scope

- Choosing single photos in the dialog from thumbnails. The choice is all, or new since the last import.
- Renaming files as they are imported.
- Ejecting the device from MediaLiner.
- Mounting drives on Linux when the desktop doesn't.
- Media devices on Linux desktops other than GNOME.
- Importing from a folder that isn't a device. Merge covers folders.
- Showing HEIC, RAW, and video files in the grid once imported. They are copied and listed in the Files pane.
- macOS.

## Open questions

All answered. See "Decisions" at the top.
