//! The work off the UI thread, replying with [`Event`]s.
//!
//! | Thread | Does |
//! |--------|------|
//! | Files (one) | Folder listings, file details, merge plans, the cache's size, and the merge journals, in order |
//! | Pictures (up to four) | Previews first, then thumbnails, in the order the grid wants them |
//! | Merge (one per merge or undo) | Moving files, with progress and cancel |
//! | Devices (one) | Looks every two seconds for a drive or media device plugged in or removed |
//! | Scan (one per device) | Lists a device's files for the Import dialog |
//! | Import (one per import) | Copying from a device, with progress and cancel, then deleting from it when asked |
//!
//! The grid sends the thumbnails it wants each frame with
//! [`Backend::want_thumbs`], which replaces the waiting ones, so photos
//! scrolled past are never decoded.

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime};

use egui::ColorImage;
use medialiner_core::devices::{self, Location, Volume};
use medialiner_core::import::{self, Item, Progress, Record, Undone};
use medialiner_core::info::{self, FileInfo};
use medialiner_core::merge::{self, Journal, MergePlan, Outcome};
use medialiner_core::scan::{self, FileEntry};
use medialiner_core::thumbs::{self, Cache, PREVIEW_MAX};

/// Thumbnails not shown for this long are removed at start.
const CACHE_AGE: Duration = Duration::from_secs(90 * 24 * 60 * 60);
/// Merge journals kept for undo.
pub const JOURNALS_KEPT: usize = 10;
/// How often the plugged-in devices are looked at.
const DEVICE_CHECK: Duration = Duration::from_secs(2);
/// The least time between two progress events of an import.
const PROGRESS_EVERY: Duration = Duration::from_millis(60);

/// A thumbnail the grid wants.
#[derive(Clone, Debug, PartialEq)]
pub struct ThumbJob {
    pub path: PathBuf,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

impl ThumbJob {
    pub fn of(f: &FileEntry) -> Self {
        Self {
            path: f.path.clone(),
            size: f.size,
            modified: f.modified,
        }
    }

    pub fn key(&self) -> String {
        thumbs::key(&self.path, self.size, self.modified)
    }
}

/// Merge or undo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeKind {
    Merge,
    Undo,
}

// Events go through a channel one at a time; boxing the big ones gains nothing.
#[allow(clippy::large_enum_variant)]
pub enum Event {
    Listing {
        dir: PathBuf,
        deep: bool,
        files: Result<Vec<FileEntry>, String>,
    },
    Thumb {
        path: PathBuf,
        key: String,
        /// `None`: it couldn't be decoded.
        image: Option<ColorImage>,
        /// The embedded EXIF thumbnail, a better one follows.
        quick: bool,
    },
    Preview {
        path: PathBuf,
        image: Result<ColorImage, String>,
    },
    Info {
        path: PathBuf,
        info: Result<FileInfo, String>,
    },
    MergePlanned {
        source: PathBuf,
        dest: PathBuf,
        plan: Result<MergePlan, String>,
    },
    MergeProgress {
        done: usize,
        total: usize,
    },
    MergeDone {
        kind: MergeKind,
        outcome: Result<Outcome, String>,
    },
    CacheUsage {
        bytes: u64,
        count: usize,
    },
    Journals(Vec<Journal>),
    /// A drive or media device was plugged in.
    DeviceAdded(Volume),
    DeviceRemoved(Volume),
    /// The devices there now, for Import from Device.
    Devices(Vec<Volume>),
    DeviceScanning {
        location: Location,
        found: usize,
    },
    DeviceScanned {
        volume: Volume,
        items: Result<Vec<Item>, String>,
        record: Option<Record>,
        /// Found by the watcher, so the dialog is offered.
        offer: bool,
    },
    ImportProgress(Progress),
    /// The copying is over. With `ask_delete`, the import waits to hear
    /// whether to delete the copied files from the device.
    ImportDone {
        outcome: import::Outcome,
        ask_delete: bool,
    },
    ImportDeleting {
        done: usize,
        total: usize,
    },
    ImportDeleted {
        deleted: usize,
        failed: Vec<(String, String)>,
    },
    ImportUndone(Result<Undone, String>),
    ImportRecords(Vec<Record>),
}

