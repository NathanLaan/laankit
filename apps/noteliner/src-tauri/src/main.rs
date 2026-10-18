// NoteLiner — Tauri host. Port of src/main/main.js; the renderer and
// preload are shared with the Electron build, and the backend lives in
// crates/noteliner-core.
//
// `noteliner-tauri --mcp-bridge` runs the stdio ↔ socket bridge MCP clients
// spawn, instead of the app (replaces bin/noteliner-mcp-bridge.js, so no
// Node runtime is needed).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(target_os = "linux")]
mod webkit;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use anyhow::Context;
use marina_core::debounce::Debouncer;
use marina_tauri::dialog::{self, Filter};
use marina_tauri::{bytes_value, json, null, Args, Host, HostConfig, IpcResult, WindowSpec};
use noteliner_core::git::{GitConfigRequired, NoteGit};
use noteliner_core::links::LinkGraph;
use noteliner_core::mcp::{self, ConfirmFuture, McpHost, McpServer};
use noteliner_core::project::Project;
use noteliner_core::window_state::WindowState;
use noteliner_core::workspace::{Shared, Workspace};
use noteliner_core::{export, frontmatter, import, templates};
use serde_json::{json as j, Value};
use tauri::{AppHandle, Manager, WebviewWindow, WindowEvent};
use tauri_plugin_opener::OpenerExt;

const PRELOAD: &str = include_str!(concat!(env!("OUT_DIR"), "/preload.js"));
const PRODUCT: &str = "NoteLiner";
const MAX_RECENT: usize = 5;

const HELP_WINDOW: WindowSpec = WindowSpec {
    page: "help.html",
    title: "NoteLiner Help",
    width: 1000.0,
    height: 720.0,
    min_width: 560.0,
    min_height: 360.0,
};

struct App {
    host: Arc<Host>,
    ws: Shared,
    mcp: Arc<McpServer>,
    window_state: WindowState,
    /// Open project folder, readable from window-event callbacks without
    /// awaiting the workspace lock.
    project_path: Mutex<Option<String>>,
    /// Last un-maximized bounds, for saving while maximized.
    normal_bounds: Mutex<Option<Value>>,
    bounds_saver: Debouncer,
    confirms: Mutex<HashMap<u64, tokio::sync::oneshot::Sender<String>>>,
    next_confirm: AtomicU64,
}

/// Commit failures caused by a missing git identity become the
/// `{ error: 'git_config_required' }` reply the renderer expects.
fn git_config_reply(e: &anyhow::Error) -> Option<Value> {
    e.downcast_ref::<GitConfigRequired>().map(|_| j!({ "error": "git_config_required" }))
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}

fn downloads_dir() -> PathBuf {
    if let Ok(out) = std::process::Command::new("xdg-user-dir").arg("DOWNLOAD").output() {
        let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if out.status.success() && !p.is_empty() && p != marina_core::paths::home_dir().display().to_string() {
            return PathBuf::from(p);
        }
    }
    marina_core::paths::home_dir().join("Downloads")
}

impl App {
    fn prefs_flag(&self, key: &str, default: bool) -> bool {
        self.host.pref(key).as_bool().unwrap_or(default)
    }

    fn runtime_path(&self) -> PathBuf {
        self.host.user_data.join("mcp-runtime.json")
    }

    async fn maybe_start_mcp(&self) {
        if !self.prefs_flag("mcpEnabled", false) || self.mcp.is_running() {
            return;
        }
        if !self.ws.lock().await.project.is_open() {
            return;
        }
        if let Err(e) = self.mcp.start(&self.runtime_path()).await {
            eprintln!("[MCP] failed to start: {e}");
            self.ws.lock().await.project.git.log(format!("[MCP] failed to start: {e}"));
        }
    }

    fn reject_pending_confirms(&self) {
        for (_, tx) in self.confirms.lock().unwrap().drain() {
            let _ = tx.send("deny".into());
        }
    }

    fn set_project_path(&self, path: Option<&Path>) {
        *self.project_path.lock().unwrap() = path.map(|p| p.display().to_string());
    }

