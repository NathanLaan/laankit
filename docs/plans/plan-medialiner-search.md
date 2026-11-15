# MediaLiner: Photo Search Plan

Status: for review (2026-11-14). Adds a search box that finds photos by name, folder, date, camera, and other details, in the selected folder or in every folder in your list, and shows the results in the grid. A second part, which can ship later, finds photos by what is in them: searching for "dog" finds photos with a dog.

## Summary

- A search box sits in the bar above the grid. Ctrl+F puts the cursor in it.
- Plain words match a photo's file name and its folder path. Filters such as `taken:2012` and `camera:sony` match its details.
- Results show in the grid and the Files pane in place of the folder's listing. Everything that works on a folder's photos works on results: preview, stepping with Left and Right, the Info pane, and the right-click menu.
- Searching by name reads only folder listings, so it needs nothing stored. Searching by details reads each photo's EXIF data once and keeps it in a details cache beside the thumbnail cache, so the second search is quick.
- Nothing is written into your photo folders, and the cache can be cleared and rebuilt, as the thumbnail cache can.
- Results keep up with your folders. A photo that is added, imported, merged in, or removed shows in or leaves the results within a few seconds, as it does in a folder's listing. See "When photos are added or removed".
- Searching by contents uses an image and text model that runs on your computer. No photo and no search leaves the machine. It needs a model file of a few hundred megabytes, a library to run it, and a one-time pass over your photos. See "Searching by what is in the picture".
- The work is in seven phases. Name search (P1 and P2) is useful on its own and can ship first. Contents search (P5 to P7) starts with a trial that measures speed and quality before anything is built into the app.

## Today

| Part | Where | Does |
|---|---|---|
| Listings | `medialiner-core::scan` (`files`, `files_deep`) | Lists a folder, or a folder and everything below it, without hidden files. Each `FileEntry` has the path, name, kind, size, and modification time. |
| Details | `medialiner-core::info::read` | Reads one file's details and EXIF data for the Info pane: date taken, camera, lens, focal length, aperture, exposure, ISO, flash, orientation, and location. Each value is text ready to show. |
| The grid's source | `App::set_files` | The grid, the Files pane, the preview, and the keyboard all work from one list of `FileEntry`, filled by `Event::Listing`. |
| Noticing changes | `App::watch` | While the window has the focus, the selected folder is listed again every second (every five seconds with Include Subfolders), and the grid changes only when the listing's fingerprint does. The selection and the preview are kept while their photo is still there. An import, a merge, and their undos ask for a new listing when they finish. There is no file system watcher. |
| Finding a photo | The Files pane and Include Subfolders | You scroll a list sorted by name. There is no way to narrow it, and no way to look across the folders in your list. |
| Stored state | `settings.json`, `ui-preferences.json`, merge journals, and the thumbnail cache | There is no index of photos. The README says there is no database. |

Reading a photo's EXIF data and its small embedded thumbnail took 0.3 ms per photo in a release build on this machine, with the files already in the system's file cache. A first read from disk is slower and was not measured.

## Proposal

### The search box

- The box is in the centre header, between the title and the thumbnail size slider, with a magnifier icon and the hint "Search photos".
- Ctrl+F focuses it and selects its text. `/` does the same when the grid has the focus.
- Results update as you type, after a pause of 200 ms.
- Enter or Down moves the focus to the first result in the grid.
- Esc in the box clears the search and returns to the folder. Esc in the results grid does the same when no preview is open. A clear button (an x) shows in the box while it has text.
- While a search is shown, the header's title reads "Search" and the subtitle reads "42 photos in Pics-2012" or "42 photos in all folders". The status bar shows "Searching…" with the count so far while the search runs.
- No results shows "No photos match." in the grid, with the scope named and, when the scope is the selected folder, a "Search All Folders" button.

### Scope

| Scope | Covers | Chosen by |
|---|---|---|
| This Folder (default) | The selected folder and every folder below it, whether or not Include Subfolders is on | A drop-down at the right of the box |
| All Folders | Every folder in your list | The same drop-down, or Ctrl+Shift+F while the box has the focus |

- The scope is remembered in `ui-preferences.json` as `searchScope`.
- Selecting a folder in the tree while a search is shown keeps the query and searches the new folder when the scope is This Folder.

### The query

- Words are separated by spaces. Every word must match. Matching ignores case.
- A plain word matches anywhere in the file name or in the folder path below the listed root. `beach 2012` finds `Pics-2012/beach-01.jpg` and `2012/Trips/Beach/IMG_0412.jpg`.
- Quotes keep spaces: `"new york"`.
- A leading minus excludes: `beach -raw`.
- A word with a colon is a filter:

