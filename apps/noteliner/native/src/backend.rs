//! Everything that touches disk, git or the network. Calls run on a tokio
//! runtime and report back as [`Event`]s, which the UI drains once per frame
//! ([`Backend::poll`]); nothing here blocks the UI thread.
//!
//! The logic is the Tauri host's (`apps/noteliner/src-tauri/src/main.rs`),
//! channel by channel, over the same `noteliner-core` workspace, so projects
//! and `~/.config/NoteLiner` stay compatible with the Electron and Tauri
//! builds. The workspace sits behind one async mutex, shared with the MCP
//! server, so operations never interleave mid-write.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use marina_core::git::{LogEntry, SyncStatus};
use marina_core::updates::{self, UpdateCheck};
use marina_egui::about::UpdateStatus;
use noteliner_core::git::{GitConfigRequired, NoteGit};
use noteliner_core::links::LinkGraph;
use noteliner_core::mcp::{self, ConfirmFuture, McpHost, McpServer};
use noteliner_core::project::Project;
use noteliner_core::window_state::WindowState;
use noteliner_core::workspace::{Shared, Workspace};
use noteliner_core::{export, frontmatter, import, templates};
use serde_json::{json, Value};
use tokio::runtime::Runtime;

use crate::config::Paths;

/// GitHub `owner/repo` publishing NoteLiner releases (`update_repo`).
const UPDATE_REPO: &str = "NathanLaan/noteliner";

