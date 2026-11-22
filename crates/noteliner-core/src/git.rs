//! What Files Info shows of a project that is a Git repository: the
//! remote's address, the branch, and whether anything is uncommitted.
//!
//! Two `git` commands are run, and both only read. `--no-optional-locks`
//! keeps `git status` from refreshing the index, so `.git` is not written
//! to. Nothing is fetched: what is on the remote is as of the last fetch.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

/// How long a `git` command may take.
const TIMEOUT: Duration = Duration::from_secs(5);

/// A repository, as Files Info shows it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GitInfo {
    /// The address of `origin`, or of the first remote, with no user name
    /// or password in it. `None` when the repository has no remote.
    pub remote: Option<String>,
    pub branch: Branch,
    /// The remote branch this one follows, such as `origin/main`.
    pub upstream: Option<String>,
    /// Files changed, added, removed, renamed, or untracked.
    pub changes: usize,
    /// Commits to push, and to pull, as of the last fetch.
    pub ahead: usize,
    pub behind: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Branch {
    Named(String),
    /// No branch is checked out: the commit's short id.
    Detached(String),
    /// A repository with no commit yet, and no branch name read.
    #[default]
    Unknown,
}

impl Branch {
    pub fn label(&self) -> String {
        match self {
            Branch::Named(name) => name.clone(),
            Branch::Detached(id) if id.is_empty() => "Detached".to_string(),
            Branch::Detached(id) => format!("Detached at {id}"),
            Branch::Unknown => "None".to_string(),
        }
    }
}

impl GitInfo {
    /// "Up to date", or the number of uncommitted changes.
    pub fn status(&self) -> String {
        match self.changes {
            0 => "Up to date".to_string(),
            1 => "1 uncommitted change".to_string(),
            n => format!("{n} uncommitted changes"),
        }
    }

