//! NoteLiner's git semantics (`apps/noteliner/src/main/git-service.js`) on top of
//! `laankit_core::git`: every change commits immediately, pushes are
//! debounced, and failures are logged rather than thrown where the
//! Electron build swallowed them.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use laankit_core::debounce::Debouncer;
use laankit_core::git::{Git, GitError, LogEntry, LogEvent, SyncStatus};

pub type Log = Arc<dyn Fn(String) + Send + Sync>;

/// `git commit` failed because no identity is configured — the renderer
/// shows the project settings dialog for this.
#[derive(Debug)]
pub struct GitConfigRequired;

impl std::fmt::Display for GitConfigRequired {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("git_config_required")
    }
}

impl std::error::Error for GitConfigRequired {}

pub const PUSH_DEBOUNCE: Duration = Duration::from_secs(30);

#[derive(Clone)]
pub struct NoteGit {
    pub git: Git,
    log: Log,
    push_timer: Arc<Debouncer>,
    push_dir: Arc<Mutex<Option<PathBuf>>>,
}

impl NoteGit {
    pub fn new(log: Log) -> Self {
        let sink = log.clone();
        let git = Git::new().with_log(Arc::new(move |ev| match ev {
            LogEvent::CmdStart { command } => sink(format!("> {command}")),
            LogEvent::CmdOk { output, stderr, .. } => {
                if !output.is_empty() {
                    sink(output);
                }
                if !stderr.is_empty() {
                    sink(stderr);
                }
            }
            LogEvent::CmdError {
                command,
                stdout,
                stderr,
                ..
            } => {
                if !stdout.is_empty() {
                    sink(stdout);
                }
                if !stderr.is_empty() {
                    sink(stderr.clone());
                }
                sink(format!("Error: Command failed: {command}\n{stderr}"));
            }
        }));
        Self {
            git,
            log,
            push_timer: Arc::new(Debouncer::new()),
            push_dir: Arc::new(Mutex::new(None)),
        }
    }

    pub fn log(&self, msg: impl Into<String>) {
        (self.log)(msg.into())
    }

    pub async fn is_repo(&self, dir: &Path) -> bool {
        self.git.is_inside_work_tree(dir).await
    }

    /// Stage + commit. `Ok(false)` when there was nothing to commit.
    pub async fn commit(&self, dir: &Path, message: &str) -> anyhow::Result<bool> {
        self.git.exec(&["add", "-A"], dir).await?;
        match self.git.exec(&["commit", "-m", message], dir).await {
            Ok(_) => Ok(true),
            Err(e) if e.mentions("nothing to commit") => {
                self.log("Nothing to commit.");
                Ok(false)
            }
            Err(e) if is_identity_error(&e) => Err(GitConfigRequired.into()),
            Err(e) => Err(e.into()),
        }
    }

    /// `git push`; failures are logged, not raised.
    pub async fn push(&self, dir: &Path) -> Option<String> {
        match self.git.push(dir).await {
            Ok(out) => Some(out),
            Err(e) => {
                self.log(format!("Push failed: {e}"));
                None
            }
        }
    }

    pub async fn pull(&self, dir: &Path) -> Option<String> {
        match self.git.pull(dir).await {
            Ok(out) => Some(out),
            Err(e) => {
                self.log(format!("Pull failed: {e}"));
                None
            }
        }
    }

    pub async fn pull_rebase(&self, dir: &Path) -> Option<String> {
        match self.git.pull_rebase(dir).await {
            Ok(out) => Some(out),
            Err(e) => {
                self.log(format!("Pull --rebase failed: {e}"));
                None
            }
        }
    }

    /// Push after 30 s of quiet.
    pub fn schedule_push(&self, dir: &Path) {
        *self.push_dir.lock().unwrap() = Some(dir.to_path_buf());
        let me = self.clone();
        let dir = dir.to_path_buf();
        self.push_timer.schedule(PUSH_DEBOUNCE, move || async move {
            if me.git.has_remote(&dir).await {
                me.push(&dir).await;
            }
        });
    }

    /// Push now if a push is pending (closing a project).
    pub async fn flush_push(&self, dir: &Path) {
        self.push_timer.cancel();
        if self.git.has_remote(dir).await {
            self.push(dir).await;
        }
    }

    pub async fn check_config(&self, dir: &Path) -> (Option<String>, Option<String>) {
        (
            self.git.get_config(dir, "user.name").await,
            self.git.get_config(dir, "user.email").await,
        )
    }

    pub async fn sync_status(&self, dir: &Path) -> SyncStatus {
        self.git.sync_status(dir).await
    }

    pub async fn file_log(&self, dir: &Path, file: &str) -> Vec<LogEntry> {
        self.git.file_log(dir, file).await
    }
}

fn is_identity_error(e: &GitError) -> bool {
    e.mentions("unable to auto-detect email") || e.mentions("user.name") || e.mentions("user.email")
}