enum Job {
    List {
        dir: PathBuf,
        deep: bool,
    },
    Info(PathBuf),
    PlanMerge {
        source: PathBuf,
        dest: PathBuf,
        duplicates: PathBuf,
    },
    CacheUsage,
    ClearCache,
    Journals,
    /// The devices there now. Tests get none: they pass their own.
    Devices(bool),
    UndoImport,
    ImportRecords,
    ForgetDevice(String),
}

#[derive(Default)]
struct Queue {
    previews: VecDeque<PathBuf>,
    thumbs: VecDeque<ThumbJob>,
    /// Being decoded now, so not handed out twice.
    busy: HashSet<PathBuf>,
    stop: bool,
}

struct Shared {
    queue: Mutex<Queue>,
    ready: Condvar,
    ctx: Mutex<Option<egui::Context>>,
}

impl Shared {
    fn repaint(&self) {
        if let Some(ctx) = &*self.ctx.lock().unwrap() {
            ctx.request_repaint();
        }
    }
}

/// Paths the backend works in.
pub struct Config {
    pub thumbs_dir: PathBuf,
    pub merges_dir: PathBuf,
    pub imports_dir: PathBuf,
    /// Tests: move by copying, as across drives.
    pub force_copy: bool,
    /// Whether to look for devices being plugged in. Tests pass their own.
    pub watch_devices: bool,
}

pub struct Backend {
    jobs: Sender<Job>,
    events_tx: Sender<Event>,
    events: Receiver<Event>,
    shared: Arc<Shared>,
    merges_dir: PathBuf,
    force_copy: bool,
    /// The running merge's cancel flag.
    cancel: Option<Arc<AtomicBool>>,
    last_wanted: Vec<PathBuf>,
    imports_dir: PathBuf,
    watch_devices: bool,
    /// The running import's cancel flag, and where its answer about
    /// deleting from the device goes.
    import: Option<(Arc<AtomicBool>, Sender<bool>)>,
}