/// `DEFAULT_PRESENTATION` in `lib/slides.js`: what "Presentation" seeds.
pub fn default_presentation() -> Value {
    json!({
        "theme": "dark",
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

/// How a write ended (`{ error: 'git_config_required' }` is its own case:
/// the app opens Project Settings for it).
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome<T> {
    Ok(T),
    GitConfigRequired,
    Err(String),
}

impl<T> Outcome<T> {
    fn of(r: Result<T>) -> Self {
        match r {
            Ok(v) => Outcome::Ok(v),
            Err(e) if e.downcast_ref::<GitConfigRequired>().is_some() => Outcome::GitConfigRequired,
            Err(e) => Outcome::Err(e.to_string()),
        }
    }
}

/// A git operation the Sync dialog waits on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Connect,
    Disconnect,
    Push,
    Pull,
    PullRebase,
    Reset,
}

/// A Convert/Save-to target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportKind {
    Html,
    Markdown,
    Pdf,
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
    /// A line for the Log panel (git output, MCP, import notes).
    Log(String),
    /// `project:open` / `project:init` loaded a project.
    ProjectOpened {
        folder: PathBuf,
        index: Value,
        needs_git_config: bool,
    },
    /// The folder isn't a NoteLiner project yet (`needs_setup`).
    NeedsSetup(PathBuf),
    OpenFailed(String),
    /// `project:close` finished (pending pushes flushed).
    ProjectClosed,
    /// `file:read` plus `file:getFrontmatter` for the selected note.
    FileLoaded {
        id: String,
        content: String,
        frontmatter: Value,
    },
    /// `file:write` finished for this note.
    Saved {
        id: String,
        content: String,
        outcome: Outcome<()>,
        /// The note's index entry after the write (`modifiedAt`, `deck`).
        entry: Option<Value>,
    },
    /// `file:create` / `file:duplicate` / an import added this entry.
    Created(Value),
    Renamed {
        id: String,
        entry: Option<Value>,
    },
    Deleted(String),
    /// `project:saveIndex` finished.
    IndexSaved(Outcome<()>),
    /// `file:setPresentation` finished: the new `presentation` (or null).
    Presentation {
        id: String,
        outcome: Outcome<Value>,
    },
    Templates(Vec<Value>),
    GitConfig {
        name: String,
        email: String,
    },
    /// Branch, remote and sync status (status bar and Sync dialog).
    GitState {
        branch: Option<String>,
        remote: Option<String>,
        sync: Option<SyncStatus>,
    },
    GitDone {
        op: Op,
        result: Result<(), String>,
    },
    /// `git:resetToRemote` reloaded the index.
    IndexReloaded(Value),
    Mcp(McpStatus),
    McpConfirm(McpConfirm),
    Update(UpdateStatus),
    /// A Save-to finished: the Downloads folder to reveal, or why not.
    Exported {
        kind: ExportKind,
        result: Result<PathBuf, String>,
    },
    /// `file:import` of the file `name` finished: `{ entry, stats }`.
    Imported {
        name: String,
        result: Result<Value, String>,
    },
    // --- The Search, Backlinks, History and Attachments panes ---
    SearchResults {
        query: String,
        results: Vec<Value>,
    },
    Backlinks {
        id: String,
        links: Vec<Value>,
    },
    /// For wikilink autocomplete, with marina-editor.
    #[allow(dead_code)]
    NoteNames(Vec<Value>),
    History {
        filename: String,
        entries: Vec<LogEntry>,
    },
    HistoryContent {
        hash: String,
        content: Option<String>,
    },
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
    /// A commit needs a git identity: open Project Settings (`required`).
    GitConfigRequired,
    /// Something failed in the background; show it as an error toast.
    Error(String),
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

    async fn git_state(&self) -> Event {
        let ws = self.ws.lock().await;
        let Ok(dir) = ws.project.dir().map(Path::to_path_buf) else {
            return Event::GitState {
                branch: None,
                remote: None,
                sync: None,
            };
        };
        let git = &ws.project.git;
        let branch = git.git.current_branch(&dir).await;
        let remote = git.git.get_remote_url(&dir).await;
        // `git:getSyncStatus` fetches, so it only runs when there's a remote.
        let sync = match &remote {
            Some(_) => Some(git.sync_status(&dir).await),
            None => None,
        };
        Event::GitState {
            branch,
            remote,
            sync,
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
        // Debouncers (pushes, window state) spawn on this runtime when
        // scheduled from the UI thread.
        marina_core::debounce::set_runtime(rt.handle().clone());
        let (tx, rx) = channel();
        let ctx: Arc<Mutex<Option<egui::Context>>> = Arc::new(Mutex::new(None));

        // Git output streams to the Log panel.
        let log_tx = Mutex::new(tx.clone());
        let log_ctx = ctx.clone();
        let git = NoteGit::new(Arc::new(move |msg| {
            let _ = log_tx.lock().unwrap().send(Event::Log(msg));
            if let Some(ctx) = log_ctx.lock().unwrap().as_ref() {
                ctx.request_repaint();
            }
        }));
        let mut project = Project::new(git);
        project.write_frontmatter = paths.prefs().write_frontmatter;
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
            marina_egui::fonts::bundled_faces()
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
        });
        *host_core.lock().unwrap() = Some(Arc::downgrade(&core));
        Self { rt, rx, core }
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

    fn spawn<F>(&self, f: impl FnOnce(Arc<Core>) -> F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        self.rt.spawn(f(self.core.clone()));
    }

    // --- Projects (project:*, projects:*) --------------------------------------

    /// `project:open`.
    pub fn open_project(&self, folder: PathBuf) {
        self.spawn(|core| async move {
            let res = {
                let mut ws = core.ws.lock().await;
                let res = ws.project.open(&folder).await;
                if res.as_ref().is_ok_and(|r| r["status"] == "loaded") {
                    ws.rebuild_links();
                }
                res
            };
            core.send(opened(&folder, res));
            core.maybe_start_mcp().await;
        });
    }

    /// `project:init`: a new repository, or a clone of `remote`.
    pub fn init_project(&self, folder: PathBuf, remote: Option<String>) {
        self.spawn(|core| async move {
            let res = {
                let mut ws = core.ws.lock().await;
                let res = ws.project.init(&folder, remote.as_deref()).await;
                if res.is_ok() {
                    ws.rebuild_links();
                }
                res
            };
            core.send(opened(&folder, res));
            core.maybe_start_mcp().await;
        });
    }

    /// `handleNewProjectConfirm`: `fs:ensureDir`, `project:init` and
    /// `git:setConfig` in one go.
    pub fn new_project(&self, folder: PathBuf, name: String, email: String) {
        self.spawn(|core| async move {
            if let Err(e) = std::fs::create_dir_all(&folder) {
                core.send(Event::OpenFailed(format!(
                    "Failed to create directory: {e}"
                )));
                return;
            }
            let res = {
                let mut ws = core.ws.lock().await;
                let res = ws.project.init(&folder, None).await;
                if res.is_ok() {
                    if let Err(e) = ws.project.set_git_config(&name, &email).await {
                        core.send(Event::Error(format!("Couldn't set the git identity: {e}")));
                    }
                    ws.rebuild_links();
                }
                res
            };
            // The identity was just set, so there's nothing to ask for.
            let event = match opened(&folder, res) {
                Event::ProjectOpened { folder, index, .. } => Event::ProjectOpened {
                    folder,
                    index,
                    needs_git_config: false,
                },
                other => other,
            };
            core.send(event);
            core.maybe_start_mcp().await;
        });
    }

    /// `project:close`: write the editor's pending edit, stop MCP, flush the
    /// pending push, close.
    pub fn close_project(&self, pending: Option<crate::editor::SaveRequest>) {
        self.spawn(|core| async move {
            core.reject_pending_confirms();
            core.mcp.stop().await;
            let mut ws = core.ws.lock().await;
            if let Some(save) = pending {
                if let Err(e) = ws.project.write_file(&save.filename, &save.content).await {
                    log::warn!("saving {} before closing: {e}", save.filename);
                }
            }
            if let Ok(dir) = ws.project.dir().map(Path::to_path_buf) {
                ws.project.git.flush_push(&dir).await;
            }
            ws.project.close();
            ws.links.reset();
            drop(ws);
            core.send(Event::ProjectClosed);
        });
    }

    /// `project:close` on quit: blocks until the push is flushed.
    pub fn shutdown(&self) {
        self.core.reject_pending_confirms();
        self.core.window_state.save_now();
        let core = self.core.clone();
        self.rt.block_on(async move {
            core.mcp.stop().await;
            let ws = core.ws.lock().await;
            if let Ok(dir) = ws.project.dir() {
                ws.project.git.flush_push(dir).await;
            }
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

    /// `project:saveIndex`.
    pub fn save_index(&self, index: Value) {
        self.spawn(|core| async move {
            let r = core.ws.lock().await.project.save_index(index).await;
            core.send(Event::IndexSaved(Outcome::of(r)));
        });
    }

    // --- Notes (file:*) ---------------------------------------------------------

    /// `file:read` + `file:getFrontmatter`.
    pub fn read_file(&self, id: String, filename: String) {
        self.spawn(|core| async move {
            let ws = core.ws.lock().await;
            match ws.project.read_file(&filename) {
                Ok(content) => {
                    let frontmatter = ws.project.read_frontmatter(&filename);
                    drop(ws);
                    core.send(Event::FileLoaded {
                        id,
                        content,
                        frontmatter,
                    });
                }
                Err(e) => core.send(Event::Error(format!("Couldn't read {filename}: {e}"))),
            }
        });
    }

    /// `file:write`: the body (frontmatter is mirrored from the index), then
    /// a commit and a debounced push.
    pub fn write_file(&self, id: String, filename: String, content: String) {
        self.spawn(|core| async move {
            let mut ws = core.ws.lock().await;
            let r = ws.project.write_file(&filename, &content).await;
            if r.is_ok() {
                ws.scan_links(&id);
            }
            let entry = ws.project.find(&id).cloned();
            drop(ws);
            core.send(Event::Saved {
                id,
                content,
                outcome: Outcome::of(r),
                entry,
            });
        });
    }

    /// `file:write` without waiting, on quit.
    pub fn write_file_now(&self, filename: &str, content: &str) -> Result<()> {
        let core = self.core.clone();
        let (filename, content) = (filename.to_string(), content.to_string());
        self.rt.block_on(async move {
            core.ws
                .lock()
                .await
                .project
                .write_file(&filename, &content)
                .await
        })
    }

    /// `handleNewFileConfirm`: `file:create` (from a template, under a
    /// parent), then for a presentation the blank deck body and the
    /// `presentation:` block.
    pub fn create_file(
        &self,
        name: String,
        tags: Vec<String>,
        template: Option<String>,
        parent: Option<String>,
        deck: bool,
    ) {
        self.spawn(|core| async move {
            let mut ws = core.ws.lock().await;
            let body = template
                .as_deref()
                .and_then(|t| templates::body_for(&ws.project, t, &name));
            let created = ws
                .project
                .create_file(&name, &json!(tags), body, parent.as_deref())
                .await;
            let mut entry = match created {
                Ok(entry) => entry,
                Err(e) => {
                    drop(ws);
                    send_failure(&core, e, "Couldn't create the file");
                    return;
                }
            };
            if deck {
                let filename = entry["filename"].as_str().unwrap_or_default().to_string();
                if template.is_none() {
                    if let Err(e) = ws
                        .project
                        .write_file(&filename, &blank_deck_body(&name))
                        .await
                    {
                        log::warn!("blank deck body: {e}");
                    }
                }
                if !entry["deck"].as_bool().unwrap_or(false) {
                    match ws
                        .project
                        .set_presentation(&filename, &default_presentation())
                        .await
                    {
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

    /// `file:duplicate`.
    pub fn duplicate_file(
        &self,
        source: String,
        name: String,
        tags: Vec<String>,
        parent: Option<String>,
    ) {
        self.spawn(|core| async move {
            let mut ws = core.ws.lock().await;
            let body = ws
                .project
                .find(&source)
                .and_then(|f| f["filename"].as_str().map(str::to_string))
                .and_then(|f| ws.project.read_file(&f).ok());
            let r = ws
                .project
                .create_file(&name, &json!(tags), body, parent.as_deref())
                .await;
            if r.is_ok() {
                ws.rebuild_links();
            }
            drop(ws);
            match r {
                Ok(entry) => core.send(Event::Created(entry)),
                Err(e) => send_failure(&core, e, "Couldn't duplicate the file"),
            }
        });
    }

    /// Markdown files dropped on the sidebar (`handleExternalDrop`): each
    /// becomes a note with the file's content.
    pub fn import_markdown(&self, files: Vec<PathBuf>) {
        self.spawn(|core| async move {
            for path in files {
                let Ok(content) = std::fs::read_to_string(&path) else {
                    continue;
                };
                let name = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let mut ws = core.ws.lock().await;
                match ws.project.create_file(&name, &json!([]), None, None).await {
                    Ok(mut entry) => {
                        let filename = entry["filename"].as_str().unwrap_or_default().to_string();
                        if let Err(e) = ws.project.write_file(&filename, &content).await {
                            log::warn!("dropped file {}: {e}", path.display());
                        }
                        if let Some(updated) = ws.project.find(entry["id"].as_str().unwrap_or("")) {
                            entry = updated.clone();
                        }
                        ws.rebuild_links();
                        drop(ws);
                        core.send(Event::Created(entry));
                    }
                    Err(e) => {
                        drop(ws);
                        send_failure(&core, e, "Couldn't add the dropped file");
                    }
                }
            }
        });
    }

    /// `file:rename`.
    pub fn rename_file(&self, id: String, name: String) {
        self.spawn(|core| async move {
            let mut ws = core.ws.lock().await;
            let r = ws.project.rename_file(&id, &name).await;
            if r.is_ok() {
                ws.rebuild_links();
            }
            drop(ws);
            match r {
                Ok(entry) => core.send(Event::Renamed { id, entry }),
                Err(e) => send_failure(&core, e, "Couldn't rename the file"),
            }
        });
    }

    /// `file:delete`.
    pub fn delete_file(&self, id: String) {
        self.spawn(|core| async move {
            let mut ws = core.ws.lock().await;
            let r = ws.project.delete_file(&id).await;
            if r.is_ok() {
                ws.links.remove_file(&id);
                ws.rebuild_links();
            }
            drop(ws);
            match r {
                Ok(()) => core.send(Event::Deleted(id)),
                Err(e) => send_failure(&core, e, "Couldn't delete the file"),
            }
        });
    }

    /// `file:setPresentation`: adds, replaces or (with null) removes a
    /// note's `presentation:` block.
    pub fn set_presentation(&self, id: String, filename: String, presentation: Value) {
        self.spawn(|core| async move {
            let r = core
                .ws
                .lock()
                .await
                .project
                .set_presentation(&filename, &presentation)
                .await;
            core.send(Event::Presentation {
                id,
                outcome: Outcome::of(r),
            });
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

    /// `templates:save` (Save as Template), reported in the Log panel.
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

    /// `file:import` (DOCX/PPTX).
    pub fn import(&self, path: PathBuf) {
        self.spawn(|core| async move {
            let r = {
                let mut ws = core.ws.lock().await;
                let r = import::import(&mut ws.project, &path).await;
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
                result: r.map_err(|e| {
                    if e.downcast_ref::<GitConfigRequired>().is_some() {
                        "git_config_required".to_string()
                    } else {
                        e.to_string()
                    }
                }),
            });
        });
    }

    /// `file:convertTo*`: writes the note to the Downloads folder.
    ///
    /// PDF goes through `noteliner-export` (Typst, in-process) instead of the
    /// Tauri build's WebKit print, with the same page (Letter, 0.5 in
    /// margins) and the note's name as the title heading; the layout runs on
    /// a blocking thread.
    pub fn export(&self, kind: ExportKind, filename: String, name: String) {
        self.spawn(|core| async move {
            let result = async {
                let (raw, project_dir) = {
                    let ws = core.ws.lock().await;
                    if !ws.project.is_open() {
                        anyhow::bail!("No project is open");
                    }
                    let raw = std::fs::read_to_string(ws.project.file_path(&filename)?)?;
                    (raw, ws.project.dir()?.to_path_buf())
                };
                let dir = downloads_dir();
                std::fs::create_dir_all(&dir)?;
                let slug = export::export_slug(&name);
                match kind {
                    ExportKind::Markdown => std::fs::write(dir.join(format!("{slug}.md")), &raw)?,
                    ExportKind::Html => std::fs::write(
                        dir.join(format!("{slug}.html")),
                        export::html_document(&name, &frontmatter::strip_body(&raw)),
                    )?,
                    ExportKind::Pdf => {
                        let pdf = pdf_bytes(core.exporter.clone(), raw, name, project_dir).await?;
                        std::fs::write(dir.join(format!("{slug}.pdf")), pdf)?
                    }
                }
                Ok(dir)
            }
            .await;
            core.send(Event::Exported {
                kind,
                result: result.map_err(|e: anyhow::Error| e.to_string()),
            });
        });
    }

    // --- Git (git:*) ------------------------------------------------------------

    /// Branch, remote and sync status, for the status bar and Sync dialog.
    pub fn git_state(&self) {
        self.spawn(|core| async move {
            let state = core.git_state().await;
            core.send(state);
        });
    }

    /// `git:getConfig`.
    pub fn git_config(&self) {
        self.spawn(|core| async move {
            let v = core.ws.lock().await.project.get_git_config().await;
            core.send(Event::GitConfig {
                name: v["name"].as_str().unwrap_or_default().to_string(),
                email: v["email"].as_str().unwrap_or_default().to_string(),
            });
        });
    }

    /// `git:setConfig`; an error comes back as `Error`.
    pub fn set_git_config(&self, name: String, email: String) {
        self.spawn(|core| async move {
            if let Err(e) = core
                .ws
                .lock()
                .await
                .project
                .set_git_config(&name, &email)
                .await
            {
                core.send(Event::Error(format!("Failed to save: {e}")));
            }
        });
    }

    /// A Sync-dialog operation; `GitDone` reports it, then `GitState`.
    pub fn git_op(&self, op: Op, url: Option<String>) {
        self.spawn(|core| async move {
            let result: Result<(), String> = async {
                let mut ws = core.ws.lock().await;
                let dir = ws
                    .project
                    .dir()
                    .map(Path::to_path_buf)
                    .map_err(|e| e.to_string())?;
                let git = ws.project.git.clone();
                match op {
                    Op::Connect => {
                        let url = url.unwrap_or_default();
                        git.git
                            .set_remote_url(&dir, &url)
                            .await
                            .map_err(|e| e.to_string())?;
                        let branch = git.git.current_branch(&dir).await.unwrap_or_default();
                        git.git
                            .push_upstream(&dir, &branch)
                            .await
                            .map_err(|e| e.to_string())?;
                    }
                    Op::Disconnect => {
                        git.git
                            .remove_remote(&dir)
                            .await
                            .map_err(|e| e.to_string())?;
                    }
                    // Failures are logged, not raised (as in the Tauri build).
                    Op::Push => {
                        git.push(&dir).await;
                    }
                    Op::Pull => {
                        git.pull(&dir).await;
                    }
                    Op::PullRebase => {
                        git.pull_rebase(&dir).await;
                    }
                    Op::Reset => {
                        let branch = git.git.current_branch(&dir).await;
                        git.git
                            .reset_to_remote(&dir, branch.as_deref())
                            .await
                            .map_err(|e| e.to_string())?;
                        let raw =
                            std::fs::read_to_string(dir.join(noteliner_core::project::INDEX_FILE))
                                .map_err(|e| e.to_string())?;
                        let index: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
                        ws.project.index = Some(index.clone());
                        ws.rebuild_links();
                        core.send(Event::IndexReloaded(index));
                    }
                }
                Ok(())
            }
            .await;
            core.send(Event::GitDone { op, result });
            let state = core.git_state().await;
            core.send(state);
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
                match updates::check(UPDATE_REPO, env!("CARGO_PKG_VERSION")).await {
                    Ok(UpdateCheck::Available(rel)) => UpdateStatus::Available {
                        version: rel.version,
                        notes: rel.notes,
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

/// The channels behind the Search, Backlinks, History and Attachments panes.
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

    /// `links:getAllNames`: for wikilink autocomplete, with marina-editor.
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

    /// `file:getHistory`.
    pub fn history(&self, filename: String) {
        self.spawn(|core| async move {
            let ws = core.ws.lock().await;
            let entries = match ws.project.dir() {
                Ok(dir) => ws.project.git.file_log(dir, &filename).await,
                Err(_) => Vec::new(),
            };
            drop(ws);
            core.send(Event::History { filename, entries });
        });
    }

    /// `file:getHistoryContent`: the body at a commit, frontmatter stripped.
    pub fn history_content(&self, hash: String, filename: String) {
        self.spawn(|core| async move {
            let ws = core.ws.lock().await;
            let content = match ws.project.dir() {
                Ok(dir) => ws
                    .project
                    .git
                    .git
                    .file_at_commit(dir, &hash, &filename)
                    .await
                    .map(|r| frontmatter::strip_body(&r)),
                Err(_) => None,
            };
            drop(ws);
            core.send(Event::HistoryContent { hash, content });
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

fn opened(folder: &Path, res: Result<Value>) -> Event {
    match res {
        Ok(r) if r["status"] == "loaded" => Event::ProjectOpened {
            folder: folder.to_path_buf(),
            index: r["index"].clone(),
            needs_git_config: r["needsGitConfig"].as_bool().unwrap_or(false),
        },
        Ok(r) if r["status"] == "needs_setup" => Event::NeedsSetup(folder.to_path_buf()),
        Ok(r) => Event::OpenFailed(format!("Unexpected result: {r}")),
        Err(e) => Event::OpenFailed(e.to_string()),
    }
}

/// A failed write: the git-identity case opens Project Settings; anything
/// else is a toast.
fn send_failure(core: &Core, e: anyhow::Error, what: &str) {
    if e.downcast_ref::<GitConfigRequired>().is_some() {
        core.send(Event::GitConfigRequired);
    } else {
        core.send(Event::Error(format!("{what}: {e}")));
    }
}

/// Lays out `raw` (a note with its frontmatter) as a PDF on a blocking thread.
pub async fn pdf_bytes(
    exporter: Arc<noteliner_export::LazyExporter>,
    raw: String,
    name: String,
    project_dir: PathBuf,
) -> Result<Vec<u8>> {
    let options = pdf_options(name, project_dir);
    tokio::task::spawn_blocking(move || exporter.get().export(&raw, &options))
        .await?
        .map_err(|e| anyhow::anyhow!("{e}"))
}

/// What today's export prints (`export::pdf_document` through WebKit): Letter,
/// half-inch margins, the note's name as the heading, images from
/// `_attachments/`. Wikilinks render as styled text, since a standalone PDF
/// has no other notes to jump to.
pub fn pdf_options(name: String, project_dir: PathBuf) -> noteliner_export::ExportOptions {
    noteliner_export::ExportOptions {
        title: noteliner_export::Title::Text(name),
        base_dir: Some(project_dir),
        ..Default::default()
    }
}

/// Electron's `app.getPath('downloads')`.
pub fn downloads_dir() -> PathBuf {
    let home = marina_core::paths::home_dir();
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

/// `system:getInfo`: the New Project dialog's defaults.
pub fn system_info() -> (String, String, PathBuf) {
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_default();
    let host = std::fs::read_to_string("/proc/sys/kernel/hostname")
        .map(|h| h.trim().to_string())
        .or_else(|_| std::env::var("HOSTNAME"))
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .unwrap_or_default();
    (user, host, marina_core::paths::home_dir())
}
