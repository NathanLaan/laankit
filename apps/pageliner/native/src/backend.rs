//! Everything that touches disk, git or the network. Calls run on a tokio
//! runtime and report back as [`Event`]s, which the UI drains once per frame
//! ([`Backend::poll`]); nothing here blocks the UI thread.
//!
//! The logic is the Tauri host's (`apps/pageliner/src-tauri/src/main.rs`),
//! channel by channel, over the same `pageliner-core` library, so the
//! library folder and `~/.config/PageLiner` stay compatible with the
//! Electron and Tauri builds.

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Result};
use egui::ColorImage;
use marina_core::debounce::Debouncer;
use marina_core::git::{Git, SyncStatus};
use marina_core::updates::{self, UpdateCheck};
use marina_egui::about::UpdateStatus;
use pageliner_core::library::{Book, Library};
use serde_json::{json, Map, Value};
use tokio::runtime::Runtime;

use crate::config::Paths;
use crate::reader::pdf::doc::{self, PdfInfo};

/// The git identity for commits when the repo has none.
const GIT_IDENTITY: (&str, &str) = ("PageLiner", "pageliner@localhost");
/// The auto-sync debounce (`schedule_sync`).
const SYNC_DEBOUNCE: Duration = Duration::from_secs(8);
/// GitHub `owner/repo` publishing PageLiner releases: none, as in the Tauri
/// host, so About reports "latest version" without a request.
const UPDATE_REPO: Option<&str> = None;
/// Covers are decoded off-thread and scaled to at most this height
/// (cards are ~200 px tall at 100%, so this stays sharp at 2×).
const COVER_MAX_HEIGHT: u32 = 480;

/// A book's file and state, read for a reader.
pub struct OpenedBook {
    pub id: String,
    pub path: PathBuf,
    pub bytes: Arc<Vec<u8>>,
    pub state: Map<String, Value>,
    /// Measured and outlined, for PDFs.
    pub pdf: Option<PdfInfo>,
}

/// `git:getInfo`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GitInfo {
    pub enabled: bool,
    pub git_installed: bool,
    pub repo: bool,
    pub remote_url: Option<String>,
    pub branch: Option<String>,
    pub library_dir: PathBuf,
}

/// A git operation the Sync dialog waits on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitOp {
    Enable,
    Disable,
    SetRemote,
    SyncNow,
    Status,
}

/// A result coming back from the backend.
pub enum Event {
    /// The whole library (`library:list`).
    Books(Vec<Book>),
    /// `library:import` added these.
    Imported(Vec<Book>),
    ImportFailed(Vec<String>),
    Deleted(String),
    /// A decoded cover; `None` when it can't be read.
    Cover {
        id: String,
        image: Option<ColorImage>,
    },
    Opened(OpenedBook),
    OpenFailed {
        id: String,
        message: String,
    },
    /// Something failed in the background; show it as an error toast.
    Error(String),
    GitInfo(GitInfo),
    /// A Sync-dialog operation finished: an error message, or a status to show.
    GitDone {
        op: GitOp,
        result: Result<Option<SyncStatus>, String>,
    },
    Update(UpdateStatus),
}

struct Core {
    paths: Paths,
    library: Mutex<Option<Library>>,
    git: Git,
    sync: Debouncer,
    tx: Mutex<Sender<Event>>,
    /// Repainted when an event arrives; set by [`Backend::attach`].
    ctx: Mutex<Option<egui::Context>>,
}

pub struct Backend {
    rt: Runtime,
    rx: Receiver<Event>,
    core: Arc<Core>,
}

