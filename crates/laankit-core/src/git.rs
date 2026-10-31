//! Git operations via the system `git` binary, merged from the three
//! Electron implementations (`noteliner/src/main/git-service.js` and the
//! identical `threadliner`/`pageliner` `git-sync.js`).

use std::fmt;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tokio::process::Command;

const TIMEOUT: Duration = Duration::from_secs(60);

/// One git invocation, reported to the app's log sink.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum LogEvent {
    CmdStart {
        command: String,
    },
    CmdOk {
        command: String,
        output: String,
        stderr: String,
    },
    CmdError {
        command: String,
        message: String,
        stdout: String,
        stderr: String,
    },
}

pub type LogSink = Arc<dyn Fn(LogEvent) + Send + Sync>;

#[derive(Debug, Clone)]
pub struct GitError {
    /// Human-readable summary: stderr, else stdout, else the spawn error.
    pub message: String,
    pub stdout: String,
    pub stderr: String,
}

impl fmt::Display for GitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for GitError {}

impl GitError {
    /// True if either stream mentions `needle` (Node's error.message carried
    /// stderr; some git messages such as "nothing to commit" go to stdout).
    pub fn mentions(&self, needle: &str) -> bool {
        self.message.contains(needle)
            || self.stdout.contains(needle)
            || self.stderr.contains(needle)
    }
}

pub type GitResult<T> = Result<T, GitError>;

