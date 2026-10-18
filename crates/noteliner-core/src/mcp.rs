//! Local MCP server: JSON-RPC 2.0 over a user-only Unix socket, reached
//! by MCP clients through `noteliner --mcp-bridge` (stdio ↔ socket).
//! Port of `apps/noteliner/src/main/mcp-service.js`; the tool, resource, and prompt
//! surface is unchanged.

use std::collections::HashSet;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use base64::Engine;
use serde_json::{json, Value};
#[cfg(unix)]
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
#[cfg(unix)]
use tokio::net::{UnixListener, UnixStream};
use tokio::task::JoinSet;

use crate::workspace::Shared;

pub const PROTOCOL_VERSION: &str = "2024-11-05";
const MAX_BODY_BYTES: usize = 10 * 1024 * 1024;
const MAX_FRAME_BYTES: usize = 32 * 1024 * 1024;

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const INTERNAL_ERROR: i64 = -32603;

pub const WRITE_TOOLS: [&str; 7] =
    ["create_note", "update_note", "delete_note", "rename_note", "set_tags", "add_attachment", "remove_attachment"];
pub const READ_TOOLS: [&str; 6] = ["list_notes", "read_note", "search", "get_backlinks", "list_attachments", "list_tags"];

pub type ConfirmFuture = Pin<Box<dyn Future<Output = String> + Send>>;

/// What the server needs from the app around it.
pub trait McpHost: Send + Sync + 'static {
    fn log(&self, msg: String);
    /// (confirm writes, disabled tool names), read live per call.
    fn prefs(&self) -> (bool, Vec<String>);
    /// Ask the user; resolves to "allow" | "session" | "deny".
    fn confirm(&self, tool: &str, summary: &str, args: &Value) -> ConfirmFuture;
}

/// Error surfaced to the model as a tool result (`isError: true`).
struct ToolError(String);

/// Error returned as a JSON-RPC error object.
struct RpcError(i64, String);

impl From<ToolError> for RpcError {
    fn from(e: ToolError) -> Self {
        RpcError(INTERNAL_ERROR, e.0)
    }
}

fn tool_err(msg: impl Into<String>) -> ToolError {
    ToolError(msg.into())
}

impl From<anyhow::Error> for ToolError {
    fn from(e: anyhow::Error) -> Self {
        ToolError(e.to_string())
    }
}

pub fn tool_classification() -> Value {
    json!({ "read": READ_TOOLS, "write": WRITE_TOOLS })
}

struct Running {
    socket_path: PathBuf,
    runtime_path: PathBuf,
    tasks: JoinSet<()>,
}

pub struct McpServer {
    ws: Shared,
    host: Arc<dyn McpHost>,
    version: String,
    running: Mutex<Option<Running>>,
    session_allowed: Mutex<HashSet<String>>,
}

pub fn socket_path() -> PathBuf {
    std::env::temp_dir().join(format!("noteliner-mcp-{}.sock", std::process::id()))
}

impl McpServer {
    pub fn new(ws: Shared, host: Arc<dyn McpHost>, version: &str) -> Arc<Self> {
        Arc::new(Self {
            ws,
            host,
            version: version.to_string(),
            running: Mutex::new(None),
            session_allowed: Mutex::new(HashSet::new()),
        })
    }

    pub fn is_running(&self) -> bool {
        self.running.lock().unwrap().is_some()
    }

    pub fn socket_path(&self) -> Option<PathBuf> {
        self.running.lock().unwrap().as_ref().map(|r| r.socket_path.clone())
    }

    /// The server listens on a Unix socket; other platforms (Windows) don't
    /// have one yet, so starting reports that instead.
    #[cfg(not(unix))]
    pub async fn start(self: &Arc<Self>, _runtime_path: &Path) -> anyhow::Result<()> {
        anyhow::bail!("the MCP server isn't available on this platform yet (it needs a Unix socket)")
    }

