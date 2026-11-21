//! Everything that touches disk or the network. Calls run on a tokio
//! runtime and report back as [`Event`]s, which the UI drains once per frame
//! ([`Backend::poll`]); nothing here blocks the UI thread.
//!
//! A project is a folder, held by `noteliner-core`'s workspace. The workspace
//! sits behind one async mutex, shared with the MCP server, so operations
//! never interleave mid-write. The core's index is the one that counts: the
//! app's copy is replaced by [`Event::Tree`] after a folder operation, a
//! change made by another program, or a write by an AI assistant.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

use std::time::SystemTime;

use anyhow::Result;
use laankit_core::updates::{self, UpdateCheck};
use laankit_egui::about::UpdateStatus;
use noteliner_core::links::LinkGraph;
use noteliner_core::mcp::{self, ConfirmFuture, McpHost, McpServer};
use noteliner_core::project::{FileKind, Project, TrashUnavailable, Written};
use noteliner_core::tree;
use noteliner_core::window_state::WindowState;
use noteliner_core::workspace::{Shared, Workspace};
use noteliner_core::{export, frontmatter, import, templates};
use serde_json::{json, Value};
use tokio::runtime::Runtime;

use crate::config::Paths;

/// GitHub `owner/repo` publishing NoteLiner releases (`update_repo`), as
/// `native-v*` tags.
const UPDATE_REPO: &str = "NathanLaan/laankit";

/// `DEFAULT_PRESENTATION` in `lib/slides.js`: what "Presentation" seeds.
pub fn default_presentation() -> Value {
    presentation_in(noteliner_slides::SlideTheme::DEFAULT_ID)
}

/// The slide theme of a new presentation. `None` keeps the theme of the
/// template it is made from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeckTheme(pub Option<String>);

/// The starting settings of a presentation in slide theme `theme`.
fn presentation_in(theme: &str) -> Value {
    json!({
        "theme": theme,
        "aspect": "16:9",
        "slideLevel": 2,
        "header": "",
        "footer": "",
        "slideNumbers": true,
        "firstSlideTitle": true,
    })
}

/// `blankDeckBody`: the starter body for a blank presentation.
pub fn blank_deck_body(name: &str) -> String {
    format!("# {name}\n\n<!-- notes\nOpening line.\n-->\n\n## First point\n\n- \n")
}

/// What Delete was asked to remove.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeleteTarget {
    /// A note or diagram, by id.
    File(String),
    /// A folder, by path.
    Folder(String),
}

/// A Convert/Save-to target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportKind {
    Html,
    Markdown,
    Pdf,
    /// A diagram's `.mmd` file, as it is.
    Mermaid,
    /// A presentation as an editable PowerPoint file.
    Pptx,
}

/// `mcp:getStatus`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct McpStatus {
    pub enabled: bool,
    pub running: bool,
    pub socket_path: Option<PathBuf>,
    /// The executable MCP clients spawn (`noteliner --mcp-bridge`).
    pub bridge_command: PathBuf,
    pub project_open: bool,
    pub confirm_writes: bool,
    pub disabled_tools: Vec<String>,
    pub read_tools: Vec<String>,
    pub write_tools: Vec<String>,
}

/// An MCP write waiting for the user (`mcp:confirm-request`).
#[derive(Clone, Debug, PartialEq)]
pub struct McpConfirm {
    pub id: u64,
    pub tool: String,
    pub summary: String,
    pub args: Value,
}

/// A result coming back from the backend.
pub enum Event {
    /// A line for the Activity Log (MCP activity and import notes).
    Log(String),
    /// A folder was opened.
    ProjectOpened {
        folder: PathBuf,
        index: Value,
        /// Every folder under it, by path.
        folders: Vec<String>,
        /// It holds more files than NoteLiner reads.
        truncated: bool,
    },
    OpenFailed(String),
    /// The project was closed.
    ProjectClosed,
    /// The selected file as it is on disk: a note with its frontmatter.
    FileLoaded {
        id: String,
        content: String,
    },
    /// A save finished for this note.
    Saved {
        id: String,
        content: String,
        /// What was written, when NoteLiner put a field back into the
        /// frontmatter, and whether the frontmatter could be read.
        result: Result<Written, String>,
        /// The note's index entry after the write (`modifiedAt`, `deck`,
        /// `tags`).
        entry: Option<Value>,
    },
    /// `file:create` / `file:duplicate` / an import added this entry.
    Created(Value),
    Renamed {
        id: String,
        entry: Option<Value>,
    },
    Deleted(String),
    /// The trash couldn't take it: ask before deleting it for good.
    DeleteFailed {
        target: DeleteTarget,
        message: String,
    },
    /// The index and the folders, as they are now: after a folder was added,
    /// renamed, moved or deleted, a note was moved, something changed on
    /// disk, or an AI assistant wrote to the project.
    Tree {
        index: Value,
        folders: Vec<String>,
    },
    /// A folder was made: show it, open.
    FolderCreated(String),
    /// A folder has a new path (renamed or moved).
    FolderMoved {
        from: String,
        to: String,
    },
    /// A save was not written: another program changed the file since the
    /// app last read or wrote it. `content` is the edit, `disk` the file.
    /// `report` is as for [`Event::FileChanged`].
    SaveConflict {
        id: String,
        content: String,
        disk: String,
        report: u64,
    },
    /// An open file was changed on disk by another program. `report` numbers
    /// the change: the app sends the last one it handled with each save of
    /// the file, so a save sent before it heard of this change is refused.
    FileChanged {
        id: String,
        content: String,
        report: u64,
    },
    /// An AI assistant wrote to the project: check the open files now.
    McpChanged,
    /// The app's tag changes were saved.
    IndexSaved(Result<(), String>),
    /// A note's `presentation:` block was set: the new one (or null).
    Presentation {
        id: String,
        outcome: Result<Value, String>,
    },
    Templates(Vec<Value>),
    Mcp(McpStatus),
    McpConfirm(McpConfirm),
    Update(UpdateStatus),
    /// A Save-to finished: the Downloads folder to reveal, or why not.
    Exported {
        kind: ExportKind,
        result: Result<PathBuf, String>,
        /// What could not be saved in full, one line for each.
        warnings: Vec<String>,
    },
    /// `file:import` of the file `name` finished: `{ entry, stats }`.
    Imported {
        name: String,
        result: Result<Value, String>,
    },
    // --- The Search, Backlinks and Attachments panes ---
    SearchResults {
        query: String,
        results: Vec<Value>,
    },
    Backlinks {
        id: String,
        links: Vec<Value>,
    },
    /// What Files Info shows of the repository at `folder`.
    GitInfo {
        folder: PathBuf,
        result: Result<noteliner_core::git::GitInfo, noteliner_core::git::GitError>,
    },
    /// For wikilink autocomplete, with laankit-editor.
    #[allow(dead_code)]
    NoteNames(Vec<Value>),
    AttachmentAdded {
        file_id: String,
        result: Result<Value, String>,
        /// Insert its Markdown into the editor (pasted or dropped on it).
        insert: bool,
    },
    AttachmentRemoved {
        file_id: String,
        attachment_id: String,
    },
    /// Something failed in the background; show it as an error toast.
    Error(String),
}