    // --- Recent projects (recent-projects.json) -------------------------------

    fn recent_path(&self) -> PathBuf {
        self.host.user_data.join("recent-projects.json")
    }

    fn recent(&self) -> Vec<Value> {
        marina_core::json::read::<Vec<Value>>(&self.recent_path()).unwrap_or_default()
    }

    fn save_recent(&self, list: &[Value]) -> anyhow::Result<()> {
        marina_core::json::write(&self.recent_path(), list)
    }

    // --- Window bounds ----------------------------------------------------------

    fn bounds_of(window: &WebviewWindow) -> Option<Value> {
        let scale = window.scale_factor().ok()?;
        let pos = window.outer_position().ok()?.to_logical::<f64>(scale);
        let size = window.inner_size().ok()?.to_logical::<f64>(scale);
        Some(j!({ "x": pos.x.round(), "y": pos.y.round(), "width": size.width.round(), "height": size.height.round() }))
    }

    fn on_window_event(self: &Arc<Self>, window: &WebviewWindow, event: &WindowEvent) {
        let Some(folder) = self.project_path.lock().unwrap().clone() else { return };
        let maximized = window.is_maximized().unwrap_or(false);
        match event {
            WindowEvent::Moved(_) | WindowEvent::Resized(_) => {
                if maximized {
                    let normal = self.normal_bounds.lock().unwrap().clone();
                    if let Some(b) = normal {
                        self.window_state.set_bounds(&folder, b, true, false);
                    }
                    return;
                }
                let Some(bounds) = Self::bounds_of(window) else { return };
                *self.normal_bounds.lock().unwrap() = Some(bounds.clone());
                let me = self.clone();
                self.bounds_saver.schedule(Duration::from_secs(1), move || async move {
                    me.window_state.set_bounds(&folder, bounds, false, false);
                });
            }
            WindowEvent::CloseRequested { .. } | WindowEvent::Destroyed => {
                self.bounds_saver.cancel();
                let bounds = if maximized { self.normal_bounds.lock().unwrap().clone() } else { Self::bounds_of(window) };
                if let Some(b) = bounds {
                    self.window_state.set_bounds(&folder, b, maximized, true);
                }
            }
            _ => {}
        }
    }

    fn restore_bounds(&self, window: &WebviewWindow, folder: &str) -> anyhow::Result<()> {
        let Some((bounds, maximized)) = self.window_state.bounds(folder) else { return Ok(()) };
        if bounds.is_null() {
            return Ok(());
        }
        let (x, y, w, h) = (
            bounds["x"].as_f64().unwrap_or(0.0),
            bounds["y"].as_f64().unwrap_or(0.0),
            bounds["width"].as_f64().unwrap_or(1200.0),
            bounds["height"].as_f64().unwrap_or(800.0),
        );
        let visible = window.available_monitors()?.iter().any(|m| {
            let sf = m.scale_factor();
            let pos = m.position().to_logical::<f64>(sf);
            let size = m.size().to_logical::<f64>(sf);
            x < pos.x + size.width && x + w > pos.x && y < pos.y + size.height && y + h > pos.y
        });
        if visible {
            window.set_position(tauri::LogicalPosition::new(x, y))?;
            window.set_size(tauri::LogicalSize::new(w, h))?;
            *self.normal_bounds.lock().unwrap() = Some(bounds);
        }
        if maximized {
            window.maximize()?;
        }
        Ok(())
    }

    fn create_main_window(self: &Arc<Self>, app: &AppHandle) -> tauri::Result<WebviewWindow> {
        let win = self.host.create_main_window(app)?;
        let me = self.clone();
        let w = win.clone();
        win.on_window_event(move |ev| me.on_window_event(&w, ev));
        #[cfg(target_os = "linux")]
        webkit::set_spellcheck(&win, self.prefs_flag("spellCheckEnabled", true));
        Ok(win)
    }

    // --- IPC ------------------------------------------------------------------