impl Backend {
    /// Opens the library (creating it on first run) and starts the runtime.
    /// Call [`Backend::attach`] with the UI's context so events wake it.
    pub fn new(paths: Paths) -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("pageliner-backend")
            .build()
            .expect("tokio runtime");
        // The debouncer spawns on this runtime when scheduled from the UI thread.
        marina_core::debounce::set_runtime(rt.handle().clone());
        let (tx, rx) = channel();
        let library = match Library::open(paths.library_dir()) {
            Ok(lib) => Some(lib),
            Err(e) => {
                let _ = tx.send(Event::Error(format!("Couldn't open the library: {e}")));
                None
            }
        };
        let core = Arc::new(Core {
            paths,
            library: Mutex::new(library),
            git: Git::new().with_default_identity(GIT_IDENTITY.0, GIT_IDENTITY.1),
            sync: Debouncer::new(),
            tx: Mutex::new(tx),
            ctx: Mutex::new(None),
        });
        let backend = Self { rt, rx, core };
        backend.list();
        if backend.core.paths.prefs().git_sync_enabled {
            let core = backend.core.clone();
            backend.rt.spawn(async move {
                if let Err(e) = core.ensure_sync_repo().await {
                    log::warn!("sync init failed: {e}");
                }
            });
        }
        backend
    }

    /// The context to repaint when an event arrives.
    pub fn attach(&self, ctx: &egui::Context) {
        let mut slot = self.core.ctx.lock().unwrap();
        if slot.is_none() {
            *slot = Some(ctx.clone());
            ctx.request_repaint();
        }
    }

    /// This frame's events, in arrival order.
    pub fn poll(&self) -> Vec<Event> {
        self.rx.try_iter().collect()
    }

    pub fn paths(&self) -> &Paths {
        &self.core.paths
    }

    fn blocking(&self, f: impl FnOnce(&Arc<Core>) + Send + 'static) {
        let core = self.core.clone();
        self.rt.spawn_blocking(move || f(&core));
    }

    // --- Library -------------------------------------------------------------

    /// `library:list`.
    pub fn list(&self) {
        self.blocking(|core| {
            if let Ok(books) = core.with_library(|l| Ok(l.books().to_vec())) {
                core.send(Event::Books(books));
            }
        });
    }

    /// `library:import`, for files picked or dropped. Files that aren't EPUB
    /// or PDF are skipped (the dialog filters them; drops can't).
    pub fn import(&self, paths: Vec<PathBuf>) {
        self.blocking(move |core| {
            let mut added = Vec::new();
            let mut failed = Vec::new();
            for path in paths {
                let format = pageliner_core::metadata::format_from_ext(&path);
                if format != "epub" && format != "pdf" {
                    failed.push(display_name(&path));
                    continue;
                }
                match core.with_library(|l| l.add(&path)) {
                    Ok(book) => added.push(book),
                    Err(e) => {
                        log::warn!("import failed: {} {e}", path.display());
                        failed.push(display_name(&path));
                    }
                }
            }
            if !added.is_empty() {
                core.schedule_sync();
                core.send(Event::Imported(added));
            }
            if !failed.is_empty() {
                core.send(Event::ImportFailed(failed));
            }
        });
    }

    /// `library:delete`.
    pub fn delete(&self, id: String) {
        self.blocking(move |core| match core.with_library(|l| l.delete(&id)) {
            Ok(_) => {
                core.schedule_sync();
                core.send(Event::Deleted(id));
            }
            Err(e) => core.send(Event::Error(format!("Couldn't remove the book: {e}"))),
        });
    }

    /// `library:coverDataUrl`, decoded for a texture.
    pub fn load_cover(&self, id: String) {
        self.blocking(move |core| {
            let path = core.with_library(|l| Ok(l.cover_file(&id))).ok().flatten();
            let image = path.and_then(|p| decode_cover(&p));
            core.send(Event::Cover { id, image });
        });
    }

    /// `library:getBookData` + `library:getState`, plus measuring PDFs.
    pub fn open(&self, id: String) {
        self.blocking(move |core| {
            let result = (|| -> Result<OpenedBook> {
                let (book, path, state) = core.with_library(|l| {
                    let book = l
                        .get(&id)
                        .cloned()
                        .ok_or_else(|| anyhow!("Book not found"))?;
                    let path = l.book_file(&id).ok_or_else(|| anyhow!("Book not found"))?;
                    Ok((book, path, l.get_state(&id)))
                })?;
                let bytes = Arc::new(
                    std::fs::read(&path)
                        .map_err(|_| anyhow!("Could not read the document file."))?,
                );
                let pdf = if book.format == "pdf" {
                    Some(doc::inspect(bytes.clone())?)
                } else {
                    None
                };
                Ok(OpenedBook {
                    id: id.clone(),
                    path,
                    bytes,
                    state,
                    pdf,
                })
            })();
            core.send(match result {
                Ok(opened) => Event::Opened(opened),
                Err(e) => Event::OpenFailed {
                    id,
                    message: e.to_string(),
                },
            });
        });
    }

    /// `library:setState`: merge `patch` into the book's state.
    pub fn set_state(&self, id: String, patch: Value) {
        self.blocking(
            move |core| match core.with_library(|l| l.set_state(&id, &patch)) {
                Ok(_) => core.schedule_sync(),
                Err(e) => core.send(Event::Error(format!("Couldn't save reading state: {e}"))),
            },
        );
    }

    /// [`Backend::set_state`], on this thread (on exit, when the runtime is
    /// about to stop).
    pub fn set_state_now(&self, id: &str, patch: &Value) -> Result<()> {
        self.core
            .with_library(|l| l.set_state(id, patch).map(|_| ()))
    }

    /// `ui:setPrefs`.
    pub fn set_prefs(&self, patch: Value) {
        if let Err(e) = self.core.paths.set_prefs(&patch) {
            self.core
                .send(Event::Error(format!("Failed to save preference: {e}")));
        }
    }

    // --- Git sync ------------------------------------------------------------

    /// `git:getInfo`.
    pub fn git_info(&self) {
        let core = self.core.clone();
        self.rt.spawn(async move {
            let info = core.git_info().await;
            core.send(Event::GitInfo(info));
        });
    }

    fn git_op<F>(&self, op: GitOp, f: impl FnOnce(Arc<Core>) -> F)
    where
        F: std::future::Future<Output = Result<Option<SyncStatus>>> + Send + 'static,
    {
        let core = self.core.clone();
        let fut = f(core.clone());
        self.rt.spawn(async move {
            let result = fut.await.map_err(|e| e.to_string());
            core.send(Event::GitDone { op, result });
            let info = core.git_info().await;
            core.send(Event::GitInfo(info));
        });
    }

    /// `git:enable` then `setUIPrefs({ gitSyncEnabled: true })`; or just the pref off.
    pub fn set_sync_enabled(&self, on: bool) {
        let op = if on { GitOp::Enable } else { GitOp::Disable };
        self.git_op(op, move |core| async move {
            if on {
                core.ensure_sync_repo().await?;
            }
            core.paths.set_prefs(&json!({ "gitSyncEnabled": on }))?;
            Ok(None)
        });
    }

    /// `git:setRemote`: set, or remove with `None`.
    pub fn set_remote(&self, url: Option<String>) {
        self.git_op(GitOp::SetRemote, move |core| async move {
            let dir = core.dir()?;
            if !core.git.is_repo(&dir).await {
                core.ensure_sync_repo().await?;
            }
            match url {
                Some(url) => {
                    core.git.set_remote_url(&dir, &url).await?;
                }
                None => {
                    let _ = core.git.remove_remote(&dir).await;
                }
            }
            Ok(None)
        });
    }

    /// `git:syncNow`: commit, pull (rebase), push.
    pub fn sync_now(&self) {
        self.git_op(GitOp::SyncNow, |core| async move {
            let dir = core.dir()?;
            let git = &core.git;
            if !git.is_repo(&dir).await {
                return Err(anyhow!("Sync is not set up."));
            }
            git.configure_user(&dir).await?;
            git.commit_all(&dir, "Sync library").await?;
            let pulled = git.sync_pull(&dir).await;
            if !pulled.success {
                return Err(anyhow!("Pull failed: {}", pulled.error.unwrap_or_default()));
            }
            let pushed = git.sync_push(&dir).await;
            if !pushed.success {
                return Err(anyhow!("Push failed: {}", pushed.error.unwrap_or_default()));
            }
            // The pull may have brought in books and reading state.
            core.reopen();
            Ok(Some(git.sync_status(&dir).await))
        });
    }

    /// `git:getStatus`.
    pub fn git_status(&self) {
        self.git_op(GitOp::Status, |core| async move {
            let dir = core.dir()?;
            if !core.git.is_repo(&dir).await {
                return Ok(Some(SyncStatus::NoRepo));
            }
            Ok(Some(core.git.sync_status(&dir).await))
        });
    }

    // --- About ---------------------------------------------------------------

    /// The About dialog's update check (`update:checkNow`).
    pub fn check_updates(&self) {
        let core = self.core.clone();
        self.rt.spawn(async move {
            let status = if cfg!(debug_assertions) {
                UpdateStatus::Unavailable { dev: true }
            } else {
                match UPDATE_REPO {
                    None => UpdateStatus::Unavailable { dev: false },
                    Some(repo) => match updates::check(repo, env!("CARGO_PKG_VERSION")).await {
                        Ok(UpdateCheck::Available(rel)) => UpdateStatus::Available {
                            version: rel.version,
                            notes: rel.notes,
                        },
                        Ok(UpdateCheck::UpToDate { .. }) => {
                            UpdateStatus::Unavailable { dev: false }
                        }
                        Err(e) => UpdateStatus::Error(e.to_string()),
                    },
                }
            };
            core.send(Event::Update(status));
        });
    }
}

