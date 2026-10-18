//! Everything that touches disk, git or the network. Calls run on a tokio
//! runtime and report back as [`Event`]s, which the UI drains once per frame
//! ([`Backend::poll`]); nothing here blocks the UI thread.
//!
//! The logic is the Tauri host's (`apps/threadliner/src-tauri/src/main.rs`),
//! channel by channel, over the same `threadliner-core` store and sync
//! engine, so the data folder and `~/.config/ThreadLiner` stay compatible
//! with the Electron and Tauri builds.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Result};
use marina_core::git::SyncStatus;
use marina_core::updates::{self, UpdateCheck};
use marina_egui::about::UpdateStatus;
use serde_json::Value;
use threadliner_core::feed;
use threadliner_core::store::Store;
use threadliner_core::sync::{LogEntry, SyncManager};
use tokio::runtime::Runtime;
use tokio::sync::Notify;

use crate::config::{parse_int, truthy, Paths};
use crate::model::{Entry, Feed, Tag};

/// The poll intervals the Settings select offers, in minutes.
pub const ALLOWED_INTERVALS: [u64; 5] = [1, 5, 10, 30, 60];
const DEFAULT_INTERVAL_MIN: u64 = 10;
/// The push debounce when `syncWaitTime` is unset, in seconds.
const DEFAULT_WAIT_SECS: u64 = 10;
/// The git identity for commits when the repo has none.
const GIT_IDENTITY: (&str, &str) = ("ThreadLiner", "threadliner@localhost");
/// GitHub `owner/repo` publishing ThreadLiner releases. None, as in the
/// Tauri host, so About reports "latest version" without a request.
const UPDATE_REPO: Option<&str> = None;

/// A finished operation the UI may be waiting on (a dialog's spinner).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Setup,
    AddFeed,
    EditFeed,
    RemoveFeed,
    RefreshFeed,
    AddTag,
    EditTag,
    RemoveTag,
    AssignTag,
    MarkRead,
    SetSetting,
    PollNow,
    ForcePush,
    ForcePull,
    Connect,
    Disconnect,
    Pull,
    PullRebase,
    Push,
    Reset,
}

/// The synced settings the UI reads (`configuration/settings.json`).
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub sync_wait_time: String,
    pub poll_interval: String,
    pub notifications: bool,
    pub theme: Option<String>,
    pub scale: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            sync_wait_time: DEFAULT_WAIT_SECS.to_string(),
            poll_interval: DEFAULT_INTERVAL_MIN.to_string(),
            notifications: true,
            theme: None,
            scale: None,
        }
    }
}

/// A result coming back from the backend.
#[derive(Debug)]
pub enum Event {
    /// Start-up finished: the data store is open (or setup is needed).
    Ready {
        setup_complete: bool,
    },
    Feeds(Vec<Feed>),
    Tags(Vec<Tag>),
    Entries {
        feed_id: String,
        entries: Vec<Entry>,
    },
    Settings(Settings),
    /// An operation finished; `Err` carries the message to show.
    Done {
        op: Op,
        result: Result<(), String>,
    },
    /// The poller found new entries in these feeds.
    FeedsUpdated {
        feed_ids: Vec<String>,
    },
    Remote {
        url: Option<String>,
        branch: Option<String>,
    },
    GitStatus(SyncStatus),
    Update(UpdateStatus),
}