impl Backend {
    pub fn new(config: Config) -> Self {
        let (events_tx, events) = channel();
        let (jobs, jobs_rx) = channel();
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue::default()),
            ready: Condvar::new(),
            ctx: Mutex::new(None),
        });
        let cache = Cache::new(config.thumbs_dir.clone());
        {
            let tx = events_tx.clone();
            let shared = shared.clone();
            let cache = cache.clone();
            let merges = config.merges_dir.clone();
            let imports = config.imports_dir.clone();
            std::thread::Builder::new()
                .name("medialiner-files".into())
                .spawn(move || files_thread(jobs_rx, tx, shared, cache, merges, imports))
                .expect("files thread");
        }
        if config.watch_devices {
            let tx = events_tx.clone();
            let shared = shared.clone();
            std::thread::Builder::new()
                .name("medialiner-devices".into())
                .spawn(move || devices_thread(tx, shared))
                .expect("devices thread");
        }
        let workers = std::thread::available_parallelism()
            .map_or(2, |n| n.get())
            .clamp(1, 4);
        for i in 0..workers {
            let tx = events_tx.clone();
            let shared = shared.clone();
            let cache = cache.clone();
            std::thread::Builder::new()
                .name(format!("medialiner-pictures-{i}"))
                .spawn(move || picture_thread(tx, shared, cache))
                .expect("picture thread");
        }
        // Old thumbnails, once, in the background.
        std::thread::spawn(move || {
            let n = cache.clean(CACHE_AGE);
            if n > 0 {
                log::info!("removed {n} old thumbnails");
            }
        });
        Self {
            jobs,
            events_tx,
            events,
            shared,
            merges_dir: config.merges_dir,
            force_copy: config.force_copy,
            cancel: None,
            last_wanted: Vec::new(),
            imports_dir: config.imports_dir,
            watch_devices: config.watch_devices,
            import: None,
        }
    }

    /// So events wake the UI.
    pub fn attach(&self, ctx: &egui::Context) {
        *self.shared.ctx.lock().unwrap() = Some(ctx.clone());
    }

    pub fn poll(&self) -> Vec<Event> {
        self.events.try_iter().collect()
    }

    fn job(&self, job: Job) {
        let _ = self.jobs.send(job);
    }

    pub fn list(&self, dir: &Path, deep: bool) {
        self.job(Job::List {
            dir: dir.to_owned(),
            deep,
        });
    }

    pub fn info(&self, path: &Path) {
        self.job(Job::Info(path.to_owned()));
    }

    pub fn plan_merge(&self, source: &Path, dest: &Path, duplicates: &Path) {
        self.job(Job::PlanMerge {
            source: source.to_owned(),
            dest: dest.to_owned(),
            duplicates: duplicates.to_owned(),
        });
    }

    pub fn cache_usage(&self) {
        self.job(Job::CacheUsage);
    }

    pub fn clear_cache(&self) {
        self.job(Job::ClearCache);
    }

    pub fn journals(&self) {
        self.job(Job::Journals);
    }

    /// Replaces the waiting thumbnails with `wanted`, in order.
    pub fn want_thumbs(&mut self, wanted: Vec<ThumbJob>) {
        let paths: Vec<PathBuf> = wanted.iter().map(|j| j.path.clone()).collect();
        if paths == self.last_wanted {
            return;
        }
        self.last_wanted = paths;
        let mut q = self.shared.queue.lock().unwrap();
        q.thumbs = wanted
            .into_iter()
            .filter(|j| !q.busy.contains(&j.path))
            .collect();
        drop(q);
        self.shared.ready.notify_all();
    }

    /// Decodes `path` for the preview, ahead of any thumbnail, then each
    /// of `ahead`.
    pub fn want_preview(&self, path: &Path, ahead: &[PathBuf]) {
        let mut q = self.shared.queue.lock().unwrap();
        q.previews.clear();
        for p in std::iter::once(path).chain(ahead.iter().map(PathBuf::as_path)) {
            if !q.busy.contains(p) {
                q.previews.push_back(p.to_owned());
            }
        }
        drop(q);
        self.shared.ready.notify_all();
    }

    pub fn merging(&self) -> bool {
        self.cancel.is_some()
    }

    /// Runs a merge on its own thread.
    pub fn run_merge(&mut self, plan: MergePlan, remove_source: bool) {
        let cancel = Arc::new(AtomicBool::new(false));
        self.cancel = Some(cancel.clone());
        let tx = self.events_tx.clone();
        let shared = self.shared.clone();
        let dir = self.merges_dir.clone();
        let force_copy = self.force_copy;
        std::thread::spawn(move || {
            let progress = |done, total| {
                let _ = tx.send(Event::MergeProgress { done, total });
                shared.repaint();
            };
            let outcome = merge::run(&plan, remove_source, &dir, &cancel, force_copy, progress)
                .map_err(|e| format!("{e:#}"));
            merge::prune(&dir, JOURNALS_KEPT);
            let _ = tx.send(Event::MergeDone {
                kind: MergeKind::Merge,
                outcome,
            });
            shared.repaint();
        });
    }

    /// Undoes a merge on its own thread.
    pub fn undo_merge(&mut self, journal: Journal) {
        self.cancel = Some(Arc::new(AtomicBool::new(false)));
        let tx = self.events_tx.clone();
        let shared = self.shared.clone();
        let force_copy = self.force_copy;
        std::thread::spawn(move || {
            let progress = |done, total| {
                let _ = tx.send(Event::MergeProgress { done, total });
                shared.repaint();
            };
            let outcome = merge::undo(&journal, force_copy, progress).map_err(|e| format!("{e:#}"));
            let _ = tx.send(Event::MergeDone {
                kind: MergeKind::Undo,
                outcome,
            });
            shared.repaint();
        });
    }

    /// Stops the running merge after its current file.
    pub fn cancel_merge(&self) {
        if let Some(c) = &self.cancel {
            c.store(true, Ordering::Relaxed);
        }
    }

    /// The merge's done event arrived.
    pub fn merge_finished(&mut self) {
        self.cancel = None;
    }

    // --- Importing ----------------------------------------------------------

    /// Asks for the devices there now.
    pub fn devices(&self) {
        self.job(Job::Devices(self.watch_devices));
    }

    /// Lists `volume`'s files on a thread of its own.
    pub fn scan_device(&self, volume: Volume, offer: bool) {
        let tx = self.events_tx.clone();
        let shared = self.shared.clone();
        let dir = self.imports_dir.clone();
        std::thread::spawn(move || {
            let location = volume.location.clone();
            let mut last = Instant::now();
            let mut found = |n: usize| {
                if last.elapsed() >= PROGRESS_EVERY {
                    last = Instant::now();
                    let _ = tx.send(Event::DeviceScanning {
                        location: location.clone(),
                        found: n,
                    });
                    shared.repaint();
                }
            };
            let items = import::open_source(&volume)
                .and_then(|mut src| src.list(&AtomicBool::new(false), &mut found))
                .map_err(|e| format!("{e:#}"));
            let record = Record::load(&dir, &volume.key);
            let _ = tx.send(Event::DeviceScanned {
                volume,
                items,
                record,
                offer,
            });
            shared.repaint();
        });
    }

    pub fn importing(&self) -> bool {
        self.import.is_some()
    }

    /// Copies `items` from `volume` into `dest` on a thread of its own.
    /// With `ask_delete`, the thread then waits for [`Self::import_delete`].
    pub fn run_import(
        &mut self,
        volume: Volume,
        items: Vec<Item>,
        dest: PathBuf,
        ask_delete: bool,
    ) {
        let cancel = Arc::new(AtomicBool::new(false));
        let (answer_tx, answer_rx) = channel::<bool>();
        self.import = Some((cancel.clone(), answer_tx));
        let tx = self.events_tx.clone();
        let shared = self.shared.clone();
        let dir = self.imports_dir.clone();
        std::thread::spawn(move || {
            let mut source = match import::open_source(&volume) {
                Ok(s) => s,
                Err(e) => {
                    let why = format!("{e:#}");
                    let outcome = import::Outcome {
                        failed: items
                            .iter()
                            .map(|i| (i.name.clone(), why.clone()))
                            .collect(),
                        ..import::Outcome::default()
                    };
                    let _ = tx.send(Event::ImportDone {
                        outcome,
                        ask_delete: false,
                    });
                    shared.repaint();
                    return;
                }
            };
            let mut last = Instant::now();
            let mut progress = |p: &Progress| {
                if last.elapsed() >= PROGRESS_EVERY || p.done == p.total {
                    last = Instant::now();
                    let _ = tx.send(Event::ImportProgress(p.clone()));
                    shared.repaint();
                }
            };
            let outcome = import::run(source.as_mut(), &items, &dest, &cancel, &mut progress);
            if let Err(e) = import::finish(&dir, &volume, &dest, &outcome) {
                log::warn!("recording the import: {e:#}");
            }
            let ask = ask_delete && !outcome.copied.is_empty() && !outcome.removed;
            let _ = tx.send(Event::ImportDone {
                outcome: outcome.clone(),
                ask_delete: ask,
            });
            shared.repaint();
            if !ask || !answer_rx.recv().unwrap_or(false) {
                return;
            }
            let mut deleting = |done, total| {
                let _ = tx.send(Event::ImportDeleting { done, total });
                shared.repaint();
            };
            let (deleted, failed) =
                import::delete_copied(source.as_mut(), &outcome, &dir, &mut deleting);
            let _ = tx.send(Event::ImportDeleted { deleted, failed });
            shared.repaint();
        });
    }

    /// Stops the running import.
    pub fn cancel_import(&self) {
        if let Some((c, _)) = &self.import {
            c.store(true, Ordering::Relaxed);
        }
    }

    /// The answer to "delete the copied files from the device?".
    pub fn import_delete(&mut self, delete: bool) {
        if let Some((_, answer)) = &self.import {
            let _ = answer.send(delete);
        }
        if !delete {
            self.import = None;
        }
    }

    /// The import has nothing more to do.
    pub fn import_finished(&mut self) {
        self.import = None;
    }

    pub fn undo_import(&self) {
        self.job(Job::UndoImport);
    }

    pub fn import_records(&self) {
        self.job(Job::ImportRecords);
    }

    pub fn forget_device(&self, key: &str) {
        self.job(Job::ForgetDevice(key.to_owned()));
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        self.shared.queue.lock().unwrap().stop = true;
        self.shared.ready.notify_all();
    }
}