    /// The commits to push and to pull, when there are any.
    pub fn sync(&self) -> Option<String> {
        let commits = |n: usize, what: &str| match n {
            1 => format!("1 commit to {what}"),
            n => format!("{n} commits to {what}"),
        };
        match (self.ahead, self.behind) {
            (0, 0) => None,
            (a, 0) => Some(commits(a, "push")),
            (0, b) => Some(commits(b, "pull")),
            (a, b) => Some(format!("{}, {}", commits(a, "push"), commits(b, "pull"))),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GitError {
    /// There is no `git` program to run.
    NotInstalled,
    /// `git` ran and failed, or took too long: the first line it said.
    Failed(String),
}

impl GitError {
    /// The line Files Info shows.
    pub fn message(&self) -> String {
        match self {
            GitError::NotInstalled => {
                "Git is not installed, so the status is not available.".to_string()
            }
            GitError::Failed(why) if why.is_empty() => {
                "Git could not read this repository.".to_string()
            }
            GitError::Failed(why) => format!("Git could not read this repository. {why}"),
        }
    }
}

/// `folder` is the top of a Git repository: it has a `.git` in it (a folder,
/// or the file a worktree or a submodule has).
pub fn is_repo(folder: &Path) -> bool {
    folder.join(".git").exists()
}

/// Reads the repository at `folder`.
pub async fn read(folder: &Path) -> Result<GitInfo, GitError> {
    let status = run(
        folder,
        &[
            "--no-optional-locks",
            "status",
            "--porcelain=v2",
            "--branch",
        ],
    )
    .await?;
    let mut info = parse_status(&status);
    // A repository whose remotes can't be listed still has a status.
    let remotes = run(folder, &["remote", "-v"]).await.unwrap_or_default();
    info.remote = parse_remotes(&remotes);
    Ok(info)
}

async fn run(folder: &Path, args: &[&str]) -> Result<String, GitError> {
    let mut cmd = tokio::process::Command::new("git");
    cmd.args(args)
        .current_dir(folder)
        // Never wait for a password, and never write the index.
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        // Only this folder's own repository, never one in a folder above it.
        .env(
            "GIT_CEILING_DIRECTORIES",
            folder.parent().unwrap_or(folder).as_os_str(),
        )
        // The same words whatever the language, for the first line of an error.
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            GitError::NotInstalled
        } else {
            GitError::Failed(e.to_string())
        }
    })?;
    let out = match tokio::time::timeout(TIMEOUT, child.wait_with_output()).await {
        Ok(Ok(out)) => out,
        Ok(Err(e)) => return Err(GitError::Failed(e.to_string())),
        Err(_) => return Err(GitError::Failed("It took too long.".to_string())),
    };
    if !out.status.success() {
        let said = String::from_utf8_lossy(&out.stderr);
        let line = said.lines().next().unwrap_or_default().trim();
        let line = line.strip_prefix("fatal: ").unwrap_or(line);
        return Err(GitError::Failed(line.to_string()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Reads `git status --porcelain=v2 --branch`.
pub fn parse_status(out: &str) -> GitInfo {
    let mut info = GitInfo::default();
    let mut oid = "";
    let mut head = "";
    for line in out.lines() {
        if let Some(rest) = line.strip_prefix("# branch.oid ") {
            oid = rest.trim();
        } else if let Some(rest) = line.strip_prefix("# branch.head ") {
            head = rest.trim();
        } else if let Some(rest) = line.strip_prefix("# branch.upstream ") {
            info.upstream = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("# branch.ab ") {
            for part in rest.split_whitespace() {
                if let Some(n) = part.strip_prefix('+') {
                    info.ahead = n.parse().unwrap_or(0);
                } else if let Some(n) = part.strip_prefix('-') {
                    info.behind = n.parse().unwrap_or(0);
                }
            }
        } else if ["1 ", "2 ", "u ", "? "].iter().any(|p| line.starts_with(p)) {
            info.changes += 1;
        }
    }
    info.branch = match head {
        "" => Branch::Unknown,
        "(detached)" => Branch::Detached(if oid == "(initial)" {
            String::new()
        } else {
            oid.chars().take(7).collect()
        }),
        name => Branch::Named(name.to_string()),
    };
    info
}

/// Reads `git remote -v`: the address `origin` is fetched from, or the first
/// remote's when there is no `origin`.
pub fn parse_remotes(out: &str) -> Option<String> {
    let mut first = None;
    for line in out.lines() {
        let Some((name, rest)) = line.split_once('\t') else {
            continue;
        };
        let url = rest
            .strip_suffix(" (fetch)")
            .or_else(|| rest.strip_suffix(" (push)"))
            .unwrap_or(rest)
            .trim();
        if url.is_empty() {
            continue;
        }
        if name == "origin" {
            return Some(clean_url(url));
        }
        first.get_or_insert_with(|| clean_url(url));
    }
    first
}

/// Takes a user name, a password, or a token out of an address with a
/// scheme. `git@host:you/notes.git` has none of these and is left alone.
pub fn clean_url(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_string();
    };
    let (host, path) = match rest.find('/') {
        Some(i) => rest.split_at(i),
        None => (rest, ""),
    };
    let Some((user, host_only)) = host.rsplit_once('@') else {
        return url.to_string();
    };
    // `ssh://git@host/...` names the account every user signs in as.
    if scheme == "ssh" && !user.contains(':') {
        return url.to_string();
    }
    format!("{scheme}://{host_only}{path}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clean_branch_that_follows_its_remote() {
        let info = parse_status(
            "# branch.oid 1234567890abcdef\n# branch.head main\n# branch.upstream origin/main\n# branch.ab +0 -0\n",
        );
        assert_eq!(info.branch, Branch::Named("main".into()));
        assert_eq!(info.upstream.as_deref(), Some("origin/main"));
        assert_eq!(info.status(), "Up to date");
        assert_eq!(info.sync(), None);
    }

    #[test]
    fn each_kind_of_change_is_counted_once() {
        let info = parse_status(concat!(
            "# branch.oid abc\n",
            "# branch.head main\n",
            "1 .M N... 100644 100644 100644 aaa bbb changed.md\n",
            "1 A. N... 000000 100644 100644 000 ccc added.md\n",
            "1 D. N... 100644 000000 000000 ddd 000 removed.md\n",
            "2 R. N... 100644 100644 100644 eee eee R100 new.md\told.md\n",
            "u UU N... 100644 100644 100644 100644 a b c both.md\n",
            "? untracked.md\n",
            "! ignored.md\n",
        ));
        assert_eq!(info.changes, 6);
        assert_eq!(info.status(), "6 uncommitted changes");
        assert_eq!(info.upstream, None);
    }

    #[test]
    fn one_change_is_singular() {
        let info = parse_status("# branch.head main\n? a.md\n");
        assert_eq!(info.status(), "1 uncommitted change");
    }

    #[test]
    fn ahead_behind_and_both() {
        let ab = |s: &str| parse_status(&format!("# branch.head main\n# branch.ab {s}\n")).sync();
        assert_eq!(ab("+2 -0").as_deref(), Some("2 commits to push"));
        assert_eq!(ab("+0 -1").as_deref(), Some("1 commit to pull"));
        assert_eq!(
            ab("+1 -3").as_deref(),
            Some("1 commit to push, 3 commits to pull")
        );
    }

    #[test]
    fn a_detached_head_and_a_new_repository() {
        let info = parse_status("# branch.oid 1234567890abcdef\n# branch.head (detached)\n");
        assert_eq!(info.branch.label(), "Detached at 1234567");
        let info = parse_status("# branch.oid (initial)\n# branch.head main\n");
        assert_eq!(info.branch.label(), "main");
    }

    #[test]
    fn origin_is_preferred_to_the_first_remote() {
        let out = "backup\thttps://example.com/b.git (fetch)\nbackup\thttps://example.com/b.git (push)\norigin\tgit@example.com:you/notes.git (fetch)\norigin\tgit@example.com:you/notes.git (push)\n";
        assert_eq!(
            parse_remotes(out).as_deref(),
            Some("git@example.com:you/notes.git")
        );
        assert_eq!(
            parse_remotes("backup\thttps://example.com/b.git (fetch)\n").as_deref(),
            Some("https://example.com/b.git")
        );
        assert_eq!(parse_remotes(""), None);
    }

    #[test]
    fn a_name_and_a_token_are_taken_out_of_an_address() {
        assert_eq!(
            clean_url("https://user:token@example.com/you/notes.git"),
            "https://example.com/you/notes.git"
        );
        assert_eq!(
            clean_url("https://token@example.com/you/notes.git"),
            "https://example.com/you/notes.git"
        );
        assert_eq!(
            clean_url("https://example.com/you/n@tes.git"),
            "https://example.com/you/n@tes.git"
        );
        assert_eq!(
            clean_url("git@example.com:you/notes.git"),
            "git@example.com:you/notes.git"
        );
        assert_eq!(
            clean_url("ssh://git@example.com/you/notes.git"),
            "ssh://git@example.com/you/notes.git"
        );
        assert_eq!(clean_url("/home/you/notes.git"), "/home/you/notes.git");
    }

    /// Every file under `dir`, with its bytes.
    fn snapshot(dir: &Path) -> Vec<(std::path::PathBuf, Vec<u8>)> {
        let mut out = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            for entry in std::fs::read_dir(&d).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else {
                    out.push((path.clone(), std::fs::read(&path).unwrap()));
                }
            }
        }
        out.sort();
        out
    }

    fn git(dir: &Path, args: &[&str]) -> bool {
        std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .is_ok_and(|o| o.status.success())
    }

    /// Skipped where `git` is not installed.
    #[tokio::test]
    async fn a_real_repository_is_read_and_left_as_it_was() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        assert!(!is_repo(dir));
        if !git(dir, &["init", "-q", "-b", "main"]) {
            eprintln!("git is not installed: skipped");
            return;
        }
        assert!(is_repo(dir));
        std::fs::write(dir.join("a.md"), "one\n").unwrap();
        let commit = [
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-q",
            "-m",
            "first",
        ];
        assert!(git(dir, &["add", "a.md"]));
        assert!(git(dir, &commit));
        assert!(git(
            dir,
            &[
                "remote",
                "add",
                "origin",
                "https://me:secret@example.com/you/notes.git"
            ]
        ));

        let info = read(dir).await.unwrap();
        assert_eq!(info.branch, Branch::Named("main".into()));
        assert_eq!(info.status(), "Up to date");
        assert_eq!(
            info.remote.as_deref(),
            Some("https://example.com/you/notes.git")
        );

        // Changed after the index was written: `git status` would refresh
        // the index here, were it allowed to.
        std::fs::write(dir.join("a.md"), "two\n").unwrap();
        std::fs::write(dir.join("b.md"), "new\n").unwrap();
        let before = snapshot(&dir.join(".git"));
        let info = read(dir).await.unwrap();
        assert_eq!(info.changes, 2);
        assert_eq!(snapshot(&dir.join(".git")), before);
    }

    #[tokio::test]
    async fn a_folder_that_only_looks_like_a_repository_fails() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
        std::fs::write(tmp.path().join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        // Either way it is an error and not a status.
        match read(tmp.path()).await {
            Err(GitError::NotInstalled) => {}
            Err(GitError::Failed(_)) => {}
            Ok(info) => panic!("read as a repository: {info:?}"),
        }
    }
}