| Filter | Matches | Examples | Needs the details cache |
|---|---|---|---|
| `name:` | The file name only | `name:dsc054` | No |
| `folder:` | The folder path only | `folder:trips` | No |
| `type:` | The format | `type:jpg`, `type:png` | No |
| `size:` | The file size, with `>` or `<` | `size:>5mb`, `size:<200kb` | No |
| `modified:` | The modification date | `modified:2024`, `modified:2024-06`, `modified:>2024-06-01` | No |
| `taken:` | The EXIF date taken, or the modification date when the photo has none | `taken:2012`, `taken:2012-12`, `taken:2012-12-25`, `taken:<2013` | Yes |
| `camera:` | The camera's make and model | `camera:sony`, `camera:"nex-3n"` | Yes |
| `lens:` | The lens | `lens:50mm` | Yes |
| `iso:` | The ISO, a number with optional `>` or `<` | `iso:>1600` | Yes |
| `has:` | `has:location`, `has:exif` | `has:location` | Yes |

- A date with only a year or a month matches the whole year or month.
- An unknown filter, or a value that can't be read (`size:big`), is shown under the box as "Unknown filter: size:big" and the rest of the query still runs.
- `has:location` finds photos that have a location without showing it. Show Location still governs whether the Info pane shows where.

### Results

- Results are photos only. Other files are left out, since the grid can't show them.
- They are ordered by folder, then by name in natural order, the order Include Subfolders uses.
- The Files pane lists the results with each photo's folder (below the listed root) in muted text after its name.
- A cell's tooltip shows the photo's full path.
- The right-click menu gains Show in Its Folder: it clears the search, selects the photo's folder in the tree, and selects the photo in the grid.
- Preview, Left and Right, Info, Open in Folder, Open With Default App, and Copy Path work as they do in a folder.
- Merge commands are unaffected: they act on folders in the tree, not on results.
- Results refresh on their own, and Refresh (F5) runs the search again at once. See "When photos are added or removed".

### How a search runs