fn files_thread(
    jobs: Receiver<Job>,
    tx: Sender<Event>,
    shared: Arc<Shared>,
    cache: Cache,
    merges: PathBuf,
    imports: PathBuf,
) {
    for job in jobs {
        let event = match job {
            Job::List { dir, deep } => {
                let files = if deep {
                    scan::files_deep(&dir)
                } else {
                    scan::files(&dir)
                };
                Event::Listing {
                    dir,
                    deep,
                    files: files.map_err(|e| e.to_string()),
                }
            }
            Job::Info(path) => Event::Info {
                info: info::read(&path).map_err(|e| e.to_string()),
                path,
            },
            Job::PlanMerge {
                source,
                dest,
                duplicates,
            } => Event::MergePlanned {
                plan: merge::plan(&source, &dest, &duplicates).map_err(|e| format!("{e:#}")),
                source,
                dest,
            },
            Job::CacheUsage => {
                let (bytes, count) = cache.usage();
                Event::CacheUsage { bytes, count }
            }
            Job::ClearCache => {
                if let Err(e) = cache.clear() {
                    log::warn!("clearing the thumbnail cache: {e}");
                }
                let (bytes, count) = cache.usage();
                Event::CacheUsage { bytes, count }
            }
            Job::Journals => Event::Journals(merge::journals(&merges)),
            Job::Devices(real) => {
                Event::Devices(if real { devices::volumes() } else { Vec::new() })
            }
            Job::UndoImport => {
                Event::ImportUndone(import::undo(&imports).map_err(|e| format!("{e:#}")))
            }
            Job::ImportRecords => Event::ImportRecords(import::records(&imports)),
            Job::ForgetDevice(key) => {
                if let Err(e) = import::forget(&imports, &key) {
                    log::warn!("forgetting a device: {e}");
                }
                Event::ImportRecords(import::records(&imports))
            }
        };
        if tx.send(event).is_err() {
            return;
        }
        shared.repaint();
    }
}

