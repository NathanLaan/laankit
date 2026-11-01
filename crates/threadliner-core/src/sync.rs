//! Auto-sync engine. Port of `apps/threadliner/src/main/sync-manager.js`: every change is
//! committed immediately; push (after a rebase-pull) runs on a debounce.
//! All git work is serialised through one queue.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use laankit_core::debounce::Debouncer;
use laankit_core::git::{Git, LogEvent};
use serde::Serialize;
use serde_json::{json, Value};

const MAX_LOG_ENTRIES: usize = 200;
/// Plumbing commands kept out of the log.
const FILTERED_COMMANDS: [&str; 2] = ["config user.", "rev-parse --git-dir"];

#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    pub id: u64,
    pub timestamp: i64,
    pub level: &'static str,
    pub message: String,
    pub detail: Option<String>,
}

struct State {
    data_dir: Option<PathBuf>,
    wait: Duration,
    status: &'static str,
    last_sync_time: Option<String>,
    last_error: Option<String>,
    log: Vec<LogEntry>,
    log_id: u64,
    timer_started: Option<Instant>,
}

impl State {
    fn add_log(&mut self, level: &'static str, message: String) {
        self.log_id += 1;
        self.log.push(LogEntry {
            id: self.log_id,
            timestamp: chrono::Utc::now().timestamp_millis(),
            level,
            message,
            detail: None,
        });
        if self.log.len() > MAX_LOG_ENTRIES {
            let excess = self.log.len() - MAX_LOG_ENTRIES;
            self.log.drain(..excess);
        }
    }
}

pub struct SyncManager {
    state: Arc<Mutex<State>>,
    queue: tokio::sync::Mutex<()>,
    push_timer: Debouncer,
    pub git: Git,
}

impl SyncManager {
    pub fn new(identity: (&str, &str)) -> Arc<Self> {
        let state = Arc::new(Mutex::new(State {
            data_dir: None,
            wait: Duration::from_secs(10),
            status: "idle",
            last_sync_time: None,
            last_error: None,
            log: Vec::new(),
            log_id: 0,
            timer_started: None,
        }));
        let sink_state = state.clone();
        let git = Git::new()
            .with_default_identity(identity.0, identity.1)
            .with_log(Arc::new(move |ev| {
                let command = match &ev {
                    LogEvent::CmdStart { command }
                    | LogEvent::CmdOk { command, .. }
                    | LogEvent::CmdError { command, .. } => command,
                };
                if FILTERED_COMMANDS.iter().any(|f| command.contains(f)) {
                    return;
                }
                let mut st = sink_state.lock().unwrap();
                match ev {
                    LogEvent::CmdStart { command } => st.add_log("info", format!("$ {command}")),
                    LogEvent::CmdOk { output, .. } if !output.is_empty() => {
                        st.add_log("info", output)
                    }
                    LogEvent::CmdOk { .. } => {}
                    LogEvent::CmdError { message, .. } => st.add_log("error", message),
                }
            }));
        Arc::new(Self {
            state,
            queue: tokio::sync::Mutex::new(()),
            push_timer: Debouncer::new(),
            git,
        })
    }

    pub fn init(&self, dir: PathBuf, wait_seconds: u64) {
        self.push_timer.cancel();
        let mut st = self.state.lock().unwrap();
        st.data_dir = Some(dir);
        if wait_seconds > 0 {
            st.wait = Duration::from_secs(wait_seconds);
        }
        st.status = "idle";
        st.last_error = None;
        st.log.clear();
        st.log_id = 0;
        st.timer_started = None;
    }

    pub fn data_dir(&self) -> Option<PathBuf> {
        self.state.lock().unwrap().data_dir.clone()
    }

    pub fn update_wait_time(&self, seconds: u64) {
        self.state.lock().unwrap().wait = Duration::from_secs(seconds);
    }