/// An edit to write as the project closes or the app quits.
pub struct ClosingSave {
    pub save: crate::editor::SaveRequest,
    /// The app already knows that another program changed the file, and was
    /// waiting to ask about it.
    pub conflict: bool,
    /// The last change to the file that the app handled: the `report` of an
    /// [`Event::FileChanged`] or an [`Event::SaveConflict`], or 0.
    pub seen: u64,
    /// A save of this edit was sent, and the app has no answer yet: it may
    /// have been written, refused, or not run.
    pub in_flight: bool,
}

struct Core {
    paths: Paths,
    /// PDF export (Typst), built and warmed up on a background thread at
    /// startup so the first export doesn't pay the ~200 ms setup.
    exporter: Arc<noteliner_export::LazyExporter>,
    ws: Shared,
    mcp: Arc<McpServer>,
    window_state: WindowState,
    tx: Mutex<Sender<Event>>,
    /// Repainted when an event arrives; set by [`Backend::attach`].
    ctx: Arc<Mutex<Option<egui::Context>>>,
    confirms: Mutex<HashMap<u64, tokio::sync::oneshot::Sender<String>>>,
    next_confirm: AtomicU64,
    /// Each file's time and size when the app last read or wrote it, by
    /// path. A file that no longer matches was changed by another program.
    stamps: Mutex<HashMap<String, Stamp>>,
    /// The number of the last change reported to the app for each file, by
    /// path. A save that names an earlier one was sent before the app heard
    /// of the change.
    reports: Mutex<HashMap<String, u64>>,
    next_report: AtomicU64,
}

/// A file's modification time, its size, and a hash of its content.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Stamp {
    modified: Option<SystemTime>,
    len: u64,
    hash: u64,
}

impl Stamp {
    /// The time and size are the ones on disk: one `stat`, no read.
    fn looks_like(&self, (modified, len): (Option<SystemTime>, u64)) -> bool {
        self.modified == modified && self.len == len
    }
}

fn stat_of(path: &Path) -> (Option<SystemTime>, u64) {
    match std::fs::metadata(path) {
        Ok(m) => (m.modified().ok(), m.len()),
        Err(_) => (None, 0),
    }
}

fn hash_of(content: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    content.hash(&mut h);
    h.finish()
}

fn stamp_of(path: &Path) -> Stamp {
    let (modified, len) = stat_of(path);
    let hash = std::fs::read(path).map(|c| hash_of(&c)).unwrap_or(0);
    Stamp {
        modified,
        len,
        hash,
    }
}

impl Core {
    fn send(&self, event: Event) {
        let _ = self.tx.lock().unwrap().send(event);
        if let Some(ctx) = self.ctx.lock().unwrap().as_ref() {
            ctx.request_repaint();
        }
    }

    async fn maybe_start_mcp(&self) {
        if !self.paths.prefs().mcp_enabled || self.mcp.is_running() {
            return;
        }
        if !self.ws.lock().await.project.is_open() {
            return;
        }
        if let Err(e) = self.mcp.start(&self.paths.mcp_runtime()).await {
            log::warn!("[MCP] failed to start: {e}");
            self.send(Event::Log(format!("[MCP] failed to start: {e}")));
        }
    }

    fn reject_pending_confirms(&self) {
        for (_, tx) in self.confirms.lock().unwrap().drain() {
            let _ = tx.send("deny".into());
        }
    }

    async fn mcp_status(&self) -> McpStatus {
        let prefs = self.paths.prefs();
        let tools = mcp::tool_classification();
        let names = |k: &str| -> Vec<String> {
            tools[k]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|t| t.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default()
        };
        McpStatus {
            enabled: prefs.mcp_enabled,
            running: self.mcp.is_running(),
            socket_path: self.mcp.socket_path(),
            bridge_command: std::env::current_exe().unwrap_or_default(),
            project_open: self.ws.lock().await.project.is_open(),
            confirm_writes: prefs.mcp_confirm_writes,
            disabled_tools: prefs.mcp_disabled_tools,
            read_tools: names("read"),
            write_tools: names("write"),
        }
    }

    /// The file on disk, if another program changed it since the app last
    /// read or wrote it. The stamp is then the file's, so the change is
    /// reported once. A file that is gone, one the app has not read, and one
    /// that was written again with the same content have not changed.
    fn changed_on_disk(&self, project: &Project, filename: &str) -> Option<(String, u64)> {
        let path = project.file_path(filename).ok()?;
        let held = *self.stamps.lock().unwrap().get(filename)?;
        let stat = stat_of(&path);
        if stat.0.is_none() || held.looks_like(stat) {
            return None;
        }
        let disk = project.read_file(filename).ok()?;
        let now = stamp_of(&path);
        self.stamps
            .lock()
            .unwrap()
            .insert(filename.to_string(), now);
        (now.hash != held.hash).then(|| (disk, self.report(filename)))
    }

    /// Numbers a change to `filename` that the app is about to be told of.
    fn report(&self, filename: &str) -> u64 {
        let report = self.next_report.fetch_add(1, Ordering::SeqCst) + 1;
        self.reports
            .lock()
            .unwrap()
            .insert(filename.to_string(), report);
        report
    }

    /// The number of the last change to `filename` the app was told of.
    fn last_report(&self, filename: &str) -> u64 {
        let reports = self.reports.lock().unwrap();
        reports.get(filename).copied().unwrap_or(0)
    }

    /// The file as it is on disk, if a save of `filename` may not be
    /// written over it: the app, which has handled the changes up to `seen`,
    /// has not heard of the last one, or another program changed the file
    /// since.
    fn in_the_way(&self, project: &Project, filename: &str, seen: u64) -> Option<(String, u64)> {
        let last = self.last_report(filename);
        if last > seen {
            return Some((project.read_file(filename).ok()?, last));
        }
        self.changed_on_disk(project, filename)
    }

    /// Another program changed `filename` since the app last read or wrote
    /// it. Unlike [`Core::changed_on_disk`], the stamp stays as it is.
    fn differs_on_disk(&self, project: &Project, filename: &str) -> bool {
        let Ok(path) = project.file_path(filename) else {
            return false;
        };
        let Some(held) = self.stamps.lock().unwrap().get(filename).copied() else {
            return false;
        };
        let stat = stat_of(&path);
        stat.0.is_some() && !held.looks_like(stat) && stamp_of(&path).hash != held.hash
    }