    async fn route(self: &Arc<Self>, app: &AppHandle, window: &WebviewWindow, channel: &str, args: &Args) -> IpcResult {
        match channel {
            "dialog:openFolder" => json(dialog::pick_folder(app, Some(window), None)),

            // --- Project ---
            "project:open" => {
                let folder = PathBuf::from(args.str(0)?);
                let res = {
                    let mut ws = self.ws.lock().await;
                    let res = ws.project.open(&folder).await?;
                    if res["status"] == "loaded" {
                        ws.rebuild_links();
                    }
                    res
                };
                if res["status"] == "loaded" {
                    self.set_project_path(Some(&folder));
                    self.maybe_start_mcp().await;
                }
                json(res)
            }
            "project:init" => {
                let folder = PathBuf::from(args.str(0)?);
                let res = {
                    let mut ws = self.ws.lock().await;
                    let res = ws.project.init(&folder, args.opt_str(1).as_deref()).await?;
                    ws.rebuild_links();
                    res
                };
                self.set_project_path(Some(&folder));
                self.maybe_start_mcp().await;
                json(res)
            }
            "project:close" => {
                if self.project_path.lock().unwrap().is_none() {
                    return null();
                }
                self.reject_pending_confirms();
                self.mcp.stop().await;
                let mut ws = self.ws.lock().await;
                if let Ok(dir) = ws.project.dir().map(Path::to_path_buf) {
                    ws.project.git.flush_push(&dir).await;
                }
                ws.project.close();
                ws.links.reset();
                self.set_project_path(None);
                null()
            }
            "project:getIndex" => json(self.ws.lock().await.project.index.clone()),
            "project:saveIndex" => {
                let mut ws = self.ws.lock().await;
                ws.project.save_index(args.value(0).clone()).await?;
                null()
            }

            // --- Notes ---
            "file:read" => json(self.ws.lock().await.project.read_file(&args.str(0)?)?),
            "file:getFrontmatter" => {
                let ws = self.ws.lock().await;
                if !ws.project.is_open() {
                    return json(j!({}));
                }
                json(ws.project.read_frontmatter(&args.str(0)?))
            }
            "file:setPresentation" => {
                let mut ws = self.ws.lock().await;
                if !ws.project.is_open() {
                    return json(j!({ "error": "no_project" }));
                }
                match ws.project.set_presentation(&args.str(0)?, args.value(1)).await {
                    Ok(p) => json(j!({ "presentation": p })),
                    Err(e) => json(git_config_reply(&e).unwrap_or_else(|| j!({ "error": e.to_string() }))),
                }
            }
            "file:write" => {
                let filename = args.str(0)?;
                let content = args.str(1).unwrap_or_default();
                let mut ws = self.ws.lock().await;
                match ws.project.write_file(&filename, &content).await {
                    Ok(()) => {
                        let id = ws.project.find_by_filename(&filename).map(|f| s(&f["id"]).to_string());
                        if let Some(id) = id {
                            ws.scan_links(&id);
                        }
                        null()
                    }
                    Err(e) => git_config_reply(&e).map(json).unwrap_or(Err(e)),
                }
            }
            "file:create" | "file:duplicate" => {
                let duplicate = channel == "file:duplicate";
                // create: (name, tags, templateId, parentId); duplicate: (sourceId, name, tags, parentId)
                let (name, tags, extra, parent) = if duplicate {
                    (args.str(1)?, args.value(2).clone(), args.opt_str(0), args.opt_str(3))
                } else {
                    (args.str(0)?, args.value(1).clone(), args.opt_str(2), args.opt_str(3))
                };
                let mut ws = self.ws.lock().await;
                let body = match (&extra, duplicate) {
                    (Some(template), false) => templates::body_for(&ws.project, template, &name),
                    (Some(source), true) => ws.project.find(source).map(|f| s(&f["filename"]).to_string()).and_then(|f| ws.project.read_file(&f).ok()),
                    _ => None,
                };
                match ws.project.create_file(&name, &tags, body, parent.as_deref()).await {
                    Ok(entry) => {
                        ws.rebuild_links();
                        json(entry)
                    }
                    Err(e) => git_config_reply(&e).map(json).unwrap_or(Err(e)),
                }
            }
            "templates:list" => json(templates::list(&self.ws.lock().await.project)),
            "templates:save" => {
                let ws = self.ws.lock().await;
                if !ws.project.is_open() {
                    return json(j!({ "error": "no_project" }));
                }
                match templates::save(&ws.project, &args.str(0).unwrap_or_default(), &args.str(1).unwrap_or_default()).await {
                    Ok(v) => json(v),
                    Err(e) => json(git_config_reply(&e).unwrap_or_else(|| j!({ "error": e.to_string() }))),
                }
            }
            "file:delete" => {
                let id = args.str(0)?;
                let mut ws = self.ws.lock().await;
                match ws.project.delete_file(&id).await {
                    Ok(()) => {
                        ws.links.remove_file(&id);
                        ws.rebuild_links();
                        null()
                    }
                    Err(e) => git_config_reply(&e).map(json).unwrap_or(Err(e)),
                }
            }
            "file:rename" => {
                let mut ws = self.ws.lock().await;
                match ws.project.rename_file(&args.str(0)?, &args.str(1).unwrap_or_default()).await {
                    Ok(entry) => {
                        ws.rebuild_links();
                        json(entry)
                    }
                    Err(e) => git_config_reply(&e).map(json).unwrap_or(Err(e)),
                }
            }

            // --- Import ---
            "dialog:openImportFile" => json(dialog::pick_file(
                app,
                Some(window),
                None,
                &[
                    Filter { name: "Documents", extensions: &["docx", "pptx"] },
                    Filter { name: "Word Document", extensions: &["docx"] },
                    Filter { name: "PowerPoint Presentation", extensions: &["pptx"] },
                ],
            )),
            "file:import" => {
                let mut ws = self.ws.lock().await;
                match import::import(&mut ws.project, Path::new(&args.str(0)?)).await {
                    Ok(res) => {
                        ws.rebuild_links();
                        json(res)
                    }
                    Err(e) => json(git_config_reply(&e).unwrap_or_else(|| j!({ "error": e.to_string() }))),
                }
            }

            // --- Links ---
            "links:getBacklinks" => {
                let ws = self.ws.lock().await;
                json(ws.links.backlink_snippets(&ws.project, &args.str(0)?))
            }
            "links:getAllNames" => json(LinkGraph::all_note_names(&self.ws.lock().await.project)),
            "links:rebuild" => {
                self.ws.lock().await.rebuild_links();
                null()
            }

            // --- Git ---
            "git:push" | "git:pull" | "git:pullRebase" => {
                let ws = self.ws.lock().await;
                let Ok(dir) = ws.project.dir().map(Path::to_path_buf) else { return null() };
                let git = &ws.project.git;
                json(match channel {
                    "git:push" => git.push(&dir).await,
                    "git:pull" => git.pull(&dir).await,
                    _ => git.pull_rebase(&dir).await,
                })
            }
            "git:getRemoteUrl" => {
                let ws = self.ws.lock().await;
                match ws.project.dir() {
                    Ok(dir) => json(ws.project.git.git.get_remote_url(dir).await),
                    Err(_) => null(),
                }
            }
            "git:setRemoteUrl" => {
                let ws = self.ws.lock().await;
                let Ok(dir) = ws.project.dir() else { return null() };
                json(ws.project.git.git.set_remote_url(dir, &args.str(0)?).await?)
            }
            "git:removeRemote" => {
                let ws = self.ws.lock().await;
                let Ok(dir) = ws.project.dir() else { return null() };
                json(ws.project.git.git.remove_remote(dir).await?)
            }
            "git:getSyncStatus" => {
                let ws = self.ws.lock().await;
                match ws.project.dir() {
                    Ok(dir) => json(ws.project.git.sync_status(dir).await),
                    Err(_) => json(j!({ "status": "error", "message": "No project open" })),
                }
            }
            "git:getBranch" => {
                let ws = self.ws.lock().await;
                match ws.project.dir() {
                    Ok(dir) => json(ws.project.git.git.current_branch(dir).await),
                    Err(_) => null(),
                }
            }
            "git:pushUpstream" => {
                let ws = self.ws.lock().await;
                let Ok(dir) = ws.project.dir() else { return null() };
                let branch = ws.project.git.git.current_branch(dir).await.unwrap_or_default();
                json(ws.project.git.git.push_upstream(dir, &branch).await?)
            }
            "git:resetToRemote" => {
                let mut ws = self.ws.lock().await;
                let Ok(dir) = ws.project.dir().map(Path::to_path_buf) else { return null() };
                let branch = ws.project.git.git.current_branch(&dir).await;
                ws.project.git.git.reset_to_remote(&dir, branch.as_deref()).await?;
                if let Ok(raw) = std::fs::read_to_string(dir.join(noteliner_core::project::INDEX_FILE)) {
                    ws.project.index = Some(serde_json::from_str(&raw)?);
                }
                json(j!({ "index": ws.project.index }))
            }
            "git:getConfig" => json(self.ws.lock().await.project.get_git_config().await),
            "git:setConfig" => {
                self.ws.lock().await.project.set_git_config(&args.str(0)?, &args.str(1)?).await?;
                null()
            }

            // --- Attachments ---
            "file:addAttachment" => {
                let data = args.bytes(1)?;
                let mut ws = self.ws.lock().await;
                json(ws.project.add_attachment(&args.str(0)?, &data, &args.str(2)?).await?)
            }
            "file:removeAttachment" => {
                self.ws.lock().await.project.remove_attachment(&args.str(0)?, &args.str(1)?).await?;
                null()
            }
            "file:getAttachmentPath" => json(self.ws.lock().await.project.attachment_path(&args.str(0)?)?),
            "dialog:openFiles" => {
                let files: Vec<Value> = dialog::pick_files(app, Some(window), None, &[])
                    .into_iter()
                    .filter_map(|p| {
                        let data = std::fs::read(&p).ok()?;
                        let name = p.file_name()?.to_string_lossy().into_owned();
                        Some(j!({ "buffer": bytes_value(&data), "name": name }))
                    })
                    .collect();
                json(files)
            }
            "shell:openPath" => {
                let path = args.str(0)?;
                // Electron resolves to "" on success or an error message.
                json(match app.opener().open_path(&path, None::<&str>) {
                    Ok(()) => String::new(),
                    Err(e) => e.to_string(),
                })
            }

            // --- Recent projects ---
            "projects:getRecent" => json(self.recent()),
            "projects:addRecent" => {
                let folder = args.str(0)?;
                let mut list: Vec<Value> = self.recent().into_iter().filter(|p| p["path"] != folder.as_str()).collect();
                let name = Path::new(&folder).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                list.insert(0, j!({ "path": folder, "name": name, "openedAt": noteliner_core::project::now_iso() }));
                list.truncate(MAX_RECENT);
                self.save_recent(&list)?;
                null()
            }
            "projects:removeRecent" => {
                let folder = args.str(0)?;
                let list: Vec<Value> = self.recent().into_iter().filter(|p| p["path"] != folder.as_str()).collect();
                self.save_recent(&list)?;
                null()
            }

            // --- System ---
            "system:getInfo" => {
                let hostname = std::fs::read_to_string("/proc/sys/kernel/hostname")
                    .map(|h| h.trim().to_string())
                    .or_else(|_| std::env::var("HOSTNAME"))
                    .unwrap_or_default();
                json(j!({
                    "username": std::env::var("USER").unwrap_or_default(),
                    "hostname": hostname,
                    "homeDir": marina_core::paths::home_dir(),
                }))
            }
            "fs:ensureDir" => {
                std::fs::create_dir_all(args.str(0)?)?;
                null()
            }
            "search:query" => {
                let ws = self.ws.lock().await;
                let case = args.value(1)["caseSensitive"].as_bool().unwrap_or(false);
                json(ws.project.search(&args.opt_str(0).unwrap_or_default(), case))
            }

            // --- Convert / export (to the Downloads folder) ---
            "file:convertToHtml" | "file:convertToMarkdown" | "file:convertToPdf" => {
                let (filename, name) = (args.str(0)?, args.str(1)?);
                let (raw, attachments) = {
                    let ws = self.ws.lock().await;
                    if !ws.project.is_open() {
                        return null();
                    }
                    (std::fs::read_to_string(ws.project.file_path(&filename)?)?, ws.project.attachments_dir()?)
                };
                let dir = downloads_dir();
                std::fs::create_dir_all(&dir)?;
                let slug = export::export_slug(&name);
                let out = match channel {
                    "file:convertToMarkdown" => {
                        let out = dir.join(format!("{slug}.md"));
                        std::fs::write(&out, &raw)?;
                        out
                    }
                    "file:convertToHtml" => {
                        let out = dir.join(format!("{slug}.html"));
                        std::fs::write(&out, export::html_document(&name, &frontmatter::strip_body(&raw)))?;
                        out
                    }
                    _ => {
                        let out = dir.join(format!("{slug}.pdf"));
                        let html = export::pdf_document(&name, &frontmatter::strip_body(&raw), &attachments);
                        #[cfg(target_os = "linux")]
                        webkit::print_to_pdf(app, html, &out).await?;
                        #[cfg(not(target_os = "linux"))]
                        {
                            let _ = html;
                            return Err(anyhow::anyhow!("PDF export is only implemented on Linux"));
                        }
                        out
                    }
                };
                json(j!({ "outputPath": out, "downloadsDir": dir }))
            }

            // --- History ---
            "file:getHistory" => {
                let ws = self.ws.lock().await;
                match ws.project.dir() {
                    Ok(dir) => json(ws.project.git.file_log(dir, &args.str(0)?).await),
                    Err(_) => json(j!([])),
                }
            }
            "file:getHistoryContent" => {
                let ws = self.ws.lock().await;
                let Ok(dir) = ws.project.dir() else { return null() };
                let raw = ws.project.git.git.file_at_commit(dir, &args.str(0)?, &args.str(1)?).await;
                json(raw.map(|r| frontmatter::strip_body(&r)))
            }

            // --- Window state ---
            "window-state:getLayout" => json(self.window_state.layout(&args.str(0)?)),
            "window-state:saveLayout" => {
                self.window_state.set_layout(&args.str(0)?, args.value(1).clone());
                null()
            }
            "window-state:restoreBounds" => {
                if let Some(main) = self.host.main_window(app) {
                    self.restore_bounds(&main, &args.str(0)?)?;
                }
                null()
            }

            // --- MCP ---
            "mcp:getStatus" => {
                let exe = std::env::current_exe().unwrap_or_default();
                json(j!({
                    "enabled": self.prefs_flag("mcpEnabled", false),
                    "running": self.mcp.is_running(),
                    "socketPath": self.mcp.socket_path(),
                    "bridgePath": exe,
                    "bridgeCommand": exe,
                    "bridgeArgs": ["--mcp-bridge"],
                    "projectOpen": self.ws.lock().await.project.is_open(),
                    "confirmWrites": self.prefs_flag("mcpConfirmWrites", false),
                    "disabledTools": self.host.pref("mcpDisabledTools").as_array().cloned().unwrap_or_default(),
                    "tools": mcp::tool_classification(),
                }))
            }
            "mcp:confirm-response" => {
                let id = args.i64(0).unwrap_or(-1) as u64;
                let decision = match args.opt_str(1).as_deref() {
                    Some(d @ ("allow" | "session" | "deny")) => d.to_string(),
                    _ => "deny".into(),
                };
                let tx = self.confirms.lock().unwrap().remove(&id);
                json(match tx {
                    Some(tx) => tx.send(decision).is_ok(),
                    None => false,
                })
            }

            "help:open" => {
                self.host.open_secondary(app, "help", &HELP_WINDOW)?;
                null()
            }
            "app:relaunch" => {
                if let Some(main) = self.host.main_window(app) {
                    self.on_window_event(&main, &WindowEvent::Destroyed);
                }
                if cfg!(debug_assertions) {
                    let old = self.host.main_window(app);
                    self.create_main_window(app)?;
                    if let Some(old) = old {
                        old.destroy()?;
                    }
                    null()
                } else {
                    app.restart()
                }
            }

            _ => self.host.route(app, window, channel, args).await,
        }
    }
}