/// Tells the app when a device is plugged in or removed. The devices
/// there at the start are not reported.
fn devices_thread(tx: Sender<Event>, shared: Arc<Shared>) {
    let mut before = devices::volumes();
    loop {
        // Asleep in short steps, so closing the app isn't held up.
        let until = Instant::now() + DEVICE_CHECK;
        while Instant::now() < until {
            if shared.queue.lock().unwrap().stop {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let now = devices::volumes();
        let mut changed = false;
        for v in devices::added(&before, &now) {
            changed = true;
            if tx.send(Event::DeviceAdded(v)).is_err() {
                return;
            }
        }
        for v in devices::added(&now, &before) {
            changed = true;
            if tx.send(Event::DeviceRemoved(v)).is_err() {
                return;
            }
        }
        if changed {
            shared.repaint();
        }
        before = now;
    }
}

enum Picture {
    Preview(PathBuf),
    Thumb(ThumbJob),
}

fn picture_thread(tx: Sender<Event>, shared: Arc<Shared>, cache: Cache) {
    loop {
        let job = {
            let mut q = shared.queue.lock().unwrap();
            loop {
                if q.stop {
                    return;
                }
                if let Some(p) = q.previews.pop_front() {
                    q.busy.insert(p.clone());
                    break Picture::Preview(p);
                }
                if let Some(t) = q.thumbs.pop_front() {
                    q.busy.insert(t.path.clone());
                    break Picture::Thumb(t);
                }
                q = shared.ready.wait(q).unwrap();
            }
        };
        let (path, event) = match job {
            Picture::Preview(path) => {
                let image = thumbs::decode(&path, PREVIEW_MAX)
                    .map(|img| to_color_image(&img))
                    .map_err(|e| format!("{e:#}"));
                (path.clone(), Event::Preview { path, image })
            }
            Picture::Thumb(job) => {
                let key = job.key();
                // The embedded EXIF thumbnail first, when there's no cached one.
                if cache.load(&key).is_none() {
                    if let Some(img) = thumbs::quick(&job.path) {
                        let _ = tx.send(Event::Thumb {
                            path: job.path.clone(),
                            key: key.clone(),
                            image: Some(to_color_image(&img)),
                            quick: true,
                        });
                        shared.repaint();
                    }
                }
                let image = match cache.thumbnail(&job.path, job.size, job.modified) {
                    Ok(img) => Some(to_color_image(&img)),
                    Err(e) => {
                        log::debug!("thumbnail: {e:#}");
                        None
                    }
                };
                (
                    job.path.clone(),
                    Event::Thumb {
                        path: job.path,
                        key,
                        image,
                        quick: false,
                    },
                )
            }
        };
        shared.queue.lock().unwrap().busy.remove(&path);
        if tx.send(event).is_err() {
            return;
        }
        shared.repaint();
    }
}

fn to_color_image(img: &image::RgbaImage) -> ColorImage {
    ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw())
}
