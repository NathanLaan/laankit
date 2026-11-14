# MediaLiner: Thumbnail Cache Plan

Status: for review (2026-11-14). The plan answers a report that MediaLiner feels slow and unresponsive, and that thumbnails can take several minutes to load, for example in `/home/laan/Pictures/Pics-2012`. MediaLiner already caches thumbnails on disk. The measurements below put most of the wait on the debug build, which decodes a photo about 40 times slower than a release build. The plan fixes that first, then covers what the cache does not do yet: it only holds photos you have scrolled past, it has no size limit, a merge throws its thumbnails away, and a cached thumbnail is read twice each time it is shown.

## Summary

- Debug builds compile their dependencies with optimization, so a thumbnail takes about 75 ms to make in a debug build where it takes about 1.8 seconds now.
- Thumbnails for the whole selected folder are made in the background once the ones on screen are done, so scrolling a folder you have opened before never waits on a decode.
- A merge and its undo carry the moved photos' thumbnails with them.
- The cache gets a size limit, set in Settings → Storage, beside the 90 day rule it has now.
- Showing a cached thumbnail reads its file once and writes its time at most once a day.
- A photo that can't be decoded is remembered, so it isn't decoded again every time it scrolls into view.
- The work is in five phases. P0 is a change to `Cargo.toml` and a check that it answers the report. P1 is a fix with no visible change and can ship on its own.

## Why it is slow

`Pics-2012` holds 118 JPEGs of 16 megapixels (4912 by 3264, 411 MB in all) on a local ext4 drive. The only MediaLiner binary in `target/` is the debug one; there is no release or `fast-release` build. The workspace sets no optimization for debug builds, so the `image` crate's JPEG decoder runs unoptimized.

Times per photo, measured on this machine (16 cores) with `medialiner-core`'s own functions on photos from `Pics-2012`, one thread:

| Step | Debug | Debug, dependencies at `opt-level = 3` | Release |
|---|---|---|---|
| Make a thumbnail from the photo (`thumbs::decode`) | 1762 ms | 75 ms | 42 ms |
| Store it in the cache | 26 ms | 3.2 ms | 2.5 ms |
| Load it from the cache | 35 ms | 0.7 ms | 0.6 ms |
| Read the embedded EXIF thumbnail (`thumbs::quick`) | 9.2 ms | 0.3 ms | 0.3 ms |

What follows from these:

- In a debug build, a screen of 30 photos on four picture threads takes about 13 seconds to fill the first time, and each new screen you scroll to takes as long again. The whole folder is about 52 seconds of decoding. In a release build the whole folder is about 1.3 seconds.
- The report was several minutes, which is longer than 52 seconds. The difference is not explained by these numbers. P0 ends with a check in a release build to see whether anything is left.
- A cached thumbnail costs 35 ms to load in a debug build and is loaded twice (G2), so even a folder that is fully cached takes about a second per screen there.
- Every photo here has an embedded EXIF thumbnail, so a first image shows quickly, but it is 160 pixels on its long side and is blurry at the default cell size until the real one arrives.
- How responsive the window itself feels (scrolling, selecting) was not measured. A debug build of egui is slower too, and P0 speeds that up as well.

## Today

| Part | Where | Does |
|---|---|---|
| Disk cache | `medialiner-core::thumbs::Cache`, in `~/.cache/medialiner/thumbs/<two hex digits>/<key>.jpg` (`.png` with transparency) | Holds each thumbnail at 320 pixels on its long side, JPEG quality 85. |
| Key | `thumbs::key` | An FNV-1a hash of the photo's path, size, and modification time. A changed photo gets a new key. |
| Making | `backend::picture_thread`, up to four threads | Makes only the thumbnails the grid asks for, which are the ones on screen. A JPEG's embedded EXIF thumbnail shows first on a miss. |
| GPU | `textures::Textures` | Keeps about 600 thumbnail textures, least recently used freed first. |
| Cleanup | `Cache::clean`, once at start | Removes thumbnails not shown for 90 days. Showing one sets its file's time. |
| Settings | Settings → Storage | Shows the cache's size and count, and Clear Thumbnail Cache. |

On this machine the cache holds 90 thumbnails in 2.0 MB (about 22 KB each), and `~/Pictures` holds 833 files.

### Gaps

| # | Gap | Where | Effect |
|---|---|---|---|
| G1 | Only photos that have been on screen are cached. | `App` sends `want_thumbs` with the visible cells only. | The first scroll through any part of a folder decodes full photos (42 ms each in release and 1.8 s in debug for the 16 MP photos measured above). |
| G2 | A hit reads the cached file twice. | `picture_thread` calls `cache.load` to decide on the quick thumbnail, then `cache.thumbnail`, which calls `load` again. | Each cached thumbnail is decoded twice and its time written twice. |
| G3 | Every show writes to disk. | `Cache::load` calls `touch` each time. | One file write per thumbnail shown, where a read would do. |
| G4 | The key includes the path. | `thumbs::key` | Merging a folder remakes every moved photo's thumbnail, and the old ones stay for up to 90 days. Renaming a folder outside the app does the same. |
| G5 | No size limit. | `Cache::clean` goes by age only. | A large library grows the cache without bound for 90 days. At 22 KB each, 100,000 photos is about 2.2 GB. |
| G6 | A failed decode isn't remembered on disk. | `Thumb::Failed` lives only in `Textures`. | A broken file is read and decoded again each launch, and again after its entry leaves the 600. |