/// MCP ↔ app glue: logs go to the Log panel, prefs are read live, and
/// write confirmations round-trip through the renderer's modal.
struct McpBridgeHost {
    app: AppHandle,
}

impl McpHost for McpBridgeHost {
    fn log(&self, msg: String) {
        if let Some(state) = self.app.try_state::<Arc<App>>() {
            state.host.send(&self.app, "git:log", msg);
        }
    }

    fn prefs(&self) -> (bool, Vec<String>) {
        let Some(state) = self.app.try_state::<Arc<App>>() else { return (false, Vec::new()) };
        let disabled = state
            .host
            .pref("mcpDisabledTools")
            .as_array()
            .map(|a| a.iter().filter_map(|t| t.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        (state.prefs_flag("mcpConfirmWrites", false), disabled)
    }

    fn confirm(&self, tool: &str, summary: &str, args: &Value) -> ConfirmFuture {
        let app = self.app.clone();
        let payload = (tool.to_string(), summary.to_string(), args.clone());
        Box::pin(async move {
            let Some(state) = app.try_state::<Arc<App>>().map(|s| s.inner().clone()) else { return "deny".to_string() };
            // No UI to ask → deny rather than run a write the user can't see.
            if state.host.main_window(&app).is_none() {
                return "deny".into();
            }
            let id = state.next_confirm.fetch_add(1, Ordering::SeqCst) + 1;
            let (tx, rx) = tokio::sync::oneshot::channel();
            state.confirms.lock().unwrap().insert(id, tx);
            state.host.send(&app, "mcp:confirm-request", j!({ "id": id, "tool": payload.0, "summary": payload.1, "args": payload.2 }));
            rx.await.unwrap_or_else(|_| "deny".into())
        })
    }
}

#[tauri::command]
async fn ipc(
    app: AppHandle,
    window: WebviewWindow,
    state: tauri::State<'_, Arc<App>>,
    channel: String,
    args: Vec<Value>,
) -> Result<tauri::ipc::Response, String> {
    let me = state.inner().clone();
    marina_tauri::respond(me.route(&app, &window, &channel, &Args(args)).await)
}

/// `attachment:///<file>` → the open project's `_attachments/<file>`.
fn attachment_protocol(app: &AppHandle, request: tauri::http::Request<Vec<u8>>, responder: tauri::UriSchemeResponder) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let not_found = || tauri::http::Response::builder().status(404).body(b"Not found".to_vec()).unwrap();
        let Some(state) = app.try_state::<Arc<App>>().map(|s| s.inner().clone()) else {
            return responder.respond(not_found());
        };
        let path = request.uri().path().trim_start_matches('/').to_string();
        let name = percent_decode(&path);
        if name.is_empty() || name.contains('/') || name.contains("..") {
            return responder.respond(not_found());
        }
        let file = state.ws.lock().await.project.attachment_path(&name).ok();
        match file.and_then(|f| std::fs::read(f).ok()) {
            Some(data) => {
                let mime = mime_for(&name);
                responder.respond(
                    tauri::http::Response::builder()
                        .header("Content-Type", mime)
                        .header("Access-Control-Allow-Origin", "*")
                        .body(data)
                        .unwrap(),
                )
            }
            None => responder.respond(not_found()),
        }
    });
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn mime_for(name: &str) -> &'static str {
    match noteliner_core::project::extname(name).to_lowercase().as_str() {
        ".png" => "image/png",
        ".jpg" | ".jpeg" => "image/jpeg",
        ".gif" => "image/gif",
        ".webp" => "image/webp",
        ".svg" => "image/svg+xml",
        ".bmp" => "image/bmp",
        ".pdf" => "application/pdf",
        ".mp4" => "video/mp4",
        ".mp3" => "audio/mpeg",
        _ => "application/octet-stream",
    }
}