impl Core {
    fn send(&self, event: Event) {
        let _ = self.tx.lock().unwrap().send(event);
        if let Some(ctx) = self.ctx.lock().unwrap().as_ref() {
            ctx.request_repaint();
        }
    }

    fn with_library<T>(&self, f: impl FnOnce(&mut Library) -> Result<T>) -> Result<T> {
        let mut guard = self.library.lock().unwrap();
        let lib = guard
            .as_mut()
            .ok_or_else(|| anyhow!("The library isn't open"))?;
        f(lib)
    }

    fn dir(&self) -> Result<PathBuf> {
        self.with_library(|l| Ok(l.dir.clone()))
    }

    /// Re-reads the index after git rewrote it, and refreshes the UI.
    fn reopen(&self) {
        match Library::open(self.paths.library_dir()) {
            Ok(lib) => {
                let books = lib.books().to_vec();
                *self.library.lock().unwrap() = Some(lib);
                self.send(Event::Books(books));
            }
            Err(e) => self.send(Event::Error(format!("Couldn't reload the library: {e}"))),
        }
    }

    fn write_gitignore(&self, dir: &std::path::Path) {
        let content = [
            "# PageLiner sync: the library index + reading state (positions, bookmarks,",
            "# highlights) are versioned; large book blobs and derived covers stay local.",
            "books/",
            "covers/",
            "*.tmp",
            "",
        ]
        .join("\n");
        let _ = std::fs::write(dir.join(".gitignore"), content);
    }