## Proposal

### Debug builds (P0)

- The workspace `Cargo.toml` gains:

```toml
# Dependencies optimized in debug builds: the image decoders are
# unusably slow otherwise. The workspace's own crates stay unoptimized.
[profile.dev.package."*"]
opt-level = 3
```

- The apps' own crates keep debug assertions, fast rebuilds, and full debugging. Dependencies are compiled once, more slowly, and stepping into them in a debugger is harder.
- This applies to every app in the workspace. See Q7 for limiting it to the image crates.
- `apps/medialiner/native/README.md` already says to run with `--release`. The root `README.md` lists `cargo run -p medialiner-native` without it and gains a note.
- P0 ends with the check: open `Pics-2012` with an empty cache in a release build, then in the new debug build, and time the grid. If either is still slow, that is found before the other phases start.

### Reading a cached thumbnail (G2, G3)

- `picture_thread` reads the cache once. On a hit it sends the thumbnail. On a miss it sends the quick EXIF thumbnail if there is one, then decodes the photo, stores the thumbnail, and sends it.
- `Cache::thumbnail` goes; `Cache::load`, `thumbs::decode`, and `Cache::store` are called from the worker in that order. Its test moves to the three calls.
- `touch` writes the file's time only when the time it has is more than a day old. The 90 day rule is unaffected by a day's slack.
- `Cache::store` writes its temporary file under a name unique to the call, so two threads storing the same key can't write the same temporary file.

### Filling the folder in the background (G1)

- When a listing arrives or changes, the app sends the folder's photos to the backend as fill jobs, in grid order. With Include Subfolders on, that is the deep listing.
- A picture thread takes a fill job only when no preview and no on-screen thumbnail is waiting.
- A fill job checks whether the key's file exists (`Cache::has`, no decode). If it does, the job is done. If not, it decodes the photo and stores the thumbnail. It does not send the image to the UI, so the GPU set stays the photos you have looked at.
- Selecting another folder replaces the waiting fill jobs. A job in progress finishes.
- One picture thread never takes fill jobs when there is more than one, so a preview or a scroll waits on no more than the grid's own work.
- The status bar shows "Making thumbnails: 120 of 833" while a fill runs, and nothing when the folder is already cached.
- A setting, Make Thumbnails for the Whole Folder (on by default), turns the fill off. With it off the app behaves as it does today.

The grid can ask for a photo while a fill job is decoding it. Today `want_thumbs` drops a job whose path is busy, because the busy job will send its event. A fill job sends none, so the photo would stay a placeholder. The queue therefore keeps the set of paths the grid last asked for, and a fill job whose path is in that set when it finishes sends its thumbnail like any other.

### Thumbnails through a merge (G4)

- A move keeps a file's size and modification time (`merge::move_file` sets the time after a copy), so only the path part of the key changes.
- `Cache::carry(moves)` takes the journal's `(from, to)` pairs. For each it reads `to`'s size and time, works out the old and new keys, and renames the cached file. A missing cached file is skipped.
- The merge thread calls it after `merge::run`, and after `merge::undo` with the pairs reversed. A failure is logged and never fails the merge: the thumbnail is made again.
- A folder renamed or moved outside the app still gets new thumbnails. See Q3.

### Size limit (G5)

- `Cache::trim(max_bytes)` removes the least recently shown thumbnails until the cache fits.
- It runs at start after `clean`, and when a fill finishes.
- Settings → Storage gains a Maximum Size drop-down under the size line:

| Row | Saved as `thumbCacheMaxMb` in `settings.json` |
|---|---|
| 250 MB | `250` |
| 500 MB | `500` |
| 1 GB (default) | `1024` |
| 2 GB | `2048` |
| 5 GB | `5120` |
| No Limit | `0` |

- At 22 KB each, 1 GB is about 45,000 thumbnails.
- Choosing a smaller limit trims at once and updates the size line.
- The group's description becomes: "Thumbnails are kept so folders open quickly the next time. They're made again when needed. Ones not shown for 90 days are removed, and the least recently shown go first when the cache is full."

### Photos that can't be decoded (G6)

- A failed decode stores an empty `<key>.bad` file beside where the thumbnail would be.
- `Cache::load` returns a three-way result: a thumbnail, known bad, or missing. The worker sends `image: None` for known bad without opening the photo.
- The key changes when the file does, so a repaired photo is tried again. `clean`, `trim`, `clear`, and `usage` treat the marker as any other cache file, and the count in Settings leaves markers out.