    /// The files the app has read that another program changed since.
    fn all_that_differ(&self, project: &Project) -> Vec<String> {
        let known: Vec<String> = self.stamps.lock().unwrap().keys().cloned().collect();
        known
            .into_iter()
            .filter(|f| self.differs_on_disk(project, f))
            .collect()
    }

    /// The app changed the frontmatter of `filename`, which another program
    /// had changed first: the file as it is now, for the tab that has it
    /// open. The app's change kept the other program's text, but the open
    /// editor has not seen it.
    fn rewritten_over_change(&self, project: &Project, filename: &str) -> Option<Event> {
        let id = project
            .files()
            .iter()
            .find(|f| f["filename"].as_str() == Some(filename))?["id"]
            .as_str()?
            .to_string();
        let content = project.read_file(filename).ok()?;
        let report = self.report(filename);
        Some(Event::FileChanged {
            id,
            content,
            report,
        })
    }

    /// Writes an edit as the project closes or the app quits, when there is
    /// no one to ask. If another program changed the file, it is left alone
    /// and the edit is written beside it as a copy, whose path is returned.
    /// The copy is named in the Log pane when the project is next opened.
    async fn write_closing(
        &self,
        project: &mut Project,
        closing: &ClosingSave,
    ) -> Result<Option<String>> {
        let (filename, content) = (&closing.save.filename, &closing.save.content);
        let path = project.file_path(filename)?;
        // A save in flight that landed needs nothing more.
        if closing.in_flight && std::fs::read_to_string(&path).is_ok_and(|d| d == *content) {
            return Ok(None);
        }
        let disk = if closing.conflict {
            // The change was already found, and the stamp is the file's.
            path.exists()
                .then(|| project.read_file(filename))
                .transpose()?
        } else {
            self.in_the_way(project, filename, closing.seen)
                .map(|(disk, _)| disk)
        };
        if disk.is_none_or(|disk| disk == *content) {
            project.write_file(filename, content).await?;
            self.stamp(project, filename);
            return Ok(None);
        }
        let when = chrono::Local::now().format("%Y-%m-%d %H%M").to_string();
        let copy = project.write_conflict_copy(filename, content, &when)?;
        self.window_state.add_notice(
            &project.dir()?.display().to_string(),
            format!(
                "“{filename}” was changed by another program before your edits were saved. \
                 Your edits are in “{copy}”."
            ),
        );
        Ok(Some(copy))
    }

    /// The app just read or wrote `filename`: what is on disk is what the
    /// app has.
    fn stamp(&self, project: &Project, filename: &str) {
        if let Ok(path) = project.file_path(filename) {
            self.stamps
                .lock()
                .unwrap()
                .insert(filename.to_string(), stamp_of(&path));
        }
    }

    /// The index and folders as they are now.
    fn tree(&self, project: &Project) -> Event {
        Event::Tree {
            index: project.index.clone().unwrap_or(Value::Null),
            folders: project.folders().to_vec(),
        }
    }
}

/// MCP ↔ app glue (`McpBridgeHost`): logs go to the Log panel, prefs are
/// read live, and write confirmations wait for the app's dialog.
struct NativeMcpHost {
    core: Arc<Mutex<Option<std::sync::Weak<Core>>>>,
}

impl NativeMcpHost {
    fn core(&self) -> Option<Arc<Core>> {
        self.core.lock().unwrap().as_ref()?.upgrade()
    }
}

impl McpHost for NativeMcpHost {
    fn log(&self, msg: String) {
        if let Some(core) = self.core() {
            core.send(Event::Log(msg));
        }
    }

    fn prefs(&self) -> (bool, Vec<String>) {
        match self.core() {
            Some(core) => {
                let p = core.paths.prefs();
                (p.mcp_confirm_writes, p.mcp_disabled_tools)
            }
            None => (false, Vec::new()),
        }
    }

    fn project_changed(&self) {
        if let Some(core) = self.core() {
            core.send(Event::McpChanged);
        }
    }

    fn confirm(&self, tool: &str, summary: &str, args: &Value) -> ConfirmFuture {
        let core = self.core();
        let (tool, summary, args) = (tool.to_string(), summary.to_string(), args.clone());
        Box::pin(async move {
            // No UI to ask → deny rather than run a write the user can't see.
            let Some(core) = core else {
                return "deny".to_string();
            };
            if core.ctx.lock().unwrap().is_none() {
                return "deny".to_string();
            }
            let id = core.next_confirm.fetch_add(1, Ordering::SeqCst) + 1;
            let (tx, rx) = tokio::sync::oneshot::channel();
            core.confirms.lock().unwrap().insert(id, tx);
            core.send(Event::McpConfirm(McpConfirm {
                id,
                tool,
                summary,
                args,
            }));
            rx.await.unwrap_or_else(|_| "deny".into())
        })
    }
}

pub struct Backend {
    rt: Runtime,
    rx: Receiver<Event>,
    core: Arc<Core>,
}

impl Backend {
    /// Starts the runtime with no project open. Call [`Backend::attach`]
    /// with the UI's context so events wake it.
    pub fn new(paths: Paths) -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("noteliner-backend")
            .build()
            .expect("tokio runtime");
        // Debouncers (window state) spawn on this runtime when scheduled from
        // the UI thread.
        laankit_core::debounce::set_runtime(rt.handle().clone());
        let (tx, rx) = channel();
        let ctx: Arc<Mutex<Option<egui::Context>>> = Arc::new(Mutex::new(None));