    #[cfg(unix)]
    pub async fn start(self: &Arc<Self>, runtime_path: &Path) -> anyhow::Result<()> {
        if self.is_running() {
            return Ok(());
        }
        let path = socket_path();
        let _ = std::fs::remove_file(&path); // stale socket from a killed instance
        let listener = UnixListener::bind(&path)?;
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
        let project_path = self.ws.lock().await.project.path.clone();
        let runtime = json!({
            "socketPath": path,
            "pid": std::process::id(),
            "projectPath": project_path,
            "startedAt": crate::project::now_iso(),
        });
        if let Err(e) = marina_core::json::write(runtime_path, &runtime) {
            self.host.log(format!("[MCP] failed to write runtime file: {e}"));
        }

        let mut tasks = JoinSet::new();
        let me = self.clone();
        tasks.spawn(async move {
            let mut conns = JoinSet::new();
            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let me = me.clone();
                        conns.spawn(async move { me.serve(stream).await });
                    }
                    Err(e) => {
                        me.host.log(format!("[MCP] socket error: {e}"));
                        break;
                    }
                }
            }
        });
        *self.running.lock().unwrap() = Some(Running { socket_path: path.clone(), runtime_path: runtime_path.to_path_buf(), tasks });
        self.host.log(format!("[MCP] listening on {}", path.display()));
        Ok(())
    }

    pub async fn stop(&self) {
        let Some(mut running) = self.running.lock().unwrap().take() else { return };
        self.session_allowed.lock().unwrap().clear();
        running.tasks.shutdown().await; // drops the listener and every connection
        let _ = std::fs::remove_file(&running.socket_path);
        let _ = std::fs::remove_file(&running.runtime_path);
        self.host.log("[MCP] stopped".into());
    }

    #[cfg(unix)]
    async fn serve(self: Arc<Self>, stream: UnixStream) {
        let (read, mut write) = stream.into_split();
        let mut reader = BufReader::new(read);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match (&mut reader).take(MAX_FRAME_BYTES as u64 + 1).read_until(b'\n', &mut buf).await {
                Ok(0) => break,
                Ok(_) if buf.len() > MAX_FRAME_BYTES => {
                    self.host.log("[MCP] frame too large; closing connection".into());
                    break;
                }
                Ok(_) => {
                    let line = String::from_utf8_lossy(&buf);
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    if let Some(resp) = self.handle_message(line).await {
                        let mut out = resp.to_string();
                        out.push('\n');
                        if let Err(e) = write.write_all(out.as_bytes()).await {
                            self.host.log(format!("[MCP] write failed: {e}"));
                            break;
                        }
                    }
                }
                Err(e) => {
                    self.host.log(format!("[MCP] socket error: {e}"));
                    break;
                }
            }
        }
    }

    /// One JSON-RPC message in, an optional response out.
    pub async fn handle_message(&self, line: &str) -> Option<Value> {
        let err = |id: Value, code: i64, message: &str| Some(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }));
        let Ok(msg) = serde_json::from_str::<Value>(line) else {
            return err(Value::Null, PARSE_ERROR, "Parse error");
        };
        let method = msg.get("method").and_then(Value::as_str);
        if msg.get("jsonrpc").and_then(Value::as_str) != Some("2.0") || method.is_none() {
            if msg.get("result").is_some() || msg.get("error").is_some() {
                return None;
            }
            return err(msg.get("id").cloned().unwrap_or(Value::Null), INVALID_REQUEST, "Invalid Request");
        }
        let id = msg.get("id").cloned().unwrap_or(Value::Null);
        let is_notification = id.is_null();
        let params = msg.get("params").cloned().filter(|p| !p.is_null()).unwrap_or_else(|| json!({}));
        match self.dispatch(method.unwrap(), &params).await {
            Ok(result) if !is_notification => Some(json!({ "jsonrpc": "2.0", "id": id, "result": result })),
            Err(RpcError(code, message)) if !is_notification => err(id, code, &message),
            _ => None,
        }
    }

    async fn dispatch(&self, method: &str, params: &Value) -> Result<Value, RpcError> {
        match method {
            "initialize" => Ok(json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": { "tools": {}, "resources": {}, "prompts": {} },
                "serverInfo": { "name": "noteliner", "version": self.version },
            })),
            "initialized" | "notifications/initialized" => Ok(Value::Null),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tool_definitions() })),
            "tools/call" => self.tools_call(params).await,
            "resources/list" => Ok(self.resources_list().await),
            "resources/read" => self.resources_read(params).await,
            "prompts/list" => Ok(json!({ "prompts": prompt_definitions() })),
            "prompts/get" => self.prompts_get(params).await,
            _ => Err(RpcError(METHOD_NOT_FOUND, format!("Method not found: {method}"))),
        }
    }

    // --- Tools ------------------------------------------------------------------

    async fn tools_call(&self, params: &Value) -> Result<Value, RpcError> {
        let Some(name) = params.get("name").and_then(Value::as_str) else {
            return Err(RpcError(INVALID_PARAMS, "tools/call requires a \"name\" string".into()));
        };
        let args = params.get("arguments").cloned().filter(|a| !a.is_null()).unwrap_or_else(|| json!({}));
        let res = async {
            self.preflight(name, &args).await?;
            self.invoke(name, &args).await
        }
        .await;
        Ok(match res {
            Ok(v) => {
                self.host.log(format!("[MCP] {name} ok"));
                v
            }
            Err(ToolError(msg)) => {
                self.host.log(format!("[MCP] {name} error: {msg}"));
                json!({ "content": [{ "type": "text", "text": msg }], "isError": true })
            }
        })
    }

    async fn preflight(&self, name: &str, args: &Value) -> Result<(), ToolError> {
        let (confirm_writes, disabled) = self.host.prefs();
        if disabled.iter().any(|d| d == name) {
            return Err(tool_err(format!("Tool \"{name}\" is disabled in NoteLiner settings.")));
        }
        if !WRITE_TOOLS.contains(&name) || !confirm_writes || self.session_allowed.lock().unwrap().contains(name) {
            return Ok(());
        }
        let summary = summarize(name, args);
        match self.host.confirm(name, &summary, args).await.as_str() {
            "deny" => Err(tool_err(format!("User denied the \"{name}\" call."))),
            "session" => {
                self.session_allowed.lock().unwrap().insert(name.to_string());
                self.host.log(format!("[MCP] session-trust granted for \"{name}\""));
                Ok(())
            }
            _ => Ok(()),
        }
    }

    async fn invoke(&self, name: &str, args: &Value) -> Result<Value, ToolError> {
        let str_arg = |k: &str| args.get(k).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string);
        let mut ws = self.ws.lock().await;
        if !ws.project.is_open() {
            return Err(tool_err("No project is open in NoteLiner."));
        }
        let resolve = |ws: &crate::workspace::Workspace| -> Result<Value, ToolError> {
            resolve_entry(ws, str_arg("id"), str_arg("name"))
        };
        match name {
            "list_notes" => {
                let notes: Vec<Value> = ws
                    .project
                    .files()
                    .iter()
                    .map(|f| {
                        json!({
                            "id": f["id"], "name": f["name"], "filename": f["filename"],
                            "tags": if f["tags"].is_array() { f["tags"].clone() } else { json!([]) },
                            "parentId": if f["parentId"].is_null() || f["parentId"] == "" { Value::Null } else { f["parentId"].clone() },
                        })
                    })
                    .collect();
                self.host.log(format!("[MCP] list_notes -> {} results", notes.len()));
                Ok(json_result(&json!(notes)))
            }
            "read_note" => {
                let entry = resolve(&ws)?;
                let body = ws.project.read_file(s(&entry["filename"]))?;
                self.host.log(format!("[MCP] read_note id={} bytes={}", s(&entry["id"]), js_len(&body)));
                Ok(text_result(&body))
            }
            "create_note" => {
                let name = args.get("name").and_then(Value::as_str).filter(|n| !n.trim().is_empty());
                let Some(name) = name else { return Err(tool_err("\"name\" is required and must be a non-empty string.")) };
                let body = match args.get("body") {
                    None | Some(Value::Null) => None,
                    Some(Value::String(b)) => Some(b.clone()),
                    Some(_) => return Err(tool_err("\"body\" must be a string when provided.")),
                };
                if body.as_ref().is_some_and(|b| b.len() > MAX_BODY_BYTES) {
                    return Err(tool_err(format!("Body exceeds {}MB limit.", MAX_BODY_BYTES / 1024 / 1024)));
                }
                let tags = if args["tags"].is_array() { args["tags"].clone() } else { json!([]) };
                let entry = ws.project.create_file(name.trim(), &tags, body, None).await?;
                let entry_id = s(&entry["id"]).to_string();
                if let Some(parent) = str_arg("parentId") {
                    if ws.project.find(&parent).is_none() {
                        return Err(tool_err(format!("No parent note with id \"{parent}\".")));
                    }
                    let mut index = ws.project.index.clone().unwrap();
                    if let Some(f) = index["files"].as_array_mut().and_then(|fs| fs.iter_mut().find(|f| f["id"] == entry_id.as_str())) {
                        f["parentId"] = json!(parent);
                    }
                    ws.project.save_index(index).await?;
                }
                ws.rebuild_links();
                self.host.log(format!("[MCP] create_note name=\"{name}\" -> id={entry_id}"));
                Ok(text_result(&format!(
                    "Created note \"{}\" (id: {entry_id}, filename: {})",
                    s(&entry["name"]),
                    s(&entry["filename"])
                )))
            }
            "update_note" => {
                let Some(body) = args.get("body").and_then(Value::as_str) else {
                    return Err(tool_err("\"body\" is required and must be a string."));
                };
                if body.len() > MAX_BODY_BYTES {
                    return Err(tool_err(format!("Body exceeds {}MB limit.", MAX_BODY_BYTES / 1024 / 1024)));
                }
                let entry = resolve(&ws)?;
                ws.project.write_file(s(&entry["filename"]), body).await?;
                ws.scan_links(s(&entry["id"]));
                self.host.log(format!("[MCP] update_note id={} bytes={}", s(&entry["id"]), js_len(body)));
                Ok(text_result(&format!("Updated note \"{}\" ({} chars).", s(&entry["name"]), js_len(body))))
            }
            "delete_note" => {
                let entry = resolve(&ws)?;
                ws.project.delete_file(s(&entry["id"])).await?;
                ws.links.remove_file(s(&entry["id"]));
                ws.rebuild_links();
                self.host.log(format!("[MCP] delete_note id={}", s(&entry["id"])));
                Ok(text_result(&format!("Deleted note \"{}\".", s(&entry["name"]))))
            }
            "rename_note" => {
                let new_name = args.get("newName").and_then(Value::as_str).filter(|n| !n.trim().is_empty());
                let Some(new_name) = new_name else { return Err(tool_err("\"newName\" is required.")) };
                let entry = resolve(&ws)?;
                let updated = ws.project.rename_file(s(&entry["id"]), new_name.trim()).await?.unwrap_or(entry.clone());
                ws.rebuild_links();
                self.host.log(format!("[MCP] rename_note id={} -> \"{new_name}\"", s(&entry["id"])));
                Ok(text_result(&format!("Renamed to \"{}\" (filename: {}).", s(&updated["name"]), s(&updated["filename"]))))
            }
            "set_tags" => {
                let Some(tags) = args.get("tags").and_then(Value::as_array) else {
                    return Err(tool_err("\"tags\" must be an array."));
                };
                let entry = resolve(&ws)?;
                let tags: Vec<Value> = tags.iter().filter(|t| t.is_string()).cloned().collect();
                let mut index = ws.project.index.clone().unwrap();
                if let Some(f) = index["files"].as_array_mut().and_then(|fs| fs.iter_mut().find(|f| f["id"] == entry["id"])) {
                    f["tags"] = json!(tags);
                }
                ws.project.save_index(index).await?;
                let joined = tags.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ");
                self.host.log(format!("[MCP] set_tags id={} count={}", s(&entry["id"]), tags.len()));
                Ok(text_result(&format!("Set tags on \"{}\": [{joined}]", s(&entry["name"]))))
            }
            "search" => {
                let Some(query) = str_arg("query") else { return Err(tool_err("\"query\" is required.")) };
                let case = args.get("caseSensitive").and_then(Value::as_bool).unwrap_or(false);
                let hits = ws.project.search(&query, case);
                self.host.log(format!("[MCP] search \"{query}\" -> {} files", hits.len()));
                Ok(json_result(&json!(hits)))
            }
            "get_backlinks" => {
                let entry = resolve(&ws)?;
                let links = ws.links.backlink_snippets(&ws.project, s(&entry["id"]));
                self.host.log(format!("[MCP] get_backlinks id={} -> {} sources", s(&entry["id"]), links.len()));
                Ok(json_result(&json!(links)))
            }
            "list_attachments" => {
                let entry = resolve(&ws)?;
                let atts = if entry["attachments"].is_array() { entry["attachments"].clone() } else { json!([]) };
                Ok(json_result(&atts))
            }
            "list_tags" => {
                let mut hist: Vec<(String, usize, Vec<Value>)> = Vec::new();
                for f in ws.project.files() {
                    for tag in f["tags"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                        match hist.iter_mut().find(|h| h.0 == tag) {
                            Some(h) => {
                                h.1 += 1;
                                h.2.push(f["id"].clone());
                            }
                            None => hist.push((tag.to_string(), 1, vec![f["id"].clone()])),
                        }
                    }
                }
                hist.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase())).then_with(|| a.0.cmp(&b.0)));
                self.host.log(format!("[MCP] list_tags -> {} tags", hist.len()));
                let tags: Vec<Value> = hist.into_iter().map(|(tag, count, ids)| json!({ "tag": tag, "count": count, "noteIds": ids })).collect();
                Ok(json_result(&json!(tags)))
            }
            "add_attachment" => {
                let Some(filename) = str_arg("filename") else { return Err(tool_err("\"filename\" is required.")) };
                let Some(data) = str_arg("dataBase64") else { return Err(tool_err("\"dataBase64\" is required.")) };
                if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
                    return Err(tool_err("Invalid filename — must not contain path separators."));
                }
                let entry = resolve(&ws)?;
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(data.trim())
                    .map_err(|_| tool_err("Invalid base64 in \"dataBase64\"."))?;
                let att = ws.project.add_attachment(s(&entry["id"]), &bytes, &filename).await?;
                self.host.log(format!("[MCP] add_attachment id={} file=\"{filename}\" bytes={}", s(&entry["id"]), bytes.len()));
                Ok(json_result(&att))
            }
            "remove_attachment" => {
                let Some(att_id) = str_arg("attachmentId") else { return Err(tool_err("\"attachmentId\" is required.")) };
                let entry = resolve(&ws)?;
                let exists = entry["attachments"].as_array().into_iter().flatten().any(|a| a["id"] == att_id.as_str());
                if !exists {
                    return Err(tool_err(format!("No attachment \"{att_id}\" on note \"{}\".", s(&entry["name"]))));
                }
                ws.project.remove_attachment(s(&entry["id"]), &att_id).await?;
                self.host.log(format!("[MCP] remove_attachment id={} attachment={att_id}", s(&entry["id"])));
                Ok(text_result(&format!("Removed attachment {att_id} from \"{}\".", s(&entry["name"]))))
            }
            _ => Err(tool_err(format!("Unknown tool: {name}"))),
        }
    }

    // --- Resources ----------------------------------------------------------------

    async fn resources_list(&self) -> Value {
        let ws = self.ws.lock().await;
        if !ws.project.is_open() {
            return json!({ "resources": [] });
        }
        let mut resources = vec![json!({
            "uri": "noteliner://index",
            "name": "Project index",
            "description": "noteliner.json — id, name, filename, tags, parentId, attachments for every note.",
            "mimeType": "application/json",
        })];
        for f in ws.project.files() {
            resources.push(json!({
                "uri": format!("noteliner://note/{}", s(&f["id"])),
                "name": f["name"],
                "description": format!("Markdown body of \"{}\".", s(&f["name"])),
                "mimeType": "text/markdown",
            }));
        }
        json!({ "resources": resources })
    }

    async fn resources_read(&self, params: &Value) -> Result<Value, RpcError> {
        let Some(uri) = params.get("uri").and_then(Value::as_str).filter(|u| !u.is_empty()) else {
            return Err(RpcError(INVALID_PARAMS, "\"uri\" is required.".into()));
        };
        let ws = self.ws.lock().await;
        if !ws.project.is_open() {
            return Err(tool_err("No project is open in NoteLiner.").into());
        }
        if uri == "noteliner://index" {
            let text = serde_json::to_string_pretty(ws.project.index.as_ref().unwrap()).unwrap_or_default();
            return Ok(json!({ "contents": [{ "uri": uri, "mimeType": "application/json", "text": text }] }));
        }
        if let Some(id) = uri.strip_prefix("noteliner://note/").filter(|i| !i.is_empty() && !i.contains('/')) {
            let entry = ws.project.find(id).ok_or_else(|| tool_err(format!("No note with id \"{id}\".")))?;
            let body = ws.project.read_file(s(&entry["filename"])).map_err(ToolError::from)?;
            return Ok(json!({ "contents": [{ "uri": uri, "mimeType": "text/markdown", "text": body }] }));
        }
        if let Some(enc) = uri.strip_prefix("noteliner://attachment/").filter(|e| !e.is_empty()) {
            let filename = percent_decode(enc);
            if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
                return Err(tool_err("Invalid attachment filename.").into());
            }
            let path = ws.project.attachment_path(&filename).map_err(ToolError::from)?;
            let data = std::fs::read(path).map_err(|_| tool_err("Attachment not found."))?;
            let blob = base64::engine::general_purpose::STANDARD.encode(data);
            return Ok(json!({ "contents": [{ "uri": uri, "mimeType": "application/octet-stream", "blob": blob }] }));
        }
        Err(tool_err(format!("Unknown resource URI: {uri}")).into())
    }

    // --- Prompts ------------------------------------------------------------------

    async fn prompts_get(&self, params: &Value) -> Result<Value, RpcError> {
        let name = params.get("name").and_then(Value::as_str).unwrap_or("");
        let args = params.get("arguments").cloned().filter(|a| a.is_object()).unwrap_or_else(|| json!({}));
        let defs = prompt_definitions();
        let Some(prompt) = defs.iter().find(|p| p["name"] == name) else {
            return Err(RpcError(INVALID_PARAMS, format!("Unknown prompt: {name}")));
        };
        for arg in prompt["arguments"].as_array().into_iter().flatten() {
            let key = s(&arg["name"]);
            let v = &args[key];
            if arg["required"] == true && (v.is_null() || v == "") {
                return Err(RpcError(INVALID_PARAMS, format!("Prompt \"{name}\" requires argument \"{key}\".")));
            }
        }
        let text = self.prompt_text(name, &args).await?;
        self.host.log(format!("[MCP] prompts/get name={name}"));
        Ok(json!({ "description": prompt["description"], "messages": [{ "role": "user", "content": { "type": "text", "text": text } }] }))
    }

    async fn prompt_text(&self, name: &str, args: &Value) -> Result<String, RpcError> {
        let arg = |k: &str| match &args[k] {
            Value::Null => None,
            Value::String(s) if s.is_empty() => None,
            Value::String(s) => Some(s.clone()),
            v => Some(v.to_string()),
        };
        match name {
            "daily_note" => {
                let date = arg("date").unwrap_or_else(|| chrono::Utc::now().format("%Y-%m-%d").to_string());
                let topic = arg("topic").map(|t| format!("\n\nFocus for the day: {t}")).unwrap_or_default();
                Ok(format!(
                    "Draft a NoteLiner daily journal note for {date}.\n\nFormat the note as Markdown with an H1 title `# Daily {date}`, then sections for **Highlights**, **Did**, **Next**, and **Notes**. Keep prose terse — bullets, not paragraphs.{topic}\n\nWhen ready, call the `create_note` tool with name=\"Daily {date}\" and the drafted body."
                ))
            }
            "meeting_note" => {
                let topic = arg("topic").unwrap_or_default();
                let attendees = arg("attendees").map(|a| format!("\n\nAttendees: {a}")).unwrap_or_default();
                Ok(format!(
                    "Draft a NoteLiner meeting note titled \"{topic}\".{attendees}\n\nUse this structure:\n# {topic}\n**Date:** YYYY-MM-DD\n**Attendees:** ...\n\n## Agenda\n- ...\n\n## Discussion\n- ...\n\n## Decisions\n- ...\n\n## Action items\n- [ ] owner — task\n\nWhen ready, call the `create_note` tool with name=\"{topic}\" and the drafted body, tagged with [\"meeting\"]."
                ))
            }
            "summarize_note" | "link_suggestions" => {
                let ws = self.ws.lock().await;
                if !ws.project.is_open() {
                    return Err(tool_err("No project is open in NoteLiner.").into());
                }
                let entry = resolve_by_id_or_name(&ws, arg("note").as_deref())?;
                let body = ws.project.read_file(s(&entry["filename"])).map_err(ToolError::from)?;
                if name == "summarize_note" {
                    Ok(format!(
                        "Summarize the NoteLiner note \"{}\" (id: {}).\n\nProduce a 3-5 sentence summary, followed by a bullet list of the key entities, decisions, or open questions mentioned.\n\n--- BEGIN NOTE BODY ---\n{body}\n--- END NOTE BODY ---",
                        s(&entry["name"]),
                        s(&entry["id"])
                    ))
                } else {
                    let names: Vec<String> = ws
                        .project
                        .files()
                        .iter()
                        .filter(|f| f["id"] != entry["id"])
                        .map(|f| format!("- {}", s(&f["name"])))
                        .collect();
                    Ok(format!(
                        "For the NoteLiner note \"{}\", suggest wikilinks ([[Other Note Name]]) that would be useful to add to its body, drawn from the list of existing notes below.\n\nReturn a short Markdown list. For each suggestion include: the existing note name, the passage in the source that should link to it, and a one-sentence justification. Do not invent notes that aren't on the list.\n\n--- EXISTING NOTES ({}) ---\n{}\n\n--- SOURCE NOTE BODY ---\n{body}",
                        s(&entry["name"]),
                        names.len(),
                        names.join("\n")
                    ))
                }
            }
            _ => Err(RpcError(INVALID_PARAMS, format!("Unknown prompt: {name}"))),
        }
    }
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}