    /// Idempotent: init the repo if needed, ensure identity and `.gitignore`,
    /// and commit so there's a branch to push.
    async fn ensure_sync_repo(&self) -> Result<()> {
        let dir = self.dir()?;
        if !self.git.is_installed().await {
            return Err(anyhow!("Git is not installed or not on PATH."));
        }
        if !self.git.is_repo(&dir).await {
            self.git.init_repo(&dir, None).await?;
        } else {
            self.git.configure_user(&dir).await?;
        }
        self.write_gitignore(&dir);
        self.git
            .commit_all(&dir, "Configure PageLiner sync")
            .await?;
        Ok(())
    }

    async fn git_info(&self) -> GitInfo {
        let dir = self.dir().unwrap_or_default();
        let installed = self.git.is_installed().await;
        let mut info = GitInfo {
            enabled: self.paths.prefs().git_sync_enabled,
            git_installed: installed,
            library_dir: dir.clone(),
            ..Default::default()
        };
        if installed && self.git.is_repo(&dir).await {
            info.repo = true;
            info.remote_url = self.git.get_remote_url(&dir).await;
            info.branch = self.git.get_branch(&dir).await;
        }
        info
    }

    /// Debounced commit and push. Never pulls: that's for an explicit Sync
    /// Now, to avoid surprise rebases mid-session.
    fn schedule_sync(self: &Arc<Self>) {
        if !self.paths.prefs().git_sync_enabled {
            return;
        }
        let me = self.clone();
        self.sync.schedule(SYNC_DEBOUNCE, move || async move {
            let Ok(dir) = me.dir() else { return };
            match me.git.commit_all(&dir, "Update library").await {
                Ok(true) => {
                    me.git.sync_push(&dir).await;
                }
                Ok(false) => {}
                Err(e) => log::warn!("auto-sync failed: {e}"),
            }
        });
    }
}

fn display_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Reads and downscales a cover image (`None` for SVG or unreadable files).
fn decode_cover(path: &std::path::Path) -> Option<ColorImage> {
    let bytes = std::fs::read(path).ok()?;
    let img = image::load_from_memory(&bytes).ok()?;
    let img = if img.height() > COVER_MAX_HEIGHT {
        img.resize(
            u32::MAX,
            COVER_MAX_HEIGHT,
            image::imageops::FilterType::Triangle,
        )
    } else {
        img
    };
    let rgba = img.into_rgba8();
    Some(ColorImage::from_rgba_unmultiplied(
        [rgba.width() as usize, rgba.height() as usize],
        rgba.as_raw(),
    ))
}