        let mut project = Project::new();
        project.write_frontmatter = paths.prefs().write_frontmatter;
        // Tests delete into a folder of their own, not the real trash.
        if cfg!(test) {
            project.trash = noteliner_core::project::folder_trash(paths.user_data.join("trash"));
        }
        let ws = Workspace::new(project);
        let host_core = Arc::new(Mutex::new(None));
        let mcp = McpServer::new(
            ws.clone(),
            Arc::new(NativeMcpHost {
                core: host_core.clone(),
            }),
            env!("CARGO_PKG_VERSION"),
        );
        let exporter = Arc::new(noteliner_export::LazyExporter::spawn(
            laankit_egui::fonts::bundled_faces()
                .iter()
                .map(|(_, bytes)| *bytes),
        ));
        let core = Arc::new(Core {
            exporter,
            window_state: WindowState::new(paths.window_state()),
            paths,
            ws,
            mcp,
            tx: Mutex::new(tx),
            ctx,
            confirms: Mutex::new(HashMap::new()),
            next_confirm: AtomicU64::new(0),
            stamps: Mutex::new(HashMap::new()),
            reports: Mutex::new(HashMap::new()),
            next_report: AtomicU64::new(0),
        });
        *host_core.lock().unwrap() = Some(Arc::downgrade(&core));
        Self { rt, rx, core }
    }

    /// The context to repaint when an event arrives. Also installs the math
    /// renderer (Typst, through the PDF exporter) for the preview and slides.
    pub fn attach(&self, ctx: &egui::Context) {
        let mut slot = self.core.ctx.lock().unwrap();
        if slot.is_none() {
            *slot = Some(ctx.clone());
            laankit_richtext::math::install(
                ctx,
                Arc::new(crate::math::TypstMath(self.core.exporter.clone())),
            );
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

    fn spawn<F>(&self, f: impl FnOnce(Arc<Core>) -> F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        self.rt.spawn(f(self.core.clone()));
    }

    // --- Projects ---------------------------------------------------------------

    /// Opens a folder. Any folder opens: its notes and sub-folders are read,
    /// and `noteliner.json` is made if it has none.
    pub fn open_project(&self, folder: PathBuf) {
        self.spawn(|core| async move {
            let res = {
                let mut ws = core.ws.lock().await;
                let res = ws.project.open(&folder).await;
                if res.is_ok() {
                    ws.rebuild_links();
                }
                res
            };
            core.stamps.lock().unwrap().clear();
            core.reports.lock().unwrap().clear();
            core.send(match res {
                Ok(opened) => Event::ProjectOpened {
                    folder,
                    index: opened.index,
                    folders: opened.folders,
                    truncated: opened.truncated,
                },
                Err(e) => Event::OpenFailed(e.to_string()),
            });
            core.maybe_start_mcp().await;
        });
    }

    /// Closes the project: writes the editors' pending edits, stops MCP. An
    /// edit to a file that another program changed is written as a copy.
    pub fn close_project(&self, pending: Vec<ClosingSave>) {
        self.spawn(|core| async move {
            core.reject_pending_confirms();
            core.mcp.stop().await;
            let mut ws = core.ws.lock().await;
            for closing in pending {
                if let Err(e) = core.write_closing(&mut ws.project, &closing).await {
                    log::warn!("saving {} before closing: {e}", closing.save.filename);
                }
            }
            ws.project.close();
            ws.links.reset();
            drop(ws);
            core.stamps.lock().unwrap().clear();
            core.reports.lock().unwrap().clear();
            core.send(Event::ProjectClosed);
        });
    }

    /// On quit: nothing to wait for but the MCP server.
    pub fn shutdown(&self) {
        self.core.reject_pending_confirms();
        self.core.window_state.save_now();
        let core = self.core.clone();
        self.rt.block_on(async move {
            core.mcp.stop().await;
        });
    }

    pub fn recent(&self) -> Vec<crate::config::Recent> {
        self.core.paths.recent()
    }

    /// `projects:addRecent`.
    pub fn add_recent(&self, folder: &Path) {
        if let Err(e) = self.core.paths.add_recent(&folder.display().to_string()) {
            log::warn!("couldn't save recent projects: {e}");
        }
    }

    /// `projects:removeRecent`.
    pub fn remove_recent(&self, folder: &str) {
        if let Err(e) = self.core.paths.remove_recent(folder) {
            self.core.send(Event::Error(format!(
                "Couldn't update recent projects: {e}"
            )));
        }
    }

    /// Saves the app's tag changes: each note's tags, the tag colours and
    /// the palette.
    pub fn save_index(&self, index: Value) {
        self.spawn(|core| async move {
            let mut ws = core.ws.lock().await;
            // The tags go into each file as it is on disk, so another
            // program's change to a note's text is kept. The tab that has
            // the note open is told of it.
            let changed = core.all_that_differ(&ws.project);
            let r = ws.project.save_index(index).await;
            let mut reloads = Vec::new();
            if let Ok(rewritten) = &r {
                for filename in rewritten {
                    core.stamp(&ws.project, filename);
                    if changed.contains(filename) {
                        reloads.extend(core.rewritten_over_change(&ws.project, filename));
                    }
                }
            }
            drop(ws);
            core.send(Event::IndexSaved(r.map(|_| ()).map_err(|e| e.to_string())));
            for event in reloads {
                core.send(event);
            }
        });
    }

    // --- Notes --------------------------------------------------------------------

    /// Reads a file as it is on disk: a note with its frontmatter.
    pub fn read_file(&self, id: String, filename: String) {
        self.spawn(|core| async move {
            let ws = core.ws.lock().await;
            match ws.project.read_file(&filename) {
                Ok(content) => {
                    core.stamp(&ws.project, &filename);
                    drop(ws);
                    core.send(Event::FileLoaded { id, content });
                }
                Err(e) => core.send(Event::Error(format!("Couldn't read {filename}: {e}"))),
            }
        });
    }

    /// Saves a whole file, over whatever is on disk. For a note whose file
    /// was removed by another program, `restore` is its index entry, which is
    /// put back with it.
    pub fn write_file(&self, id: String, filename: String, content: String) {
        self.write(id, filename, content, None, None);
    }

    /// Saves a whole file, unless another program changed it since the app
    /// last read or wrote it: then nothing is written, and
    /// [`Event::SaveConflict`] carries the file as it is on disk. `seen` is
    /// the last change to the file that the app handled (the `report` of an
    /// [`Event::FileChanged`] or an [`Event::SaveConflict`], or 0): the save
    /// is refused too if the backend has reported a later one.
    pub fn write_file_checked(&self, id: String, filename: String, content: String, seen: u64) {
        self.write(id, filename, content, None, Some(seen));
    }

    pub fn write_file_restoring(
        &self,
        id: String,
        filename: String,
        content: String,
        restore: Option<Value>,
    ) {
        self.write(id, filename, content, restore, None);
    }

    fn write(
        &self,
        id: String,
        filename: String,
        content: String,
        restore: Option<Value>,
        checked: Option<u64>,
    ) {
        self.spawn(move |core| async move {
            let mut ws = core.ws.lock().await;
            if let Some(seen) = checked {
                if let Some((disk, report)) = core.in_the_way(&ws.project, &filename, seen) {
                    drop(ws);
                    core.send(Event::SaveConflict {
                        id,
                        content,
                        disk,
                        report,
                    });
                    return;
                }
            }
            let restored = match restore {
                Some(entry) if ws.project.find(&id).is_none() => {
                    if let Ok(path) = ws.project.file_path(&filename) {
                        if let Some(dir) = path.parent() {
                            let _ = std::fs::create_dir_all(dir);
                        }
                    }
                    if let Some(files) = ws
                        .project
                        .index
                        .as_mut()
                        .and_then(|i| i["files"].as_array_mut())
                    {
                        files.push(entry);
                    }
                    true
                }
                _ => false,
            };
            let r = ws.project.write_file(&filename, &content).await;
            if r.is_ok() {
                core.stamp(&ws.project, &filename);
                ws.scan_links(&id);
            }
            let entry = ws.project.find(&id).cloned();
            let tree = restored.then(|| core.tree(&ws.project));
            drop(ws);
            if let Some(tree) = tree {
                core.send(tree);
            }
            core.send(Event::Saved {
                id,
                content,
                result: r.map_err(|e| e.to_string()),
                entry,
            });
        });
    }

    /// Saves an edit without waiting, on quit. An edit to a file that
    /// another program changed is written as a copy: its path is returned.
    pub fn write_file_now(&self, closing: ClosingSave) -> Result<Option<String>> {
        let core = self.core.clone();
        self.rt.block_on(async move {
            let mut ws = core.ws.lock().await;
            core.write_closing(&mut ws.project, &closing).await
        })
    }

    /// The lines kept for the Log pane as the project in `folder` last
    /// closed. They are given once.
    pub fn take_notices(&self, folder: &Path) -> Vec<String> {
        self.core
            .window_state
            .take_notices(&folder.display().to_string())
    }

    /// A new note (from a template) in `folder`; for a presentation, the
    /// blank deck body and the `presentation:` block, in the theme given. A
    /// template that is a presentation keeps its settings, and takes the
    /// theme only when one was picked.
    pub fn create_file(
        &self,
        name: String,
        tags: Vec<String>,
        template: Option<String>,
        folder: Option<String>,
        deck: Option<DeckTheme>,
    ) {
        self.spawn(|core| async move {
            let mut ws = core.ws.lock().await;
            let body = template
                .as_deref()
                .and_then(|t| templates::body_for(&ws.project, t, &name));
            let created = ws
                .project
                .create_file(&name, &json!(tags), body, folder.as_deref())
                .await;
            let mut entry = match created {
                Ok(entry) => entry,
                Err(e) => {
                    drop(ws);
                    core.send(Event::Error(format!("Couldn't create the file: {e}")));
                    return;
                }
            };
            if let Some(DeckTheme(theme)) = deck {
                let filename = entry["filename"].as_str().unwrap_or_default().to_string();
                if template.is_none() {
                    if let Err(e) = ws
                        .project
                        .write_body(&filename, &blank_deck_body(&name))
                        .await
                    {
                        log::warn!("blank deck body: {e}");
                    }
                }
                let presentation = if !entry["deck"].as_bool().unwrap_or(false) {
                    let theme = theme.as_deref();
                    Some(presentation_in(
                        theme.unwrap_or(noteliner_slides::SlideTheme::DEFAULT_ID),
                    ))
                } else {
                    // The template's own settings, in the theme picked.
                    theme.map(|theme| {
                        let mut p = ws.project.read_frontmatter(&filename)["presentation"].clone();
                        if !p.is_object() {
                            p = default_presentation();
                        }
                        p["theme"] = json!(theme);
                        p
                    })
                };
                if let Some(presentation) = presentation {
                    match ws.project.set_presentation(&filename, &presentation).await {
                        Ok(_) => entry["deck"] = json!(true),
                        Err(e) => log::warn!("seeding the presentation: {e}"),
                    }
                }
            }
            ws.rebuild_links();
            drop(ws);
            core.send(Event::Created(entry));
        });
    }

    /// A new, blank diagram (`.mmd`).
    pub fn create_diagram(&self, name: String, tags: Vec<String>, folder: Option<String>) {
        self.spawn(|core| async move {
            let mut ws = core.ws.lock().await;
            let r = ws
                .project
                .create(
                    FileKind::Diagram,
                    &name,
                    &json!(tags),
                    None,
                    folder.as_deref(),
                )
                .await;
            drop(ws);
            match r {
                Ok(entry) => core.send(Event::Created(entry)),
                Err(e) => core.send(Event::Error(format!("Couldn't create the diagram: {e}"))),
            }
        });
    }

    /// A copy of `source` in `folder`. A note's copy keeps its frontmatter's
    /// other fields, such as a deck's settings.
    pub fn duplicate_file(
        &self,
        source: String,
        name: String,
        tags: Vec<String>,
        folder: Option<String>,
    ) {
        self.spawn(|core| async move {
            let mut ws = core.ws.lock().await;
            let filename = ws
                .project
                .find(&source)
                .and_then(|f| f["filename"].as_str().map(str::to_string));
            // The copy is the same kind of file as its source.
            let kind = filename.as_deref().map(FileKind::of).unwrap_or_default();
            let body = filename.and_then(|f| ws.project.read_file(&f).ok());
            let r = ws
                .project
                .create(kind, &name, &json!(tags), body, folder.as_deref())
                .await;
            if r.is_ok() {
                ws.rebuild_links();
            }
            drop(ws);
            match r {
                Ok(entry) => core.send(Event::Created(entry)),
                Err(e) => core.send(Event::Error(format!("Couldn't duplicate the file: {e}"))),
            }
        });
    }

    /// Markdown files dropped on the window: each becomes a note in
    /// `folder`, with the file's text and the tags in its frontmatter. A
    /// `.mmd` or `.mermaid` file becomes a diagram, if it's one BlockLiner
    /// can read.
    pub fn import_markdown(&self, files: Vec<PathBuf>, folder: Option<String>) {
        self.spawn(|core| async move {
            for path in files {
                let Ok(content) = std::fs::read_to_string(&path) else {
                    continue;
                };
                let name = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let kind = FileKind::of(&path.to_string_lossy());
                let tags = match kind {
                    FileKind::Diagram => Vec::new(),
                    FileKind::Note => {
                        frontmatter::tags_of(&frontmatter::parse(&content).data).unwrap_or_default()
                    }
                };
                let mut ws = core.ws.lock().await;
                let r = ws
                    .project
                    .create(kind, &name, &json!(tags), Some(content), folder.as_deref())
                    .await;
                if r.is_ok() {
                    ws.rebuild_links();
                }
                drop(ws);
                match r {
                    Ok(entry) => core.send(Event::Created(entry)),
                    Err(e) => core.send(Event::Error(format!("Couldn't add “{name}”: {e}"))),
                }
            }
        });
    }

    /// Renames a note or diagram, and its file within its folder.
    pub fn rename_file(&self, id: String, name: String) {
        self.spawn(|core| async move {
            let mut ws = core.ws.lock().await;
            let old = ws
                .project
                .find(&id)
                .and_then(|f| f["filename"].as_str().map(str::to_string));
            let r = ws.project.rename_file(&id, &name).await;
            if let Ok(Some(entry)) = &r {
                ws.rebuild_links();
                // The app's stamp follows the file to its new name.
                if let (Some(old), Some(new)) = (old, entry["filename"].as_str()) {
                    core.stamps.lock().unwrap().remove(&old);
                    core.stamp(&ws.project, new);
                }
            }
            drop(ws);
            match r {
                Ok(entry) => core.send(Event::Renamed { id, entry }),
                Err(e) => core.send(Event::Error(format!("Couldn't rename the file: {e}"))),
            }
        });
    }

    /// Deletes a note or diagram: to the trash, or for good with
    /// `permanent` (after the app asked, when the trash isn't available).
    pub fn delete_file(&self, id: String, permanent: bool) {
        self.spawn(|core| async move {
            let mut ws = core.ws.lock().await;
            let r = ws.project.delete_file(&id, permanent).await;
            if r.is_ok() {
                ws.links.remove_file(&id);
                ws.rebuild_links();
            }
            drop(ws);
            match r {
                Ok(()) => core.send(Event::Deleted(id)),
                Err(e) => delete_failed(&core, DeleteTarget::File(id), e),
            }
        });
    }

    /// Moves a note or diagram to `folder`, with a note's attachments.
    pub fn move_file(&self, id: String, folder: String) {
        self.spawn(|core| async move {
            let mut ws = core.ws.lock().await;
            let old = ws
                .project
                .find(&id)
                .and_then(|f| f["filename"].as_str().map(str::to_string));
            let r = ws.project.move_file(&id, &folder).await;
            if let (Ok(Some(entry)), Some(old)) = (&r, old) {
                core.stamps.lock().unwrap().remove(&old);
                core.stamp(&ws.project, entry["filename"].as_str().unwrap_or_default());
            }
            let tree = core.tree(&ws.project);
            drop(ws);
            match r {
                Ok(_) => core.send(tree),
                Err(e) => core.send(Event::Error(format!("Couldn't move the file: {e}"))),
            }
        });
    }

    // --- Folders ------------------------------------------------------------------

    /// Makes a folder at `path`.
    pub fn create_folder(&self, path: String) {
        self.spawn(|core| async move {
            let mut ws = core.ws.lock().await;
            let r = ws.project.create_folder(&path).await;
            let tree = core.tree(&ws.project);
            drop(ws);
            match r {
                Ok(made) => {
                    core.send(tree);
                    core.send(Event::FolderCreated(made));
                }
                Err(e) => core.send(Event::Error(format!("Couldn't create the folder: {e}"))),
            }
        });
    }

    /// Renames the folder at `path`. Every note under it gets its new path.
    pub fn rename_folder(&self, path: String, name: String) {
        self.spawn(|core| async move {
            let mut ws = core.ws.lock().await;
            let r = ws.project.rename_folder(&path, &name).await;
            if r.is_ok() {
                core.forget_stamps_under(&path);
            }
            let tree = core.tree(&ws.project);
            drop(ws);
            match r {
                // The app moves its open folders and selection first, then
                // takes the new tree.
                Ok(to) => {
                    core.send(Event::FolderMoved { from: path, to });
                    core.send(tree);
                }
                Err(e) => core.send(Event::Error(format!("Couldn't rename the folder: {e}"))),
            }
        });
    }

    /// Moves the folder at `path` into `into` (empty for the top).
    pub fn move_folder(&self, path: String, into: String) {
        self.spawn(|core| async move {
            let mut ws = core.ws.lock().await;
            let r = ws.project.move_folder(&path, &into).await;
            if r.is_ok() {
                core.forget_stamps_under(&path);
            }
            let tree = core.tree(&ws.project);
            drop(ws);
            match r {
                Ok(to) => {
                    core.send(Event::FolderMoved { from: path, to });
                    core.send(tree);
                }
                Err(e) => core.send(Event::Error(format!("Couldn't move the folder: {e}"))),
            }
        });
    }

    /// Deletes a folder and everything in it: to the trash, or for good
    /// with `permanent`.
    pub fn delete_folder(&self, path: String, permanent: bool) {
        self.spawn(|core| async move {
            let mut ws = core.ws.lock().await;
            let r = ws.project.delete_folder(&path, permanent).await;
            if r.is_ok() {
                ws.rebuild_links();
                core.forget_stamps_under(&path);
            }
            let tree = core.tree(&ws.project);
            drop(ws);
            match r {
                Ok(_) => core.send(tree),
                Err(e) => delete_failed(&core, DeleteTarget::Folder(path), e),
            }
        });
    }

    // --- Changes made outside NoteLiner ------------------------------------------

    /// Looks for changes made by other programs: notes and folders added,
    /// moved or removed, and the files in `open` (the open tabs, as id and
    /// path) changed on disk. A quiet project costs one `stat` per folder
    /// and per open file. With `tree`, the index is sent back even if
    /// nothing on disk changed (an AI assistant wrote to the project).
    pub fn check(&self, open: Vec<(String, String)>, tree: bool) {
        self.spawn(|core| async move { core.check(open, tree).await });
    }

    /// As [`Backend::check`], finished before it returns. What it found
    /// waits for the next [`Backend::poll`].
    #[cfg(test)]
    pub fn check_now(&self, open: Vec<(String, String)>) {
        let core = self.core.clone();
        self.rt
            .block_on(async move { core.check(open, false).await });
    }
}

impl Core {
    async fn check(&self, open: Vec<(String, String)>, tree: bool) {
        let core = self;
        {
            let mut ws = core.ws.lock().await;
            if !ws.project.is_open() {
                return;
            }
            let changed = match ws.project.rescan() {
                Ok(c) => c,
                Err(e) => {
                    log::warn!("checking the project folder: {e}");
                    None
                }
            };
            if let Some(changes) = &changed {
                ws.rebuild_links();
                // A moved file keeps the stamp it had.
                let mut stamps = core.stamps.lock().unwrap();
                for (_, old, new) in &changes.moved {
                    if let Some(stamp) = stamps.remove(old) {
                        stamps.insert(new.clone(), stamp);
                    }
                    let mut reports = core.reports.lock().unwrap();
                    if let Some(report) = reports.remove(old) {
                        reports.insert(new.clone(), report);
                    }
                }
            }
            if changed.is_some() || tree {
                core.send(core.tree(&ws.project));
            }
            // The path each open file has now.
            let mut reloads = Vec::new();
            for (id, _) in open {
                let Some(filename) = ws
                    .project
                    .find(&id)
                    .and_then(|f| f["filename"].as_str().map(str::to_string))
                else {
                    continue;
                };
                let known = core.stamps.lock().unwrap().contains_key(&filename);
                if !known {
                    core.stamp(&ws.project, &filename);
                } else if let Some((content, report)) = core.changed_on_disk(&ws.project, &filename)
                {
                    reloads.push(Event::FileChanged {
                        id,
                        content,
                        report,
                    });
                }
            }
            drop(ws);
            for event in reloads {
                core.send(event);
            }
        }
    }
}

impl Backend {
    /// Adds, replaces or (with null) removes a note's `presentation:` block.
    pub fn set_presentation(&self, id: String, filename: String, presentation: Value) {
        self.spawn(|core| async move {
            let mut ws = core.ws.lock().await;
            // As for the tags in [`Backend::save_index`].
            let changed = core.differs_on_disk(&ws.project, &filename);
            let r = ws.project.set_presentation(&filename, &presentation).await;
            let mut reload = None;
            if r.is_ok() {
                core.stamp(&ws.project, &filename);
                if changed {
                    reload = core.rewritten_over_change(&ws.project, &filename);
                }
            }
            drop(ws);
            core.send(Event::Presentation {
                id,
                outcome: r.map_err(|e| e.to_string()),
            });
            if let Some(event) = reload {
                core.send(event);
            }
        });
    }

    // --- Templates ------------------------------------------------------------

    /// `templates:list`.
    pub fn list_templates(&self) {
        self.spawn(|core| async move {
            let list = templates::list(&core.ws.lock().await.project);
            core.send(Event::Templates(list));
        });
    }

    /// Save as Template, reported in the Activity Log.
    pub fn save_template(&self, name: String, body: String) {
        self.spawn(|core| async move {
            let r = {
                let ws = core.ws.lock().await;
                templates::save(&ws.project, &name, &body).await
            };
            match r {
                Ok(v) => core.send(Event::Log(format!(
                    "Saved template \"{}\"",
                    v["name"].as_str().unwrap_or(&name)
                ))),
                Err(e) => core.send(Event::Log(format!("Save as template failed: {e}"))),
            }
        });
    }

    // --- Import / export --------------------------------------------------------

    /// Imports a Word or PowerPoint document as a note in `folder`.
    pub fn import(&self, path: PathBuf, folder: Option<String>) {
        self.spawn(|core| async move {
            let r = {
                let mut ws = core.ws.lock().await;
                let r = import::import(&mut ws.project, &path, folder.as_deref()).await;
                if r.is_ok() {
                    ws.rebuild_links();
                }
                r
            };
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            core.send(Event::Imported {
                name,
                result: r.map_err(|e| e.to_string()),
            });
        });
    }

    /// Writes the note to the Downloads folder, or to `to` (the tests).
    ///
    /// A presentation saved as PowerPoint goes through `crate::pptx`, which
    /// reads its pictures and renders its formulas first.
    ///
    /// PDF goes through `noteliner-export` (Typst, in-process): Letter, 0.5 in
    /// margins, and the note's name as the title heading. The layout runs on
    /// a blocking thread. Images come from `_attachments/` beside the note.
    pub fn export(&self, kind: ExportKind, filename: String, name: String, to: Option<PathBuf>) {
        self.spawn(|core| async move {
            let result = async {
                let (raw, note_dir, presentation) = {
                    let ws = core.ws.lock().await;
                    if !ws.project.is_open() {
                        anyhow::bail!("No project is open");
                    }
                    let raw = std::fs::read_to_string(ws.project.file_path(&filename)?)?;
                    // A note's attachments are in its own folder.
                    let note_dir = ws.project.dir()?.join(tree::folder_of(&filename));
                    let presentation = match kind {
                        ExportKind::Pptx => {
                            ws.project.read_frontmatter(&filename)["presentation"].clone()
                        }
                        _ => Value::Null,
                    };
                    (raw, note_dir, presentation)
                };
                let dir = to.unwrap_or_else(downloads_dir);
                std::fs::create_dir_all(&dir)?;
                let slug = export::export_slug(&name);
                let mut warnings = Vec::new();
                match kind {
                    ExportKind::Markdown => std::fs::write(dir.join(format!("{slug}.md")), &raw)?,
                    ExportKind::Mermaid => std::fs::write(dir.join(format!("{slug}.mmd")), &raw)?,
                    ExportKind::Html => std::fs::write(
                        dir.join(format!("{slug}.html")),
                        export::html_document(&name, &frontmatter::strip_body(&raw)),
                    )?,
                    ExportKind::Pdf => {
                        let pdf = pdf_bytes(core.exporter.clone(), raw, name, note_dir).await?;
                        std::fs::write(dir.join(format!("{slug}.pdf")), pdf)?
                    }
                    ExportKind::Pptx => {
                        let out = crate::pptx::export(
                            core.exporter.clone(),
                            raw,
                            presentation,
                            name,
                            note_dir,
                        )
                        .await?;
                        std::fs::write(dir.join(format!("{slug}.pptx")), out.bytes)?;
                        warnings = out.warnings;
                    }
                }
                Ok((dir, warnings))
            }
            .await;
            let (result, warnings) = match result {
                Ok((dir, warnings)) => (Ok(dir), warnings),
                Err(e) => (Err(e), Vec::new()),
            };
            core.send(Event::Exported {
                kind,
                result: result.map_err(|e: anyhow::Error| e.to_string()),
                warnings,
            });
        });
    }

    // --- Prefs, MCP, updates ----------------------------------------------------

    /// `ui:setPrefs` plus NoteLiner's side effects (`on_prefs_change`):
    /// frontmatter writing and starting or stopping the MCP server.
    pub fn set_prefs(&self, patch: Value) {
        if let Err(e) = self.core.paths.set_prefs(&patch) {
            self.core
                .send(Event::Error(format!("Failed to save preference: {e}")));
            return;
        }
        let write_frontmatter = patch.get("writeFrontmatter").and_then(Value::as_bool);
        let mcp_enabled = patch.get("mcpEnabled").and_then(Value::as_bool);
        if write_frontmatter.is_none() && mcp_enabled.is_none() {
            return;
        }
        self.spawn(|core| async move {
            if let Some(on) = write_frontmatter {
                core.ws.lock().await.project.write_frontmatter = on;
            }
            match mcp_enabled {
                Some(true) => core.maybe_start_mcp().await,
                Some(false) => core.mcp.stop().await,
                None => {}
            }
            if mcp_enabled.is_some() {
                let status = core.mcp_status().await;
                core.send(Event::Mcp(status));
            }
        });
    }

    /// `mcp:getStatus`.
    pub fn mcp_status(&self) {
        self.spawn(|core| async move {
            let status = core.mcp_status().await;
            core.send(Event::Mcp(status));
        });
    }

    /// `mcp:confirm-response`: "allow", "session" or "deny".
    pub fn respond_confirm(&self, id: u64, decision: &str) {
        let decision = match decision {
            d @ ("allow" | "session" | "deny") => d.to_string(),
            _ => "deny".into(),
        };
        if let Some(tx) = self.core.confirms.lock().unwrap().remove(&id) {
            let _ = tx.send(decision);
        }
    }

    /// The About dialog's update check (`update:checkNow`).
    pub fn check_updates(&self) {
        self.spawn(|core| async move {
            let status = if cfg!(debug_assertions) {
                UpdateStatus::Unavailable { dev: true }
            } else {
                match updates::check(
                    UPDATE_REPO,
                    updates::NATIVE_TAG_PREFIX,
                    env!("CARGO_PKG_VERSION"),
                )
                .await
                {
                    Ok(UpdateCheck::Available(rel)) => UpdateStatus::Available {
                        version: rel.version,
                        notes: rel.notes,
                        url: rel.url,
                    },
                    Ok(UpdateCheck::UpToDate { .. }) => UpdateStatus::Unavailable { dev: false },
                    Err(e) => UpdateStatus::Error(e.to_string()),
                }
            };
            core.send(Event::Update(status));
        });
    }

    // --- Window state (window-state:*) -----------------------------------------

    /// `window-state:getLayout`.
    pub fn layout(&self, folder: &Path) -> Value {
        self.core.window_state.layout(&folder.display().to_string())
    }

    /// `window-state:saveLayout` (debounced by `WindowState`).
    pub fn save_layout(&self, folder: &Path, layout: Value) {
        self.core
            .window_state
            .set_layout(&folder.display().to_string(), layout);
    }

    /// The saved window bounds for a project: `{ x, y, width, height }` and
    /// whether it was maximized.
    pub fn bounds(&self, folder: &Path) -> Option<(Value, bool)> {
        self.core.window_state.bounds(&folder.display().to_string())
    }

    pub fn save_bounds(&self, folder: &Path, bounds: Value, maximized: bool, immediate: bool) {
        self.core.window_state.set_bounds(
            &folder.display().to_string(),
            bounds,
            maximized,
            immediate,
        );
    }
}

/// The channels behind the Search, Backlinks and Attachments panes.
impl Backend {
    /// `links:getBacklinks`.
    pub fn backlinks(&self, id: String) {
        self.spawn(|core| async move {
            let links = {
                let ws = core.ws.lock().await;
                ws.links.backlink_snippets(&ws.project, &id)
            };
            core.send(Event::Backlinks { id, links });
        });
    }

    /// `links:getAllNames`: for wikilink autocomplete, with laankit-editor.
    #[allow(dead_code)]
    pub fn note_names(&self) {
        self.spawn(|core| async move {
            let names = LinkGraph::all_note_names(&core.ws.lock().await.project);
            core.send(Event::NoteNames(names));
        });
    }

    /// `search:query`.
    pub fn search(&self, query: String, case_sensitive: bool) {
        self.spawn(|core| async move {
            let results = core.ws.lock().await.project.search(&query, case_sensitive);
            core.send(Event::SearchResults { query, results });
        });
    }

    /// Reads the Git repository at `folder`, for Files Info. Nothing in it
    /// is changed.
    pub fn git_info(&self, folder: PathBuf) {
        self.spawn(|core| async move {
            let result = noteliner_core::git::read(&folder).await;
            core.send(Event::GitInfo { folder, result });
        });
    }

    /// `file:addAttachment`.
    pub fn add_attachment(&self, file_id: String, data: Vec<u8>, name: String, insert: bool) {
        self.spawn(|core| async move {
            let r = core
                .ws
                .lock()
                .await
                .project
                .add_attachment(&file_id, &data, &name)
                .await;
            core.send(Event::AttachmentAdded {
                file_id,
                result: r.map_err(|e| e.to_string()),
                insert,
            });
        });
    }

    /// `file:removeAttachment`.
    pub fn remove_attachment(&self, file_id: String, attachment_id: String) {
        self.spawn(|core| async move {
            let r = core
                .ws
                .lock()
                .await
                .project
                .remove_attachment(&file_id, &attachment_id)
                .await;
            match r {
                Ok(()) => core.send(Event::AttachmentRemoved {
                    file_id,
                    attachment_id,
                }),
                Err(e) => core.send(Event::Error(format!("Couldn't remove the attachment: {e}"))),
            }
        });
    }
}

/// A delete that failed: when the trash couldn't take it, the app asks
/// before deleting for good. Anything else is a toast.
fn delete_failed(core: &Core, target: DeleteTarget, e: anyhow::Error) {
    match e.downcast_ref::<TrashUnavailable>() {
        Some(t) => core.send(Event::DeleteFailed {
            target,
            message: t.0.clone(),
        }),
        None => core.send(Event::Error(format!("Couldn't delete: {e}"))),
    }
}

impl Core {
    /// The files under a folder that was renamed, moved or deleted have new
    /// paths, or none: their stamps are read again.
    fn forget_stamps_under(&self, folder: &str) {
        self.stamps
            .lock()
            .unwrap()
            .retain(|path, _| !tree::is_under(path, folder));
        self.reports
            .lock()
            .unwrap()
            .retain(|path, _| !tree::is_under(path, folder));
    }
}

/// Lays out `raw` (a note with its frontmatter) as a PDF on a blocking thread.
pub async fn pdf_bytes(
    exporter: Arc<noteliner_export::LazyExporter>,
    raw: String,
    name: String,
    note_dir: PathBuf,
) -> Result<Vec<u8>> {
    let options = pdf_options(name, note_dir);
    tokio::task::spawn_blocking(move || exporter.get().export(&raw, &options))
        .await?
        .map_err(|e| anyhow::anyhow!("{e}"))
}

/// What the export prints: Letter, half-inch margins, the note's name as the
/// heading, images from `_attachments/` in `note_dir` (the note's folder).
/// Wikilinks render as styled text, since a standalone PDF has no other
/// notes to jump to.
pub fn pdf_options(name: String, note_dir: PathBuf) -> noteliner_export::ExportOptions {
    noteliner_export::ExportOptions {
        title: noteliner_export::Title::Text(name),
        base_dir: Some(note_dir),
        ..Default::default()
    }
}

/// Electron's `app.getPath('downloads')`.
pub fn downloads_dir() -> PathBuf {
    let home = laankit_core::paths::home_dir();
    if cfg!(all(unix, not(target_os = "macos"))) {
        if let Ok(out) = std::process::Command::new("xdg-user-dir")
            .arg("DOWNLOAD")
            .output()
        {
            let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if out.status.success() && !p.is_empty() && Path::new(&p) != home {
                return PathBuf::from(p);
            }
        }
    }
    home.join("Downloads")
}