## Changes

### medialiner-core

| File | Change |
|---|---|
| `thumbs.rs` | `Cache::has`, `Cache::carry`, `Cache::trim`, the three-way `load`, the `.bad` marker, the day's slack in `touch`, and the unique temporary name. `Cache::thumbnail` is removed. |

### medialiner-native

| File | Change |
|---|---|
| `backend.rs` | The single cache read in `picture_thread`, the fill queue and `Backend::fill`, the wanted set, `Event::FillProgress { done, total }`, `carry` after a merge and an undo, `trim` at start and after a fill, and `Job::TrimCache`. |
| `app.rs` | Sends fill jobs from `set_files`, keeps the fill progress for the status bar, and passes the two new settings. |
| `config.rs` | `Settings` gains `thumb_cache_max_mb` and `fill_thumbnails`. |
| `overlays/settings.rs` | The Maximum Size drop-down, the Make Thumbnails for the Whole Folder switch, and the new description. |
| `views.rs` | The fill progress in the status bar. |

### Docs

`apps/medialiner/README.md`, `apps/medialiner/native/README.md` (the thumbnail paragraph and the threads paragraph), `ARCHITECTURE.md` (the `medialiner-core` paragraph), and `SECURITY.md` (the cache line, if the size limit is worth a mention there).

## Phases

| Phase | Work | Done when |
|---|---|---|
| P0 | Optimized dependencies in debug builds, the README note, and the timing check in `Pics-2012` | `Pics-2012` with an empty cache fills its first screen in about a second in a debug build, and nothing slow is left unexplained in a release build |
| P1 | One cache read per hit, the day's slack in `touch`, the unique temporary name, and the `.bad` marker | Core tests pass, and a second visit to a folder writes nothing to the cache |
| P2 | The size limit and its setting | A cache over the limit is trimmed at start, oldest shown first |
| P3 | Thumbnails carried through a merge and an undo | After a merge, the destination's grid shows the moved photos with no decode |
| P4 | Filling the folder in the background, its setting, and the status bar progress | Opening a folder caches all of it while you look at the first screen, and a preview opened during a fill isn't delayed |

## Tests

| Layer | Covers |
|---|---|
| Core | `has` without a decode. `load`'s three results. A bad marker for an undecodable file, and a changed file tried again. `touch` leaving a recent time alone. `trim` removing the least recently shown first and stopping at the limit. `carry` renaming by the new path for a move, skipping a photo with no thumbnail, and reversing for an undo. |
| Backend | A fill caches a folder's photos without sending thumbnail events. A photo the grid asks for during its fill job still arrives. Selecting another folder replaces the waiting fill jobs. |
| UI (`egui_kittest`) | The Maximum Size drop-down saves `thumbCacheMaxMb` and trims. The fill switch off leaves unseen photos uncached. A merge leaves the moved photos' thumbnails under their new keys. |

## Out of scope

- A cache of decoded previews. A preview is decoded from the photo each time it opens.
- Larger thumbnails for high density screens. The 320 pixel size is unchanged.
- Thumbnails for videos.
- An in-memory cache of decoded thumbnails between the disk and the GPU. A cached 320 pixel JPEG is small enough to decode when asked for.
- A version in the cache key. Nothing here changes what a cached file holds.

## Open questions

| # | Question | Recommendation |
|---|---|---|
| Q1 | What did you see that led to this? | Answered: the app feels very slow and unresponsive, and thumbnails can take several minutes to load, for example in `/home/laan/Pictures/Pics-2012`. See "Why it is slow"; P0 comes first because of it. |
| Q2 | Should the background fill be on by default? It reads every photo in the selected folder once, which for 10,000 photos like those measured is about two and a half minutes on three threads in a release build. | On, for the selected folder only, with the switch to turn it off. |
| Q3 | Should the key drop the path, so a folder renamed outside the app keeps its thumbnails? The key would be the file's name, size, and time. Two different photos with the same name, size, and time in different folders would then share a thumbnail, and one would show the wrong picture. | Keep the path. Carry thumbnails through merges, which is the move the app makes itself. |
| Q4 | Is 1 GB the right default limit? | Yes. It holds about 45,000 thumbnails. |
| Q5 | Should the fill cover folders you haven't selected, such as every folder in the list? | No. Only the selected folder, with its subfolders when Include Subfolders is on. |
| Q6 | Should the fill leave one picture thread free, or use all of them when nothing else is waiting? | Leave one free, so a preview never waits behind a fill job. |
| Q7 | Should debug builds optimize every dependency, or only the image crates (`image`, `zune-jpeg`, `png`, and the ones they use)? | Every dependency. It is one line, it can't miss a decoder crate, and it speeds up egui in debug builds for all the apps. The cost is a slower first debug build. |