/// JS `string.length` (UTF-16 units), used in log and result text.
fn js_len(s: &str) -> usize {
    s.encode_utf16().count()
}

fn text_result(text: &str) -> Value {
    json!({ "content": [{ "type": "text", "text": text }], "isError": false })
}

fn json_result(v: &Value) -> Value {
    text_result(&serde_json::to_string_pretty(v).unwrap_or_default())
}

fn resolve_entry(ws: &crate::workspace::Workspace, id: Option<String>, name: Option<String>) -> Result<Value, ToolError> {
    if let Some(id) = id {
        return ws.project.find(&id).cloned().ok_or_else(|| tool_err(format!("No note with id \"{id}\".")));
    }
    if let Some(name) = name {
        let target = name.to_lowercase();
        return ws
            .project
            .files()
            .iter()
            .find(|f| s(&f["name"]).to_lowercase() == target)
            .cloned()
            .ok_or_else(|| tool_err(format!("No note named \"{name}\".")));
    }
    Err(tool_err("Provide either \"id\" or \"name\"."))
}

fn resolve_by_id_or_name(ws: &crate::workspace::Workspace, value: Option<&str>) -> Result<Value, ToolError> {
    let Some(value) = value else { return Err(tool_err("Note reference required.")) };
    if let Some(e) = ws.project.find(value) {
        return Ok(e.clone());
    }
    let target = value.to_lowercase();
    ws.project
        .files()
        .iter()
        .find(|f| s(&f["name"]).to_lowercase() == target)
        .cloned()
        .ok_or_else(|| tool_err(format!("No note matching \"{value}\".")))
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

fn summarize(name: &str, args: &Value) -> String {
    let target = || {
        args["id"].as_str().filter(|s| !s.is_empty()).or(args["name"].as_str().filter(|s| !s.is_empty())).unwrap_or("(unspecified)").to_string()
    };
    let list = |v: &Value| v.as_array().into_iter().flatten().map(|t| t.as_str().map(str::to_string).unwrap_or_else(|| t.to_string())).collect::<Vec<_>>().join(", ");
    match name {
        "create_note" => {
            let n = args["name"].as_str().filter(|s| !s.is_empty()).unwrap_or("(unnamed)");
            let tags = args["tags"].as_array().filter(|t| !t.is_empty()).map(|_| format!(" with tags [{}]", list(&args["tags"]))).unwrap_or_default();
            format!("Create note \"{n}\"{tags}")
        }
        "update_note" => format!("Update body of \"{}\" ({} chars)", target(), args["body"].as_str().map(js_len).unwrap_or(0)),
        "delete_note" => format!("Delete note \"{}\"", target()),
        "rename_note" => format!("Rename \"{}\" to \"{}\"", target(), args["newName"].as_str().filter(|s| !s.is_empty()).unwrap_or("(unspecified)")),
        "set_tags" => format!("Set tags on \"{}\" to [{}]", target(), list(&args["tags"])),
        "add_attachment" => format!("Attach \"{}\" to \"{}\"", args["filename"].as_str().filter(|s| !s.is_empty()).unwrap_or("(unspecified)"), target()),
        "remove_attachment" => format!("Remove attachment {} from \"{}\"", args["attachmentId"].as_str().filter(|s| !s.is_empty()).unwrap_or("(unspecified)"), target()),
        _ => name.to_string(),
    }
}

fn id_name_schema(extra: Value) -> Value {
    let mut props = json!({ "id": { "type": "string" }, "name": { "type": "string" } });
    if let (Some(p), Some(e)) = (props.as_object_mut(), extra.as_object()) {
        for (k, v) in e {
            p.insert(k.clone(), v.clone());
        }
    }
    props
}

pub fn tool_definitions() -> Value {
    let obj = |props: Value, required: Option<Value>| {
        let mut o = json!({ "type": "object", "properties": props, "additionalProperties": false });
        if let Some(r) = required {
            o.as_object_mut().unwrap().insert("required".into(), r);
        }
        // Keep `required` before `additionalProperties`, as in the JS source.
        if o.get("required").is_some() {
            let ap = o.as_object_mut().unwrap().remove("additionalProperties").unwrap();
            o.as_object_mut().unwrap().insert("additionalProperties".into(), ap);
        }
        o
    };
    json!([
        { "name": "list_notes", "description": "List every note in the open NoteLiner project. Returns id, name, filename, tags, parentId.",
          "inputSchema": obj(json!({}), None) },
        { "name": "read_note", "description": "Read the markdown body of a note by id or by name.",
          "inputSchema": obj(json!({ "id": { "type": "string", "description": "Note id (preferred)." }, "name": { "type": "string", "description": "Note name (case-insensitive)." } }), None) },
        { "name": "create_note", "description": "Create a new note. Auto-commits to git via the standard project write path.",
          "inputSchema": obj(json!({ "name": { "type": "string" }, "body": { "type": "string", "description": "Markdown body. Optional." }, "tags": { "type": "array", "items": { "type": "string" } }, "parentId": { "type": "string", "description": "Optional parent note id for hierarchical placement." } }), Some(json!(["name"]))) },
        { "name": "update_note", "description": "Replace the markdown body of an existing note. Auto-commits.",
          "inputSchema": obj(id_name_schema(json!({ "body": { "type": "string" } })), Some(json!(["body"]))) },
        { "name": "delete_note", "description": "Delete a note. Children of the deleted note are re-parented to its parent.",
          "inputSchema": obj(id_name_schema(json!({})), None) },
        { "name": "rename_note", "description": "Change the human-visible name of a note. The filename is re-slugged.",
          "inputSchema": obj(json!({ "id": { "type": "string" }, "name": { "type": "string", "description": "Current name (used when id is absent)." }, "newName": { "type": "string" } }), Some(json!(["newName"]))) },
        { "name": "set_tags", "description": "Replace the tag list on a note.",
          "inputSchema": obj(id_name_schema(json!({ "tags": { "type": "array", "items": { "type": "string" } } })), Some(json!(["tags"]))) },
        { "name": "search", "description": "Full-text search across note bodies. Returns hits with line numbers.",
          "inputSchema": obj(json!({ "query": { "type": "string" }, "caseSensitive": { "type": "boolean", "default": false } }), Some(json!(["query"]))) },
        { "name": "get_backlinks", "description": "List notes that link to the given note via wikilink syntax.",
          "inputSchema": obj(id_name_schema(json!({})), None) },
        { "name": "list_attachments", "description": "List attachments on a note.",
          "inputSchema": obj(id_name_schema(json!({})), None) },
        { "name": "list_tags", "description": "List every distinct tag used across the project, with the count of notes carrying each tag.",
          "inputSchema": obj(json!({}), None) },
        { "name": "add_attachment", "description": "Attach a binary file to a note. Data must be base64-encoded. 30MB limit.",
          "inputSchema": obj(id_name_schema(json!({ "filename": { "type": "string", "description": "Original filename (used to derive extension and MIME type)." }, "dataBase64": { "type": "string", "description": "Base64-encoded file contents." } })), Some(json!(["filename", "dataBase64"]))) },
        { "name": "remove_attachment", "description": "Remove an attachment from a note by its attachment id.",
          "inputSchema": obj(json!({ "id": { "type": "string", "description": "Note id (preferred)." }, "name": { "type": "string", "description": "Note name." }, "attachmentId": { "type": "string" } }), Some(json!(["attachmentId"]))) },
    ])
}

pub fn prompt_definitions() -> Vec<Value> {
    vec![
        json!({ "name": "daily_note", "description": "Draft a daily journal note for today (or a given date).", "arguments": [
            { "name": "date", "description": "ISO date like 2026-05-16. Defaults to today.", "required": false },
            { "name": "topic", "description": "Optional focus or theme for the day.", "required": false } ] }),
        json!({ "name": "meeting_note", "description": "Draft a meeting note with attendees, agenda, and action items.", "arguments": [
            { "name": "topic", "description": "Meeting topic or title.", "required": true },
            { "name": "attendees", "description": "Comma-separated list of attendees.", "required": false } ] }),
        json!({ "name": "summarize_note", "description": "Summarize an existing note. Provide its id or name.", "arguments": [
            { "name": "note", "description": "Note id or name (case-insensitive).", "required": true } ] }),
        json!({ "name": "link_suggestions", "description": "Suggest wikilink targets to add to a note, based on its content and the rest of the project.", "arguments": [
            { "name": "note", "description": "Note id or name (case-insensitive).", "required": true } ] }),
    ]
}

/// `noteliner --mcp-bridge`: pump stdio to the running app's socket. The
/// runtime file lives in the app's userData folder
/// (`$NOTELINER_USERDATA` overrides, as with the Node bridge).
pub async fn run_bridge(user_data: &Path) -> i32 {
    let fail = |msg: String, code: i32| {
        eprintln!("noteliner-mcp-bridge: {msg}");
        code
    };
    let dir = std::env::var_os("NOTELINER_USERDATA").map(PathBuf::from).unwrap_or_else(|| user_data.to_path_buf());
    let runtime_path = dir.join("mcp-runtime.json");
    if !runtime_path.exists() {
        return fail(
            format!("NoteLiner is not running, no project is open, or the MCP server is disabled (missing {}).", runtime_path.display()),
            2,
        );
    }
    let runtime: Value = match std::fs::read_to_string(&runtime_path).map_err(|e| e.to_string()).and_then(|r| serde_json::from_str(&r).map_err(|e| e.to_string())) {
        Ok(v) => v,
        Err(e) => return fail(format!("failed to read runtime file: {e}"), 1),
    };
    let Some(sock) = runtime["socketPath"].as_str() else { return fail("runtime file is missing socketPath".into(), 1) };
    #[cfg(not(unix))]
    {
        let _ = sock;
        fail("the MCP bridge isn't available on this platform yet (it needs a Unix socket)".into(), 1)
    }
    #[cfg(unix)]
    {
        let stream = match UnixStream::connect(sock).await {
            Ok(s) => s,
            Err(e) => return fail(format!("connection error: {e}"), 1),
        };
        let (mut rd, mut wr) = stream.into_split();
        let up = async {
            let _ = tokio::io::copy(&mut tokio::io::stdin(), &mut wr).await;
            let _ = wr.shutdown().await;
            // Keep draining responses after stdin closes.
            std::future::pending::<()>().await
        };
        let down = async {
            let _ = tokio::io::copy(&mut rd, &mut tokio::io::stdout()).await;
        };
        tokio::select! {
            _ = up => {}
            _ = down => {}
        }
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::NoteGit;
    use crate::project::Project;
    use crate::workspace::Workspace;

    struct TestHost;
    impl McpHost for TestHost {
        fn log(&self, _msg: String) {}
        fn prefs(&self) -> (bool, Vec<String>) {
            (false, vec!["list_attachments".into()])
        }
        fn confirm(&self, _: &str, _: &str, _: &Value) -> ConfirmFuture {
            Box::pin(async { "allow".to_string() })
        }
    }

    async fn rpc(server: &McpServer, msg: Value) -> Value {
        server.handle_message(&msg.to_string()).await.unwrap_or(Value::Null)
    }

    #[tokio::test]
    async fn protocol_and_tools() {
        let tmp = tempfile::tempdir().unwrap();
        let mut p = Project::new(NoteGit::new(Arc::new(|_| {})));
        p.git.git.init(tmp.path()).await.unwrap();
        p.path = Some(tmp.path().to_path_buf());
        p.set_git_config("T", "t@x").await.unwrap();
        p.open(tmp.path()).await.unwrap();
        let ws = Workspace::new(p);
        let server = McpServer::new(ws.clone(), Arc::new(TestHost), "9.9.9");

        let init = rpc(&server, json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} })).await;
        assert_eq!(init["result"]["serverInfo"]["version"], "9.9.9");
        assert!(server.handle_message(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).await.is_none());
        assert_eq!(rpc(&server, json!({ "jsonrpc": "2.0", "id": 2, "method": "nope" })).await["error"]["code"], METHOD_NOT_FOUND);
        assert_eq!(server.handle_message("{bad").await.unwrap()["error"]["code"], PARSE_ERROR);
        assert_eq!(rpc(&server, json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/list" })).await["result"]["tools"].as_array().unwrap().len(), 13);

        let call = |name: &str, args: Value| json!({ "jsonrpc": "2.0", "id": 9, "method": "tools/call", "params": { "name": name, "arguments": args } });
        let created = rpc(&server, call("create_note", json!({ "name": "Alpha", "body": "see [[Beta]]", "tags": ["t1"] }))).await;
        assert_eq!(created["result"]["isError"], false, "{created}");
        rpc(&server, call("create_note", json!({ "name": "Beta", "tags": ["t1", "t2"] }))).await;
        let back = rpc(&server, call("get_backlinks", json!({ "name": "beta" }))).await;
        assert!(back["result"]["content"][0]["text"].as_str().unwrap().contains("Alpha"));
        let tags = rpc(&server, call("list_tags", json!({}))).await;
        let tags: Value = serde_json::from_str(tags["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(tags[0], json!({ "tag": "t1", "count": 2, "noteIds": tags[0]["noteIds"] }));
        let disabled = rpc(&server, call("list_attachments", json!({ "name": "Alpha" }))).await;
        assert_eq!(disabled["result"]["isError"], true);
        let missing = rpc(&server, call("read_note", json!({ "name": "Gamma" }))).await;
        assert_eq!(missing["result"]["content"][0]["text"], "No note named \"Gamma\".");
        let read = rpc(&server, call("read_note", json!({ "name": "alpha" }))).await;
        assert_eq!(read["result"]["content"][0]["text"], "see [[Beta]]\n");

        let res = rpc(&server, json!({ "jsonrpc": "2.0", "id": 5, "method": "resources/list" })).await;
        assert_eq!(res["result"]["resources"].as_array().unwrap().len(), 3);
        let prompt = rpc(&server, json!({ "jsonrpc": "2.0", "id": 6, "method": "prompts/get", "params": { "name": "summarize_note", "arguments": { "note": "Alpha" } } })).await;
        assert!(prompt["result"]["messages"][0]["content"]["text"].as_str().unwrap().contains("--- BEGIN NOTE BODY ---\nsee [[Beta]]"));
        let bad = rpc(&server, json!({ "jsonrpc": "2.0", "id": 7, "method": "prompts/get", "params": { "name": "meeting_note" } })).await;
        assert_eq!(bad["error"]["code"], INVALID_PARAMS);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn socket_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let ws = Workspace::new(Project::new(NoteGit::new(Arc::new(|_| {}))));
        let server = McpServer::new(ws, Arc::new(TestHost), "1");
        let runtime = tmp.path().join("mcp-runtime.json");
        server.start(&runtime).await.unwrap();
        let rt: Value = serde_json::from_str(&std::fs::read_to_string(&runtime).unwrap()).unwrap();
        let mut s = UnixStream::connect(rt["socketPath"].as_str().unwrap()).await.unwrap();
        s.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n").await.unwrap();
        let mut line = String::new();
        BufReader::new(&mut s).read_line(&mut line).await.unwrap();
        assert_eq!(serde_json::from_str::<Value>(&line).unwrap(), json!({ "jsonrpc": "2.0", "id": 1, "result": {} }));
        server.stop().await;
        assert!(!runtime.exists());
    }
}