    fn set_status(&self, status: &'static str, error: Option<String>) {
        let mut st = self.state.lock().unwrap();
        st.status = status;
        st.last_error = error.clone();
        if let Some(e) = error {
            st.add_log("error", e);
        } else {
            let msg = match status {
                "committing" => "Committing changes...",
                "pulling" => "Pulling from remote...",
                "pushing" => "Pushing to remote...",
                "waiting" => "Waiting to push...",
                "idle" => "Sync complete",
                _ => return,
            };
            st.add_log("info", msg.into());
        }
    }

    pub fn status(&self, since_log_id: Option<u64>) -> Value {
        let st = self.state.lock().unwrap();
        let remaining = if self.push_timer.is_pending() {
            st.timer_started
                .map(|t| st.wait.saturating_sub(t.elapsed()).as_millis() as u64)
        } else {
            None
        };
        let entries: Vec<&LogEntry> = match since_log_id {
            Some(id) if id > 0 => st.log.iter().filter(|e| e.id > id).collect(),
            _ => Vec::new(),
        };
        json!({
            "status": st.status,
            "lastSyncTime": st.last_sync_time,
            "lastError": st.last_error,
            "waitTimeMs": st.wait.as_millis() as u64,
            "remainingMs": remaining,
            "logEntries": entries,
        })
    }

    pub fn full_log(&self) -> Vec<LogEntry> {
        self.state.lock().unwrap().log.clone()
    }

    /// Commit now; (re)start the push countdown if anything was committed.
    pub async fn notify_change(self: &Arc<Self>, message: &str) {
        let Some(dir) = self.data_dir() else { return };
        let _q = self.queue.lock().await;
        self.set_status("committing", None);
        match self.git.commit_all(&dir, message).await {
            Ok(false) => self.set_status("idle", None),
            Ok(true) => self.schedule_push(),
            Err(e) => self.set_status("error", Some(format!("Commit failed: {e}"))),
        }
    }

    fn schedule_push(self: &Arc<Self>) {
        let wait = {
            let mut st = self.state.lock().unwrap();
            st.timer_started = Some(Instant::now());
            st.wait
        };
        self.set_status("waiting", None);
        let me = self.clone();
        self.push_timer.schedule(wait, move || async move {
            me.state.lock().unwrap().timer_started = None;
            let _q = me.queue.lock().await;
            me.execute_push().await;
        });
    }

    /// Caller holds the queue.
    async fn execute_push(&self) {
        let Some(dir) = self.data_dir() else { return };
        self.set_status("committing", None);
        let _ = self.git.commit_all(&dir, "Auto-commit before sync").await;

        self.set_status("pulling", None);
        let pulled = self.git.sync_pull(&dir).await;
        if !pulled.success {
            self.set_status(
                "error",
                Some(format!("Pull failed: {}", pulled.error.unwrap_or_default())),
            );
            return;
        }
        self.set_status("pushing", None);
        let pushed = self.git.sync_push(&dir).await;
        if !pushed.success {
            self.set_status(
                "error",
                Some(format!("Push failed: {}", pushed.error.unwrap_or_default())),
            );
            return;
        }
        self.state.lock().unwrap().last_sync_time =
            Some(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
        self.set_status("idle", None);
    }

    pub async fn force_push(&self) {
        self.push_timer.cancel();
        self.state.lock().unwrap().timer_started = None;
        let Some(dir) = self.data_dir() else { return };
        let _q = self.queue.lock().await;
        self.set_status("committing", None);
        let _ = self.git.commit_all(&dir, "Manual sync").await;
        self.execute_push().await;
    }

    pub async fn force_pull(&self) -> Value {
        let Some(dir) = self.data_dir() else {
            return json!({ "success": false, "error": "Not initialized" });
        };
        let _q = self.queue.lock().await;
        self.set_status("pulling", None);
        let pulled = self.git.sync_pull(&dir).await;
        if !pulled.success {
            let err = pulled.error.unwrap_or_default();
            self.set_status("error", Some(format!("Pull failed: {err}")));
            return json!({ "success": false, "error": err });
        }
        self.set_status("idle", None);
        json!({ "success": true })
    }
}