/// `{ success, skipped?, error? }` — the result shape the renderers expect
/// from pull/push.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct OpResult {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skipped: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl OpResult {
    pub fn ok() -> Self {
        Self {
            success: true,
            skipped: None,
            error: None,
        }
    }
    pub fn skipped() -> Self {
        Self {
            success: true,
            skipped: Some(true),
            error: None,
        }
    }
    pub fn err(e: impl ToString) -> Self {
        Self {
            success: false,
            skipped: None,
            error: Some(e.to_string()),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum SyncStatus {
    Synced,
    NoUpstream,
    NoRepo,
    Ahead { count: u64 },
    Behind { count: u64 },
    Diverged { ahead: u64, behind: u64 },
    Error { message: String },
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LogEntry {
    pub hash: String,
    pub date: String,
    pub author: String,
    pub message: String,
}

#[derive(Clone, Default)]
pub struct Git {
    log: Option<LogSink>,
    /// Identity applied by `configure_user` when the repo has none
    /// (ThreadLiner/PageLiner set e.g. "PageLiner" / "pageliner@localhost").
    default_identity: Option<(String, String)>,
}

impl Git {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_log(mut self, sink: LogSink) -> Self {
        self.log = Some(sink);
        self
    }

    pub fn with_default_identity(mut self, name: &str, email: &str) -> Self {
        self.default_identity = Some((name.to_string(), email.to_string()));
        self
    }

    pub fn set_log(&mut self, sink: Option<LogSink>) {
        self.log = sink;
    }

    fn emit(&self, ev: LogEvent) {
        if let Some(log) = &self.log {
            log(ev);
        }
    }

    /// Run `git <args>` in `cwd`; resolves to trimmed stdout.
    pub async fn exec(&self, args: &[&str], cwd: &Path) -> GitResult<String> {
        let command = format!("git {}", args.join(" "));
        self.emit(LogEvent::CmdStart {
            command: command.clone(),
        });

        let child = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("GIT_TERMINAL_PROMPT", "0")
            .kill_on_drop(true)
            .spawn();

        let fail = |message: String, stdout: String, stderr: String| {
            self.emit(LogEvent::CmdError {
                command: command.clone(),
                message: message.clone(),
                stdout: stdout.clone(),
                stderr: stderr.clone(),
            });
            GitError {
                message,
                stdout,
                stderr,
            }
        };

        let child = match child {
            Ok(c) => c,
            Err(e) => return Err(fail(e.to_string(), String::new(), String::new())),
        };

        let out = match tokio::time::timeout(TIMEOUT, child.wait_with_output()).await {
            Ok(Ok(out)) => out,
            Ok(Err(e)) => return Err(fail(e.to_string(), String::new(), String::new())),
            Err(_) => {
                return Err(fail(
                    "Process killed (timeout)".into(),
                    String::new(),
                    String::new(),
                ))
            }
        };

        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        if out.status.success() {
            self.emit(LogEvent::CmdOk {
                command,
                output: stdout.clone(),
                stderr,
            });
            Ok(stdout)
        } else {
            let message = if !stderr.is_empty() {
                stderr.clone()
            } else if !stdout.is_empty() {
                stdout.clone()
            } else {
                format!("Command failed: {command} ({})", out.status)
            };
            Err(fail(message, stdout, stderr))
        }
    }

    async fn ok(&self, args: &[&str], cwd: &Path) -> bool {
        self.exec(args, cwd).await.is_ok()
    }

    // --- Repository ---------------------------------------------------------

    pub async fn is_installed(&self) -> bool {
        self.ok(&["--version"], &std::env::temp_dir()).await
    }

    pub async fn is_repo(&self, dir: &Path) -> bool {
        dir.is_dir() && self.ok(&["rev-parse", "--git-dir"], dir).await
    }

    pub async fn is_inside_work_tree(&self, dir: &Path) -> bool {
        dir.is_dir() && self.ok(&["rev-parse", "--is-inside-work-tree"], dir).await
    }

    pub async fn init(&self, dir: &Path) -> GitResult<String> {
        self.exec(&["init"], dir).await
    }

    /// `git init` + identity + optional origin + an empty initial commit so
    /// there is a branch to push (ThreadLiner/PageLiner `initRepo`).
    pub async fn init_repo(&self, dir: &Path, remote_url: Option<&str>) -> GitResult<()> {
        self.init(dir).await?;
        self.configure_user(dir).await?;
        if let Some(url) = remote_url.filter(|u| !u.is_empty()) {
            self.exec(&["remote", "add", "origin", url], dir).await?;
        }
        self.exec(&["commit", "--allow-empty", "-m", "Initial commit"], dir)
            .await?;
        Ok(())
    }

    /// Clone into `dir` itself (`git clone <url> .`); `dir` must be empty.
    pub async fn clone_here(&self, remote_url: &str, dir: &Path) -> GitResult<String> {
        self.exec(&["clone", remote_url, "."], dir).await
    }

    /// Clone into `local_dir`, creating it under its parent.
    pub async fn clone_to(&self, remote_url: &str, local_dir: &Path) -> GitResult<()> {
        let parent = local_dir.parent().unwrap_or(Path::new("/"));
        let name = local_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(".");
        self.exec(&["clone", remote_url, name], parent).await?;
        self.configure_user(local_dir).await
    }

    /// Set the default identity locally when the repo has none.
    pub async fn configure_user(&self, dir: &Path) -> GitResult<()> {
        let Some((name, email)) = self.default_identity.clone() else {
            return Ok(());
        };
        if self.exec(&["config", "user.name"], dir).await.is_err() {
            self.exec(&["config", "user.name", &name], dir).await?;
        }
        if self.exec(&["config", "user.email"], dir).await.is_err() {
            self.exec(&["config", "user.email", &email], dir).await?;
        }
        Ok(())
    }

    /// Local value first, then global.
    pub async fn get_config(&self, dir: &Path, key: &str) -> Option<String> {
        if let Ok(v) = self.exec(&["config", "--local", key], dir).await {
            return Some(v);
        }
        self.exec(&["config", key], dir).await.ok()
    }

    pub async fn set_config(&self, dir: &Path, key: &str, value: &str) -> GitResult<String> {
        self.exec(&["config", "--local", key, value], dir).await
    }

    // --- Commit -------------------------------------------------------------

    /// Stage everything and commit if anything is staged. Returns whether a
    /// commit was made.
    pub async fn commit_all(&self, dir: &Path, message: &str) -> GitResult<bool> {
        self.exec(&["add", "-A"], dir).await?;
        if self.ok(&["diff", "--cached", "--quiet"], dir).await {
            return Ok(false);
        }
        self.exec(&["commit", "-m", message], dir).await?;
        Ok(true)
    }

    pub async fn get_status(&self, dir: &Path) -> GitResult<String> {
        self.exec(&["status", "--porcelain"], dir).await
    }

    // --- Remote -------------------------------------------------------------

    pub async fn has_remote(&self, dir: &Path) -> bool {
        matches!(self.exec(&["remote"], dir).await, Ok(s) if !s.is_empty())
    }

    pub async fn get_remote_url(&self, dir: &Path) -> Option<String> {
        self.exec(&["remote", "get-url", "origin"], dir).await.ok()
    }

    pub async fn set_remote_url(&self, dir: &Path, url: &str) -> GitResult<String> {
        if self.has_remote(dir).await {
            self.exec(&["remote", "set-url", "origin", url], dir).await
        } else {
            self.exec(&["remote", "add", "origin", url], dir).await
        }
    }

    pub async fn remove_remote(&self, dir: &Path) -> GitResult<String> {
        self.exec(&["remote", "remove", "origin"], dir).await
    }

    /// `rev-parse --abbrev-ref HEAD`.
    pub async fn get_branch(&self, dir: &Path) -> Option<String> {
        self.exec(&["rev-parse", "--abbrev-ref", "HEAD"], dir)
            .await
            .ok()
    }

    /// `branch --show-current` (empty on a detached HEAD).
    pub async fn current_branch(&self, dir: &Path) -> Option<String> {
        self.exec(&["branch", "--show-current"], dir).await.ok()
    }

    // --- Pull / push ----------------------------------------------------------

    /// Fetch, then rebase onto origin/main (or origin/master) only when the
    /// remote has commits we lack. Aborts a failed rebase so the repo stays
    /// clean. ThreadLiner/PageLiner `pull`.
    pub async fn sync_pull(&self, dir: &Path) -> OpResult {
        if self.get_remote_url(dir).await.is_none() {
            return OpResult::skipped();
        }
        let res: GitResult<OpResult> = async {
            self.exec(&["fetch", "origin"], dir).await?;
            let remote_branch = if self
                .ok(&["rev-parse", "--verify", "origin/main"], dir)
                .await
            {
                "origin/main"
            } else if self
                .ok(&["rev-parse", "--verify", "origin/master"], dir)
                .await
            {
                "origin/master"
            } else {
                return Ok(OpResult::skipped());
            };
            let local = self.exec(&["rev-parse", "HEAD"], dir).await?;
            let remote = self.exec(&["rev-parse", remote_branch], dir).await?;
            if local == remote
                || self
                    .ok(&["merge-base", "--is-ancestor", remote_branch, "HEAD"], dir)
                    .await
            {
                return Ok(OpResult::ok());
            }
            self.exec(&["rebase", remote_branch], dir).await?;
            Ok(OpResult::ok())
        }
        .await;
        match res {
            Ok(r) => r,
            Err(e) => {
                let _ = self.exec(&["rebase", "--abort"], dir).await;
                OpResult::err(e)
            }
        }
    }

    /// Push the current branch with `-u`. ThreadLiner/PageLiner `push`.
    pub async fn sync_push(&self, dir: &Path) -> OpResult {
        if self.get_remote_url(dir).await.is_none() {
            return OpResult::skipped();
        }
        let res = async {
            let branch = self
                .exec(&["rev-parse", "--abbrev-ref", "HEAD"], dir)
                .await?;
            self.exec(&["push", "-u", "origin", &branch], dir).await
        }
        .await;
        match res {
            Ok(_) => OpResult::ok(),
            Err(e) => OpResult::err(e),
        }
    }

    pub async fn pull(&self, dir: &Path) -> GitResult<String> {
        self.exec(&["pull"], dir).await
    }

    pub async fn pull_rebase(&self, dir: &Path) -> GitResult<String> {
        self.exec(&["pull", "--rebase"], dir).await
    }

    /// Plain `git pull`, skipped without a remote.
    pub async fn pull_merge(&self, dir: &Path) -> OpResult {
        if self.get_remote_url(dir).await.is_none() {
            return OpResult::skipped();
        }
        match self.pull(dir).await {
            Ok(_) => OpResult::ok(),
            Err(e) => OpResult::err(e),
        }
    }

    pub async fn push(&self, dir: &Path) -> GitResult<String> {
        self.exec(&["push"], dir).await
    }

    pub async fn push_upstream(&self, dir: &Path, branch: &str) -> GitResult<String> {
        self.exec(&["push", "-u", "origin", branch], dir).await
    }

    /// Fetch, then compare HEAD with its upstream.
    pub async fn sync_status(&self, dir: &Path) -> SyncStatus {
        if let Err(e) = self.exec(&["fetch", "origin"], dir).await {
            return SyncStatus::Error {
                message: format!("Fetch failed: {e}"),
            };
        }
        let res: GitResult<SyncStatus> = async {
            let local = self.exec(&["rev-parse", "HEAD"], dir).await?;
            let Ok(remote) = self.exec(&["rev-parse", "@{u}"], dir).await else {
                return Ok(SyncStatus::NoUpstream);
            };
            if local == remote {
                return Ok(SyncStatus::Synced);
            }
            let count = |range: &'static str| async move {
                self.exec(&["rev-list", "--count", range], dir)
                    .await
                    .map(|s| s.parse::<u64>().unwrap_or(0))
            };
            let base = self.exec(&["merge-base", "HEAD", "@{u}"], dir).await?;
            if base == remote {
                return Ok(SyncStatus::Ahead {
                    count: count("@{u}..HEAD").await?,
                });
            }
            if base == local {
                return Ok(SyncStatus::Behind {
                    count: count("HEAD..@{u}").await?,
                });
            }
            Ok(SyncStatus::Diverged {
                ahead: count("@{u}..HEAD").await?,
                behind: count("HEAD..@{u}").await?,
            })
        }
        .await;
        res.unwrap_or_else(|e| SyncStatus::Error { message: e.message })
    }

    /// Fetch and hard-reset to `origin/<branch>` (current branch, else main).
    pub async fn reset_to_remote(&self, dir: &Path, branch: Option<&str>) -> GitResult<()> {
        let target = match branch.filter(|b| !b.is_empty()) {
            Some(b) => b.to_string(),
            None => self.get_branch(dir).await.unwrap_or_else(|| "main".into()),
        };
        self.exec(&["fetch", "origin"], dir).await?;
        self.exec(&["reset", "--hard", &format!("origin/{target}")], dir)
            .await?;
        Ok(())
    }

    // --- History ------------------------------------------------------------

    pub async fn file_log(&self, dir: &Path, file: &str) -> Vec<LogEntry> {
        let Ok(raw) = self
            .exec(
                &[
                    "log",
                    "--follow",
                    "--pretty=format:%H|%ai|%an|%s",
                    "--",
                    file,
                ],
                dir,
            )
            .await
        else {
            return Vec::new();
        };
        raw.lines()
            .filter(|l| !l.is_empty())
            .map(|line| {
                let mut parts = line.splitn(4, '|');
                LogEntry {
                    hash: parts.next().unwrap_or_default().to_string(),
                    date: parts.next().unwrap_or_default().to_string(),
                    author: parts.next().unwrap_or_default().to_string(),
                    message: parts.next().unwrap_or_default().to_string(),
                }
            })
            .collect()
    }

    pub async fn file_at_commit(&self, dir: &Path, commit: &str, file: &str) -> Option<String> {
        self.exec(&["show", &format!("{commit}:{file}")], dir)
            .await
            .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn git() -> Git {
        Git::new().with_default_identity("Test", "test@localhost")
    }

    #[tokio::test]
    async fn init_commit_and_status() {
        let dir = tempfile::tempdir().unwrap();
        let g = git();
        assert!(!g.is_repo(dir.path()).await);
        g.init_repo(dir.path(), None).await.unwrap();
        assert!(g.is_repo(dir.path()).await);
        assert!(!g.commit_all(dir.path(), "noop").await.unwrap());
        fs::write(dir.path().join("a.txt"), "hi").unwrap();
        assert!(g.commit_all(dir.path(), "add a").await.unwrap());
        let log = g.file_log(dir.path(), "a.txt").await;
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].message, "add a");
        let head = log[0].hash.clone();
        assert_eq!(
            g.file_at_commit(dir.path(), &head, "a.txt")
                .await
                .as_deref(),
            Some("hi")
        );
    }

    #[tokio::test]
    async fn pull_and_push_skip_without_remote() {
        let dir = tempfile::tempdir().unwrap();
        let g = git();
        g.init_repo(dir.path(), None).await.unwrap();
        assert_eq!(g.sync_pull(dir.path()).await, OpResult::skipped());
        assert_eq!(g.sync_push(dir.path()).await, OpResult::skipped());
    }

    #[tokio::test]
    async fn sync_round_trip_through_bare_remote() {
        let root = tempfile::tempdir().unwrap();
        let bare = root.path().join("remote.git");
        fs::create_dir(&bare).unwrap();
        let g = git();
        g.exec(&["init", "--bare", "-b", "main"], &bare)
            .await
            .unwrap();
        let url = bare.to_str().unwrap();

        let a = root.path().join("a");
        fs::create_dir(&a).unwrap();
        g.exec(&["init", "-b", "main"], &a).await.unwrap();
        g.configure_user(&a).await.unwrap();
        g.set_remote_url(&a, url).await.unwrap();
        fs::write(a.join("x.json"), "1").unwrap();
        g.commit_all(&a, "one").await.unwrap();
        assert_eq!(g.sync_push(&a).await, OpResult::ok());
        assert_eq!(g.sync_status(&a).await, SyncStatus::Synced);

        let b = root.path().join("b");
        g.clone_to(url, &b).await.unwrap();
        fs::write(b.join("y.json"), "2").unwrap();
        g.commit_all(&b, "two").await.unwrap();
        assert_eq!(g.sync_status(&b).await, SyncStatus::Ahead { count: 1 });
        assert_eq!(g.sync_push(&b).await, OpResult::ok());

        assert_eq!(g.sync_status(&a).await, SyncStatus::Behind { count: 1 });
        assert_eq!(g.sync_pull(&a).await, OpResult::ok());
        assert!(a.join("y.json").exists());
    }

    #[test]
    fn sync_status_serializes_like_electron() {
        let v = serde_json::to_value(SyncStatus::Diverged {
            ahead: 1,
            behind: 2,
        })
        .unwrap();
        assert_eq!(
            v,
            serde_json::json!({ "status": "diverged", "ahead": 1, "behind": 2 })
        );
        let v = serde_json::to_value(SyncStatus::NoUpstream).unwrap();
        assert_eq!(v, serde_json::json!({ "status": "no-upstream" }));
    }
}