fn main() {
    if std::env::args().any(|a| a == "--mcp-bridge") {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("tokio runtime");
        let code = rt.block_on(mcp::run_bridge(&marina_core::paths::user_data_dir(PRODUCT)));
        std::process::exit(code);
    }

    let host = Host::new(HostConfig {
        product_name: PRODUCT,
        preload: PRELOAD,
        main_window: WindowSpec {
            page: "index.html",
            title: PRODUCT,
            width: 1200.0,
            height: 800.0,
            min_width: 800.0,
            min_height: 600.0,
        },
        prefs_defaults: j!({
            "customTitlebar": false,
            "writeFrontmatter": true,
            "mcpEnabled": false,
            "mcpConfirmWrites": false,
            "mcpDisabledTools": [],
            "spellCheckEnabled": true,
            "filesSortMode": "user",
        }),
        update_repo: Some("NathanLaan/noteliner"),
    });

    // Git output streams to the renderer's Log panel once the app exists.
    let app_cell: Arc<OnceLock<AppHandle>> = Arc::new(OnceLock::new());
    let log_cell = app_cell.clone();
    let log_host = host.clone();
    let git = NoteGit::new(Arc::new(move |msg| {
        if let Some(app) = log_cell.get() {
            log_host.send(app, "git:log", msg);
        }
    }));
    let mut project = Project::new(git);
    project.write_frontmatter = host.pref("writeFrontmatter").as_bool().unwrap_or(true);
    let ws = Workspace::new(project);

    tauri::Builder::default()
        // Single instance: a second launch focuses the running window.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(w) = app.state::<Arc<App>>().host.main_window(app) {
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .register_asynchronous_uri_scheme_protocol("attachment", |ctx, request, responder| {
            attachment_protocol(ctx.app_handle(), request, responder)
        })
        .invoke_handler(tauri::generate_handler![ipc])
        .setup(move |app| {
            let handle = app.handle().clone();
            let _ = app_cell.set(handle.clone());
            let mcp = McpServer::new(ws.clone(), Arc::new(McpBridgeHost { app: handle.clone() }), &app.package_info().version.to_string());
            let state = Arc::new(App {
                window_state: WindowState::new(host.user_data.join("window-state.json")),
                host: host.clone(),
                ws: ws.clone(),
                mcp,
                project_path: Mutex::new(None),
                normal_bounds: Mutex::new(None),
                bounds_saver: Debouncer::new(),
                confirms: Mutex::new(HashMap::new()),
                next_confirm: AtomicU64::new(0),
            });
            app.manage(state.clone());

            // NoteLiner-specific side effects of ui:setPrefs.
            let hook_state = state.clone();
            host.on_prefs_change(move |app, patch, _prefs| {
                let st = hook_state.clone();
                if let Some(on) = patch.get("writeFrontmatter").and_then(Value::as_bool) {
                    let st = st.clone();
                    tauri::async_runtime::spawn(async move { st.ws.lock().await.project.write_frontmatter = on });
                }
                if let Some(on) = patch.get("mcpEnabled").and_then(Value::as_bool) {
                    let st = st.clone();
                    tauri::async_runtime::spawn(async move {
                        if on { st.maybe_start_mcp().await } else { st.mcp.stop().await }
                    });
                }
                if let Some(on) = patch.get("spellCheckEnabled").and_then(Value::as_bool) {
                    #[cfg(target_os = "linux")]
                    if let Some(w) = st.host.main_window(app) {
                        webkit::set_spellcheck(&w, on);
                    }
                    st.host.send(app, "ui:spellcheck-changed", on);
                }
            });

            state.create_main_window(app.handle()).context("creating main window")?;
            if !cfg!(debug_assertions) {
                let (h, a) = (host.clone(), handle.clone());
                tauri::async_runtime::spawn(async move {
                    h.updater.check(&h, &a).await;
                });
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building NoteLiner")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                if let Some(state) = app.try_state::<Arc<App>>() {
                    state.reject_pending_confirms();
                    let state = state.inner().clone();
                    tauri::async_runtime::block_on(async move {
                        state.mcp.stop().await;
                        let ws = state.ws.lock().await;
                        if let Ok(dir) = ws.project.dir() {
                            ws.project.git.flush_push(dir).await;
                        }
                    });
                }
            }
        });
}