/// The shared state behind the backend's tasks.
struct Core {
    paths: Paths,
    store: Mutex<Option<Store>>,
    sync: Arc<SyncManager>,
    poll_minutes: AtomicU64,
    poll_reset: Notify,
    polling: AtomicBool,
    poller_started: AtomicBool,
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
    /// Starts the runtime and, if setup is complete, opens the data store.
    /// Call [`Backend::attach`] with the UI's context so events wake it.
    pub fn new(paths: Paths) -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("threadliner-backend")
            .build()
            .expect("tokio runtime");
        // The sync engine's debouncer spawns on the current runtime.
        let _guard = rt.enter();
        let (tx, rx) = channel();
        let core = Arc::new(Core {
            paths,
            store: Mutex::new(None),
            sync: SyncManager::new(GIT_IDENTITY),
            poll_minutes: AtomicU64::new(DEFAULT_INTERVAL_MIN),
            poll_reset: Notify::new(),
            polling: AtomicBool::new(false),
            poller_started: AtomicBool::new(false),
            tx: Mutex::new(tx),
            ctx: Mutex::new(None),
        });
        let backend = Self { rt, rx, core };
        let core = backend.core.clone();
        backend.rt.spawn(async move {
            let complete = core.paths.setup_complete();
            if complete {
                let dir = core.paths.configured_dir().unwrap_or_default();
                if let Err(e) = core.init_data_and_sync(&dir).await {
                    log::error!("init failed: {e}");
                }
                core.send_all();
            }
            core.send(Event::Ready {
                setup_complete: complete,
            });
        });
        backend
    }

    /// The context to repaint when an event arrives.
    pub fn attach(&self, ctx: &egui::Context) {
        let mut slot = self.core.ctx.lock().unwrap();
        if slot.is_none() {
            *slot = Some(ctx.clone());
            // Events may have arrived before the UI existed.
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

    /// `sync:getStatus`: the engine's state and the log entries after `since`.
    pub fn sync_status(&self, since: Option<u64>) -> Value {
        self.core.sync.status(since)
    }

    /// `sync:getLog`.
    pub fn sync_log(&self) -> Vec<LogEntry> {
        self.core.sync.full_log()
    }

    fn spawn<F>(&self, op: Op, f: impl FnOnce(Arc<Core>) -> F)
    where
        F: std::future::Future<Output = Result<()>> + Send + 'static,
    {
        let core = self.core.clone();
        let fut = f(core.clone());
        self.rt.spawn(async move {
            let result = fut.await.map_err(|e| e.to_string());
            if let Err(e) = &result {
                log::warn!("{op:?} failed: {e}");
            }
            core.send(Event::Done { op, result });
        });
    }

    // --- Setup ---------------------------------------------------------------

    /// `setup:init`: create or clone the data repo, write the config, open
    /// the store and start syncing and polling.
    pub fn setup(&self, dir: PathBuf, remote: Option<String>) {
        self.spawn(Op::Setup, |core| async move {
            std::fs::create_dir_all(&dir)?;
            let git = &core.sync.git;
            let is_repo = git.is_repo(&dir).await;
            match (&remote, is_repo) {
                (Some(url), false) => {
                    if std::fs::read_dir(&dir)?.next().is_none() {
                        // Clone needs to create the directory itself.
                        std::fs::remove_dir(&dir)?;
                        git.clone_to(url, &dir).await?;
                    } else {
                        git.init_repo(&dir, Some(url)).await?;
                    }
                }
                (None, false) => git.init_repo(&dir, None).await?,
                _ => {}
            }
            core.paths.write_setup(&dir, remote.as_deref())?;
            core.init_data_and_sync(&dir).await?;
            core.send_all();
            Ok(())
        });
    }

    // --- Feeds and entries ---------------------------------------------------

    pub fn load_entries(&self, feed_id: &str) {
        let core = self.core.clone();
        let feed_id = feed_id.to_string();
        self.rt.spawn(async move {
            match core.with_store(|s| Ok(s.entries(&feed_id))) {
                Ok(rows) => core.send(Event::Entries {
                    entries: rows.iter().map(Entry::from_row).collect(),
                    feed_id,
                }),
                Err(e) => core.send(Event::Done {
                    op: Op::RefreshFeed,
                    result: Err(format!("Failed to load entries: {e}")),
                }),
            }
        });
    }

    /// `feed:add`: fetch, store the feed and its entries.
    pub fn add_feed(&self, url: String) {
        self.spawn(Op::AddFeed, |core| async move {
            let parsed = feed::fetch_and_parse(&url).await?;
            let title = parsed
                .title
                .clone()
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| url.clone());
            let entries = feed::normalize_entries(&parsed.items);
            core.with_store(|s| {
                let f = s.add_feed(
                    &title,
                    &url,
                    parsed.link.clone(),
                    parsed.description.clone(),
                )?;
                s.insert_entries(f["id"].as_str().unwrap_or(""), &entries)?;
                Ok(())
            })?;
            core.sync.notify_change(&format!("Add feed: {title}")).await;
            core.send_feeds();
            Ok(())
        });
    }

    /// `feed:edit` with `{ title, url }`.
    pub fn edit_feed(&self, id: String, title: String, url: String) {
        self.spawn(Op::EditFeed, |core| async move {
            let data = serde_json::json!({ "title": title, "url": url });
            let feed = core.with_store(|s| s.edit_feed(&id, &data))?;
            let title = feed["title"].as_str().unwrap_or("").to_string();
            core.sync
                .notify_change(&format!("Edit feed: {title}"))
                .await;
            core.send_feeds();
            Ok(())
        });
    }

    pub fn remove_feed(&self, id: String) {
        self.spawn(Op::RemoveFeed, |core| async move {
            let title = core.with_store(|s| {
                let t = s
                    .feed(&id)
                    .and_then(|f| f["title"].as_str().map(str::to_string));
                s.remove_feed(&id)?;
                Ok(t)
            })?;
            core.sync
                .notify_change(&format!("Remove feed: {}", title.unwrap_or(id)))
                .await;
            core.send_feeds();
            Ok(())
        });
    }

    /// `feed:refresh`, then the feed's entries again.
    pub fn refresh_feed(&self, id: String) {
        self.spawn(Op::RefreshFeed, |core| async move {
            let feed = core
                .with_store(|s| Ok(s.feed(&id)))?
                .ok_or_else(|| anyhow!("Feed not found"))?;
            let parsed = feed::fetch_and_parse(feed["url"].as_str().unwrap_or("")).await?;
            let entries = feed::normalize_entries(&parsed.items);
            let inserted = core.with_store(|s| s.insert_entries(&id, &entries))?;
            if inserted > 0 {
                let title = feed["title"].as_str().unwrap_or("");
                core.sync
                    .notify_change(&format!("Refresh feed: {title}"))
                    .await;
            }
            core.send_feeds();
            core.send_entries(&id);
            Ok(())
        });
    }

    /// `entry:markRead` / `entry:markUnread`.
    pub fn mark_read(&self, entry_id: String, feed_id: String, read: bool) {
        self.spawn(Op::MarkRead, |core| async move {
            core.with_store(|s| s.set_read(&entry_id, &feed_id, read))?;
            core.send_feeds();
            core.sync
                .notify_change(if read {
                    "Mark entry read"
                } else {
                    "Mark entry unread"
                })
                .await;
            Ok(())
        });
    }

    /// `entry:markAllRead` / `entry:markAllUnread`.
    pub fn mark_all(&self, feed_id: String, read: bool) {
        self.spawn(Op::MarkRead, |core| async move {
            core.with_store(|s| s.set_all_read(&feed_id, read))?;
            core.send_feeds();
            core.sync
                .notify_change(if read {
                    "Mark all read"
                } else {
                    "Mark all unread"
                })
                .await;
            Ok(())
        });
    }

    // --- Tags ----------------------------------------------------------------

    pub fn add_tag(&self, name: String) {
        self.spawn(Op::AddTag, |core| async move {
            let tag = core.with_store(|s| s.add_tag(&name))?;
            let name = tag["name"].as_str().unwrap_or("").to_string();
            core.sync.notify_change(&format!("Add tag: {name}")).await;
            core.send_tags();
            Ok(())
        });
    }

    pub fn edit_tag(&self, id: String, name: String) {
        self.spawn(Op::EditTag, |core| async move {
            let data = serde_json::json!({ "name": name });
            let tag = core.with_store(|s| s.edit_tag(&id, &data))?;
            let name = tag["name"].as_str().unwrap_or("").to_string();
            core.sync.notify_change(&format!("Edit tag: {name}")).await;
            core.send_tags();
            Ok(())
        });
    }

    pub fn remove_tag(&self, id: String) {
        self.spawn(Op::RemoveTag, |core| async move {
            let name = core.with_store(|s| {
                let n = s
                    .tag(&id)
                    .and_then(|t| t["name"].as_str().map(str::to_string));
                s.remove_tag(&id)?;
                Ok(n)
            })?;
            core.sync
                .notify_change(&format!("Remove tag: {}", name.unwrap_or(id)))
                .await;
            core.send_tags();
            core.send_feeds();
            Ok(())
        });
    }

    /// `tag:assign` / `tag:unassign`.
    pub fn assign_tag(&self, feed_id: String, tag_id: String, assign: bool) {
        self.spawn(Op::AssignTag, |core| async move {
            core.with_store(|s| {
                if assign {
                    s.assign_tag(&feed_id, &tag_id)
                } else {
                    s.unassign_tag(&feed_id, &tag_id)
                }
            })?;
            core.send_feeds();
            core.sync
                .notify_change(if assign {
                    "Assign tag to feed"
                } else {
                    "Unassign tag from feed"
                })
                .await;
            Ok(())
        });
    }

    // --- Settings and polling ------------------------------------------------

    /// `settings:set`, with the side effects on the sync wait and poll interval.
    pub fn set_setting(&self, key: &'static str, value: Value) {
        self.spawn(Op::SetSetting, |core| async move {
            core.set_setting(key, value).await?;
            core.send(Event::Settings(core.settings()));
            Ok(())
        });
    }

    /// `poller:pollNow`.
    pub fn poll_now(&self) {
        self.spawn(Op::PollNow, |core| async move {
            core.poll_once().await;
            Ok(())
        });
    }

    /// Feeds, tags and settings again (after a pull changed the files).
    pub fn reload(&self) {
        let core = self.core.clone();
        self.rt.spawn(async move {
            if let Some(dir) = core.paths.configured_dir() {
                if let Ok(store) = Store::open(&dir) {
                    *core.store.lock().unwrap() = Some(store);
                }
            }
            core.send_all();
        });
    }

    // --- Sync ------------------------------------------------------------------

    pub fn force_push(&self) {
        self.spawn(Op::ForcePush, |core| async move {
            core.sync.force_push().await;
            Ok(())
        });
    }

    pub fn force_pull(&self) {
        self.spawn(Op::ForcePull, |core| async move {
            let res = core.sync.force_pull().await;
            if res["success"] == Value::Bool(false) {
                return Err(anyhow!(res["error"]
                    .as_str()
                    .unwrap_or("Pull failed")
                    .to_string()));
            }
            Ok(())
        });
    }

    /// `git:getRemoteUrl` and `git:getBranch`.
    pub fn load_remote(&self) {
        let core = self.core.clone();
        self.rt.spawn(async move {
            let (url, branch) = match core.paths.configured_dir() {
                Some(d) => (
                    core.sync.git.get_remote_url(&d).await,
                    core.sync.git.get_branch(&d).await,
                ),
                None => (None, None),
            };
            core.send(Event::Remote { url, branch });
        });
    }

    /// `git:getSyncStatus` (fetches first).
    pub fn git_status(&self) {
        let core = self.core.clone();
        self.rt.spawn(async move {
            let status = match core.paths.configured_dir() {
                Some(d) => core.sync.git.sync_status(&d).await,
                None => SyncStatus::Error {
                    message: "No data directory configured".into(),
                },
            };
            core.send(Event::GitStatus(status));
        });
    }

    /// The Sync modal's Connect: `git:setRemoteUrl`, then `git:pushUpstream`.
    pub fn connect(&self, url: String) {
        self.spawn(Op::Connect, |core| async move {
            let dir = core.data_dir()?;
            core.sync.git.set_remote_url(&dir, &url).await?;
            core.paths.set_remote_url(Some(&url))?;
            let r = core.sync.git.sync_push(&dir).await;
            if !r.success {
                return Err(anyhow!(r.error.unwrap_or_else(|| "Push failed".into())));
            }
            Ok(())
        });
    }

    /// `git:removeRemote`.
    pub fn disconnect(&self) {
        self.spawn(Op::Disconnect, |core| async move {
            let dir = core.data_dir()?;
            let _ = core.sync.git.remove_remote(&dir).await;
            core.paths.set_remote_url(None)?;
            Ok(())
        });
    }

    /// `git:pull` (merge).
    pub fn pull(&self) {
        self.spawn(Op::Pull, |core| async move {
            let r = core.sync.git.pull_merge(&core.data_dir()?).await;
            if !r.success {
                return Err(anyhow!(r.error.unwrap_or_else(|| "Pull failed".into())));
            }
            core.reopen_store()?;
            core.send_all();
            Ok(())
        });
    }

    /// `git:pullRebase`.
    pub fn pull_rebase(&self) {
        self.spawn(Op::PullRebase, |core| async move {
            let r = core.sync.git.sync_pull(&core.data_dir()?).await;
            if !r.success {
                return Err(anyhow!(r.error.unwrap_or_else(|| "Pull failed".into())));
            }
            core.reopen_store()?;
            core.send_all();
            Ok(())
        });
    }

    /// `git:push` (the engine's manual sync).
    pub fn push(&self) {
        self.spawn(Op::Push, |core| async move {
            core.sync.force_push().await;
            Ok(())
        });
    }

    /// `git:resetToRemote`, then everything reloads from disk.
    pub fn reset_to_remote(&self) {
        self.spawn(Op::Reset, |core| async move {
            let dir = core.data_dir()?;
            core.sync.git.reset_to_remote(&dir, None).await?;
            core.reopen_store()?;
            core.send_all();
            Ok(())
        });
    }

    // --- About -----------------------------------------------------------------

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

    fn data_dir(&self) -> Result<PathBuf> {
        self.paths
            .configured_dir()
            .ok_or_else(|| anyhow!("No data directory configured"))
    }

    fn with_store<T>(&self, f: impl FnOnce(&mut Store) -> Result<T>) -> Result<T> {
        let mut guard = self.store.lock().unwrap();
        let store = guard
            .as_mut()
            .ok_or_else(|| anyhow!("Data store is not initialised"))?;
        f(store)
    }

    /// The store caches feeds and tags: reopen it after git rewrote the files.
    fn reopen_store(&self) -> Result<()> {
        let dir = self.data_dir()?;
        *self.store.lock().unwrap() = Some(Store::open(&dir)?);
        Ok(())
    }

    fn setting(&self, key: &str) -> Value {
        self.store
            .lock()
            .unwrap()
            .as_ref()
            .map(|s| s.get_setting(key))
            .unwrap_or(Value::Null)
    }

    /// The settings the UI shows, defaulted as the Svelte Settings modal does.
    fn settings(&self) -> Settings {
        let mut out = Settings::default();
        let wait = self.setting("syncWaitTime");
        if truthy(&wait) {
            out.sync_wait_time = value_string(&wait);
        }
        let poll = self.setting("pollInterval");
        if truthy(&poll) {
            out.poll_interval = value_string(&poll);
        }
        let notify = self.setting("pollNotificationsEnabled");
        out.notifications = notify.is_null() || truthy(&notify);
        out.theme = self.setting("theme").as_str().map(str::to_string);
        out.scale = self.setting("scale").as_str().map(str::to_string);
        out
    }

    async fn set_setting(&self, key: &str, value: Value) -> Result<()> {
        let saved = self
            .store
            .lock()
            .unwrap()
            .as_ref()
            .map(|s| s.set_setting(key, value.clone()));
        if let Some(r) = saved {
            r?;
        }
        match key {
            "syncWaitTime" => self.sync.update_wait_time(
                parse_int(&value)
                    .filter(|n| *n > 0)
                    .unwrap_or(DEFAULT_WAIT_SECS),
            ),
            "pollInterval" => self.set_poll_interval(&value),
            _ => {}
        }
        self.sync
            .notify_change(&format!("Update setting: {key}"))
            .await;
        Ok(())
    }

    fn send_feeds(&self) {
        if let Ok(rows) = self.with_store(|s| Ok(s.all_feeds())) {
            self.send(Event::Feeds(rows.iter().map(Feed::from_row).collect()));
        }
    }

    fn send_tags(&self) {
        if let Ok(rows) = self.with_store(|s| Ok(s.all_tags())) {
            self.send(Event::Tags(rows.iter().map(Tag::from_row).collect()));
        }
    }

    fn send_entries(&self, feed_id: &str) {
        if let Ok(rows) = self.with_store(|s| Ok(s.entries(feed_id))) {
            self.send(Event::Entries {
                feed_id: feed_id.to_string(),
                entries: rows.iter().map(Entry::from_row).collect(),
            });
        }
    }

    /// Feeds, tags and settings (after setup, a pull or a reset).
    fn send_all(&self) {
        self.send_feeds();
        self.send_tags();
        self.send(Event::Settings(self.settings()));
    }

    async fn init_data_and_sync(self: &Arc<Self>, dir: &Path) -> Result<()> {
        self.sync.git.configure_user(dir).await?;
        *self.store.lock().unwrap() = Some(Store::open(dir)?);
        let wait = match self.setting("syncWaitTime") {
            Value::Null => DEFAULT_WAIT_SECS,
            v => parse_int(&v).unwrap_or(DEFAULT_WAIT_SECS),
        };
        self.sync.init(dir.to_path_buf(), wait);
        self.sync.notify_change("Initialize data store").await;
        self.set_poll_interval(&self.setting("pollInterval"));
        self.start_poller();
        Ok(())
    }

    fn set_poll_interval(&self, minutes: &Value) {
        let n = parse_int(minutes)
            .filter(|n| ALLOWED_INTERVALS.contains(n))
            .unwrap_or(DEFAULT_INTERVAL_MIN);
        self.poll_minutes.store(n, Ordering::SeqCst);
        self.poll_reset.notify_waiters();
    }

    fn start_poller(self: &Arc<Self>) {
        if self.poller_started.swap(true, Ordering::SeqCst) {
            self.poll_reset.notify_waiters();
            return;
        }
        let me = self.clone();
        tokio::spawn(async move {
            loop {
                let wait = Duration::from_secs(me.poll_minutes.load(Ordering::SeqCst) * 60);
                tokio::select! {
                    _ = tokio::time::sleep(wait) => me.poll_once().await,
                    _ = me.poll_reset.notified() => {}
                }
            }
        });
    }

    /// One pass over every feed (`feed-poller.js`).
    async fn poll_once(self: &Arc<Self>) {
        if self.polling.swap(true, Ordering::SeqCst) {
            return;
        }
        let feeds = self.with_store(|s| Ok(s.all_feeds())).unwrap_or_default();
        let mut updated: Vec<(String, String)> = Vec::new();
        let mut total = 0usize;
        for f in feeds {
            let (Some(id), Some(url)) = (f["id"].as_str(), f["url"].as_str()) else {
                continue;
            };
            match feed::fetch_and_parse(url).await {
                Ok(parsed) => {
                    let entries = feed::normalize_entries(&parsed.items);
                    if let Ok(n) = self.with_store(|s| s.insert_entries(id, &entries)) {
                        if n > 0 {
                            total += n;
                            let title = f["title"].as_str().unwrap_or("").to_string();
                            updated.push((id.to_string(), title));
                        }
                    }
                }
                Err(e) => log::warn!("poll failed for {url}: {e}"),
            }
        }
        self.polling.store(false, Ordering::SeqCst);
        if total == 0 {
            return;
        }

        let noun = if total == 1 { "entry" } else { "entries" };
        self.sync
            .notify_change(&format!("Poll: {total} new {noun}"))
            .await;
        self.send_feeds();
        self.send(Event::FeedsUpdated {
            feed_ids: updated.iter().map(|u| u.0.clone()).collect(),
        });

        let enabled = self.setting("pollNotificationsEnabled");
        if enabled.is_null() || truthy(&enabled) {
            let summary = if updated.len() == 1 {
                updated[0].1.clone()
            } else {
                format!("{} feeds", updated.len())
            };
            notify(format!("{total} new {noun} in {summary}"));
        }
    }
}

/// A system notification, off the async workers (it may block on D-Bus).
fn notify(body: String) {
    std::thread::spawn(move || {
        if let Err(e) = notify_rust::Notification::new()
            .summary("ThreadLiner")
            .body(&body)
            .show()
        {
            log::warn!("notification failed: {e}");
        }
    });
}

/// `String(value)` for a stored setting.
fn value_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}