- Searches run on one background thread of their own, so listings and the Info pane never wait behind a search.
- The thread walks the scope's folders with `scan`'s rules (hidden files and folders are skipped; a folder that can't be read is left out).
- Each new query cancels the one running. The thread checks a cancel flag between folders and between photos.
- Results arrive in batches (each folder's matches, or every 200 photos), so the grid fills while the search runs.
- Filters that need no details are checked first, so the details of a photo that already fails are never read.

### The details cache

- A query with a filter marked "Yes" above needs each candidate photo's details.
- Details come from `medialiner-core::info`, through a new function that reads only what search uses: date taken, camera, lens, ISO, and whether there is a location. The location itself is not stored.
- The cache is `~/.cache/medialiner/details/<root key>.json`, one file for each folder in your list, holding a record for each photo under the same key the thumbnail cache uses (path, size, and modification time). A changed photo is read again.
- The first details search of a folder reads every photo in scope and shows "Reading details: 1,200 of 8,400" in the status bar. Results appear as they are found. Later searches read only new and changed photos.
- The cache is written when a search finishes or is cancelled, and no more than once every ten seconds while it runs.
- Records for photos that have gone are dropped when the file is written, but only for folders the search walked and could read. A search of one folder leaves the records of the root's other folders alone, and a root that can't be found (a drive that isn't plugged in) keeps all of its records.
- A photo moved by a merge, or moved or renamed outside the app, has a new key and is read again. At 0.3 ms a photo this needs nothing more.
- Settings → Storage shows the details cache's size beside the thumbnail cache, and Clear Thumbnail Cache becomes Clear Caches and clears both.
- Removing a folder from the list deletes its details file.

The README says there is no database. The details cache is a set of files that can be rebuilt from your photos at any time, like the thumbnails, and nothing depends on it being there. The README's wording becomes "no import step and no library database: MediaLiner keeps only caches it can rebuild". See Q4.

### When photos are added or removed

Photos change under a search in four ways: you import from a device, you merge folders or undo either, another program adds, edits, moves, or deletes files, and a drive holding a listed folder is unplugged.

One rule covers all of them: the walk of your folders decides which photos exist, and the caches only supply facts about the photos the walk found. A search never returns a photo from a cache alone, so a deleted photo can't show as a result of a new search, and a new photo is never left out because a cache hasn't heard of it.

| Change | Name and folder search | Details search | Contents search |
|---|---|---|---|
| A photo is added | Found by the next walk. | Its details are read during the walk and cached. | It has no vector yet. It is counted in "aren't indexed yet" and put at the front of the index queue. |
| A photo is removed | Not found by the next walk. | Its record is dropped at the next write. | Its vector is dropped at the next sweep. |
| A photo is edited in place | Found as before. | Its size or time changed, so its key did, and it is read again. | The same: it is indexed again, and the old vector is dropped. |
| A photo is moved by a merge or an undo | Found in its new folder. | Read again in its new place. | Its vector is carried to its new key from the merge's journal, as the thumbnail cache plan carries thumbnails. |
| A photo is moved or renamed outside the app | Found in its new place. | Read again. | Its vector is reused when a record with the same name, size, and time exists whose own file has gone. Otherwise it is indexed again. See Q15. |
| A folder is added to your list | Searched by All Folders from then on. | A new details file starts at the first details search. | The folder joins the index queue. |
| A folder is removed from your list | No longer searched. | Its details file is deleted. | Its vector file is deleted. |
| A listed folder can't be found | Left out, and named under the box: "Not searched: Backup (not found)". | Its records are kept. | Its vectors are kept. |

#### Keeping the results current

- The search runs again when an import, an undo of an import, a merge, or an undo of a merge finishes.
- While results are shown and the window has the focus, the search also runs again quietly on a timer. It waits five seconds after the last run finished, or ten times as long as that run took when that is longer, so a search that takes three seconds is repeated every thirty.
- A quiet run sends its results in one piece at the end, and the grid changes only when the results differ, so nothing flickers and the status bar doesn't show "Searching…".
- The selection, the scroll position, and the preview are kept while their photo is still in the results. When it has gone, the selection clears and the preview closes, as they do in a folder.
- A quiet run reads the details of new and changed photos, and writes the cache when it read any.
- A quiet run stops when you type, since the new query replaces it.
- When the selected folder itself has gone, the results clear and the grid says the folder is no longer there.
- Show in Its Folder on a photo that has gone since the last run selects its folder and shows "That photo is no longer there."
- A file that is still being copied in by another program can be read part way. Its size and time change when the copy ends, so its key does, and it is read again. Import's own partial files are hidden and are never listed.

#### Keeping the contents index current

A photo reaches the index queue from four places:

| Source | When | Covers |
|---|---|---|
| The selected folder's listing | Every time the listing changes | Photos added to the folder you are looking at, within a second or two |
| An import or a merge | When it finishes | The copied photos are queued, and the moved photos' vectors are carried |
| A search's walk | Every search, quiet runs included | Any photo in scope without a vector, which goes to the front of the queue |
| A sweep of every folder in your list | When the app starts, when a folder is added to the list, and once an hour while the app is open | Everything else, and the only place vectors of removed photos are dropped |

- The sweep drops a vector only when it walked and could read the photo's folder, the same rule the details cache uses.
- The index thread makes a photo's thumbnail when the thumbnail cache doesn't have it.
- A photo that can't be decoded is marked so it isn't tried again until its size or time changes.
- Because the walk counts the photos in scope that have no vector, the line "3,100 photos aren't indexed yet" is exact for the search you ran, and it goes away on its own as a quiet run finds them indexed.

### Searching by what is in the picture

#### How it works

- A model of the CLIP kind has two halves. The image half turns a photo into a list of numbers (a vector, 512 numbers for the model proposed here). The text half turns words into a vector of the same kind. A photo of a dog and the word "dog" give vectors that point nearly the same way.
- Each photo's vector is worked out once and kept in a cache. A search works out the vector for your words and compares it with every photo's vector, which takes a few milliseconds for 100,000 photos.
- The words aren't a fixed list. "dog", "dog on a beach", "birthday cake", and "red car at night" all work, as far as the model has learned them.
- Everything runs on your computer. The app makes no network request for a search or for a photo.

#### What it needs

| Need | What | Notes |
|---|---|---|
| A model | Two candidates: OpenAI's CLIP ViT-B/32 (MIT licence) and Google's SigLIP base (Apache 2.0). | CLIP ViT-B/32 has about 151 million parameters, roughly 600 MB as published and about 300 MB at half precision. Apple's MobileCLIP2 is smaller and faster but its licence is for research only, so it is ruled out for an app you distribute. |
| A way to run it | `candle` (Hugging Face's Rust library, pure Rust on the CPU), or ONNX Runtime through the `ort` crate (a C++ library that `ort` downloads prebuilt and links in). | `candle-transformers` has CLIP and SigLIP modules. ONNX Runtime is usually faster on a CPU but is not Rust, which breaks "built in pure `Rust`" and adds a native library to every package. |
| A tokenizer | Turns the words into the numbers the text half reads. | The `tokenizers` crate, or about 200 lines of our own for CLIP's scheme and its vocabulary file. |
| The model on your machine | Downloaded once when you turn the feature on, with a progress bar, and checked against a SHA-256 hash. | Bundling it would add hundreds of megabytes to every installer, for a feature that is off by default. The download is a new kind of network use and goes in `SECURITY.md`. |
| Each photo's vector | The image half run once per photo, on a 224 pixel square. | The cached 320 pixel thumbnail is the input, so no photo is decoded again. This builds on the background fill in the thumbnail cache plan. |
| A vector cache | `~/.cache/medialiner/vectors/<root key>.bin`, keyed as thumbnails are. | 512 numbers at half precision is 1 KB a photo: 100 MB for 100,000 photos. It records the model's name, and a different model starts it again. |
| A cutoff | The model gives every photo a score, never a yes or no. | Results are the photos above a threshold, best first. The threshold is set from trials in P5. |

#### What isn't known yet

- How long one photo takes. Published figures are for phones and GPUs. On this machine's CPU the image half is likely to take tens of milliseconds to a few hundred, depending on the model and the library, which puts 10,000 photos somewhere between a few minutes and most of an hour. This is an estimate and P5 measures it.
- How good the results are on your photos. Models of this kind are strong on common things (dogs, beaches, cars, food, snow) and weaker on small subjects, counts ("three people"), text in the picture, and particular people. They don't know who anyone is.
- Whether `candle` on the CPU is fast enough, or the speed of ONNX Runtime is worth giving up pure Rust.
- How much the app grows. Either library adds to the binary and to build times for every platform, including the Windows cross-build.

#### How it shows in the app

- Contents search is off until you turn it on in Settings → Search, which explains the download and its size, then fetches the model.
- Once it is on, photos are indexed in the background, the selected folder first, then the rest of your list. Photos added later are picked up as "Keeping the contents index current" describes. The status bar shows "Indexing photos: 1,200 of 8,400". Indexing pauses while you scroll or preview, and can be paused from Settings.
- A plain word then matches names and folders as before, and contents as well. Results show name and folder matches first, then contents matches, best first.
- Filters narrow both: `dog taken:2012` finds dogs in photos from 2012.
- `name:dog` matches names only. A new `about:` filter matches contents only: `about:"dog on a beach"`.
- A contents search of photos not yet indexed shows a line above the results: "3,100 photos aren't indexed yet."
- The results end with "Show weaker matches", which lowers the cutoff a step each time.
- The right-click menu gains Find Similar Photos, which searches with the photo's own vector. It needs no text half and comes almost free once vectors exist.
- Settings → Search also shows the model's and the vector cache's size, and Remove Model, which deletes both and turns the feature off.
- The model reads English. Words in other languages work poorly.

### Commands and shortcuts

| Command | Shortcut | When |
|---|---|---|
| Search Photos | Ctrl+F, and `/` in the grid | A folder is selected |
| Search All Folders | Ctrl+Shift+F while the search box has the focus | The search box has the focus |
| Clear Search | Esc | A search is shown and no preview is open |
| Show in Its Folder | none (right-click menu and palette) | A result is selected |

Ctrl+Shift+F is Toggle Files Pane everywhere else, and keeps that meaning when the search box doesn't have the focus. See Q6.

## Changes

### medialiner-core

| File | Change |
|---|---|
| `search.rs` (new) | `Query::parse` with its warnings, `Query::needs_details`, `Query::matches(&FileEntry, root, Option<&Details>)`, and `run(scope, query, details, cancel, on_batch)`. `run` also reports the folders it could read, for pruning, and the roots it couldn't find. |
| `details.rs` (new) | `Details` (taken, camera, lens, ISO, has location, has EXIF), `DetailsCache` (load, get, put, save, prune, clear, usage), keyed as thumbnails are. |
| `info.rs` | `read_details(path) -> Details`, sharing the EXIF helpers `read` uses. The date taken is kept as a date and time, not only as text. |
| `lib.rs` | The two new modules in the table. |

### medialiner-native

| File | Change |
|---|---|
| `backend.rs` | The search thread, `Backend::search` and `cancel_search`, `Event::SearchBatch`, `Event::SearchDone`, and `Event::DetailsProgress`. `CacheUsage` and `ClearCache` cover both caches. |
| `app.rs` | The search state (text, scope, generation, running, warnings), results as the grid's file list, returning to the folder's listing on clear, Show in Its Folder, and the status bar text. While results are shown, `watch` runs the quiet search in place of the folder check, and the import and merge events run the search again. Results replace the grid's list only when their fingerprint differs, keeping the selection and the preview as `Event::Listing` does. `remove_folder` deletes the folder's details file. |
| `views.rs` | The search box and scope drop-down in `centre_header`, the folder text in the Files pane, the cell tooltip, the no-results view, and the menu item. |
| `commands.rs` | The four commands, and `View` gains `searching` and `search_focused`. |
| `config.rs` | `searchScope` in the prefs, and `Paths::details_dir`. |
| `overlays/settings.rs` | The details cache line and Clear Caches. |

### Contents search (P5 to P7)

| Where | Change |
|---|---|
| `crates/medialiner-vision` (new crate) | The model: loading, the image half, the text half, and the tokenizer, behind an `Embedder` trait so tests use a fake. Kept apart from `medialiner-core` so the core stays light. |
| `medialiner-core` | `vectors.rs`: the vector cache (load, get, put, save, prune, clear, usage), `carry(moves)` for a merge's journal, reuse of a record whose file has gone, and `rank(query, vectors, cutoff)`. `search.rs`: `about:`, plain words matching contents, and the merged order. |
| `medialiner-native`, `backend.rs` | An index thread at low priority that reads cached thumbnails and stores vectors, its queue fed by listings, imports, merges, searches, and the sweep, the model download with progress and a hash check, and the text half run per query. |
| `medialiner-native`, UI | Settings → Search, the indexing progress, the not-indexed line, Show weaker matches, and Find Similar Photos. |
| Packaging and CI | The new crate in the lint and test commands, and the native library in each package if ONNX Runtime is chosen. |
| Docs | The READMEs, `ARCHITECTURE.md` (the new crate), and `SECURITY.md` (the model download, and that photos and searches stay on the machine). |

### Docs

`apps/medialiner/README.md` (a Search feature, the cache in Your data, and the database wording), `apps/medialiner/native/README.md` (shortcuts, the query table, the search thread, and the modules), `ARCHITECTURE.md` (the `medialiner-core` paragraph), and `SECURITY.md` (the details cache holds camera and date data and whether a photo has a location, never the location).

## Phases

| Phase | Work | Done when |
|---|---|---|
| P1 | `search.rs`: the query, the filters that need no details, and the walk with cancel | Core tests pass |
| P2 | The search box, scope, results in the grid and Files pane, Clear Search, Show in Its Folder, the commands, and results that keep up with imports, merges, and outside changes | You can find a photo by name across all folders, open it, and jump to its folder, and a photo copied into a searched folder shows in the results without a refresh |
| P3 | `details.rs`, `read_details`, the details filters, progress, and the Storage settings | `taken:2012 camera:sony` finds the photos, and the second run reads no files |
| P4 | Docs | The READMEs, `ARCHITECTURE.md`, and `SECURITY.md` describe search |
| P5 | A trial outside the app: a small command-line program that indexes a folder and searches it, with each candidate model and library | A table of time per photo, model size, binary size, and results for 20 searches on your photos, and a choice of model and library, or a decision to stop |
| P6 | `medialiner-vision`, the vector cache, the index thread, and the model download | A folder is indexed in the background, indexing survives a restart, and a photo added after the first pass is indexed without a restart |
| P7 | Contents in the query, the cutoff and Show weaker matches, Find Similar Photos, Settings → Search, and docs | Searching "dog" in `~/Pictures` shows the dogs first |

P6 depends on the background fill in `plan-medialiner-thumbnail-cache.md` (its P4), since vectors are made from cached thumbnails.

## Tests

| Layer | Covers |
|---|---|
| Core, query | Words, quotes, minus, each filter, year and month dates, `>` and `<`, unknown filters and bad values as warnings, and case. |
| Core, run | Matching names and folder paths below the root, hidden files and folders skipped, an unreadable folder left out, the order of results, batches, and cancel. |
| Core, details | Reading the fields from the test JPEGs with an EXIF block, the modification date standing in for a missing date taken, a changed file read again, save and load, and pruning: a removed photo's record goes, a record in a folder the search didn't walk stays, and a missing root's records stay. |
| Contents | With a fake `Embedder`: the vector cache's save, load, and change of model; `rank` and the cutoff; names before contents in the merged order; `about:` and `name:`; the not-indexed count; a new photo queued by a listing, an import, and a search; a merged photo's vector carried; a removed photo's vector dropped by the sweep and kept when its root is missing. The real model is tested by hand and in the P5 trial, not in CI, since CI can't download it. |
| UI (`egui_kittest`) | Ctrl+F focuses the box. Typing shows results and Esc returns to the folder. The scope drop-down searches all folders. Preview steps through results. Show in Its Folder selects the folder and the photo. A details filter fills the cache, and Clear Caches empties it. With results shown: a photo written into the folder appears, a deleted one leaves and its preview closes, an import into the folder adds its photos, and a merge's moved photos show under their new folder. |

## Out of scope

- Recognizing particular people (faces), and reading text in a picture.
- Running the model on a graphics card.
- A model for languages other than English.
- Tags, ratings, albums, and saved searches.
- Searching by place name or on a map. `has:location` is the only location filter.
- Keywords and captions stored in the photo (IPTC and XMP). See Q5.
- Files that aren't photos.
- Sorting results by date or size.
- Fuzzy matching. A word matches only where it appears.
- A file system watcher. Changes are found by walking again, as the folder listing does today.

## Open questions

| # | Question | Recommendation |
|---|---|---|
| Q1 | Should the default scope be the selected folder or all folders? | The selected folder with its subfolders. It is quick, and the no-results view offers all folders. |
| Q2 | Is the filter list right for a first version? | Yes. `taken:` and `camera:` are the ones worth the details cache; `lens:`, `iso:`, and `has:` cost nothing more once it exists. |
| Q3 | Should `taken:` fall back to the modification date for photos with no EXIF date, such as screenshots and scans? | Yes. Otherwise a date search silently leaves them out. The Info pane shows which date a photo has. |
| Q4 | Is a rebuildable details cache acceptable, given the README's "no database"? | Yes. It is the only way to search by date taken or camera without reading every photo on every search. Without it, P3 is dropped and search covers names, folders, type, size, and modification date only. |
| Q5 | Should search read keywords and captions from IPTC and XMP? `kamadak-exif` reads neither, so it needs a new parser or crate. | Not now. Worth a look if your photos have them. |
| Q6 | Should Search All Folders have its own shortcut, given Ctrl+Shift+F is Toggle Files Pane? | Use Ctrl+Shift+F only while the search box has the focus, and the drop-down otherwise. |
| Q7 | Should results also be reachable as filters in the Files pane (a filter box there) in place of the header box? | No. One box in the header that drives both the grid and the Files pane. |
| Q8 | What version does this ship as? MediaLiner is 0.1.0. | Your call. |
| Q9 | Do you want contents search at all, given what it needs: a model download of a few hundred megabytes, a new and large dependency, and a pass over every photo? | Run P5 first. It changes nothing in the app, and it answers the speed and quality questions with your own photos. |
| Q10 | Pure Rust (`candle`) or ONNX Runtime (`ort`)? | `candle`, unless P5 shows it is too slow to be usable. Pure Rust is a stated point of these apps, and a native library complicates every package. |
| Q11 | Should the model be downloaded on first use or bundled in the installer? | Downloaded, when you turn the feature on. |
| Q12 | Where should the model come from? Hugging Face hosts both candidates; a copy on the project's GitHub releases wouldn't depend on a third party staying up. | A release asset on GitHub, with the hash in the source. |
| Q13 | Should plain words search contents, or only `about:`? | Plain words, as you described: "dog" finds dogs. `name:` and `about:` are there when you want one or the other. |
| Q14 | Should results refresh on their own while they are shown, or only on Refresh (F5) and after an import or a merge? | On their own, with the wait that grows with the search's time. A folder's listing already does this, and results that go stale would be the one place in the app that doesn't. |
| Q15 | Should a vector follow a photo that was moved or renamed outside the app, by matching its name, size, and time? The thumbnail plan's Q3 keeps the path in the key, since a wrong match there shows the wrong picture. | Yes, for vectors only, and only when the old file has gone. Indexing again costs far more than a thumbnail, and a wrong match would only misplace one photo in a contents search. |
| Q16 | Is an hourly sweep of every listed folder acceptable while the app is open? It lists folders and reads no photos unless they are new. | Yes. Searches and listings catch nearly everything sooner, and the sweep is what clears out vectors of removed photos. |
