//! The project folder on disk: one `.mmd` file per diagram, named after it.
//!
//! The quick operations (list, read, create, rename, delete) are small
//! local files, so they run synchronously and return their result. Only the
//! autosave runs in the background: [`Backend::save`] keeps the latest text
//! and writes it (atomically, temp file + rename) once edits pause for the
//! save delay, through `laankit_core::debounce`. [`Backend::flush`] writes it
//! at once, before switching diagrams or quitting.
//!
//! [`Backend::check`] notices when the open diagram's file was changed by
//! something else (an editor, a `git pull`): the app calls it about once a
//! second. A change that's only our own write is not reported.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use anyhow::{anyhow, Context, Result};
use laankit_core::debounce::Debouncer;
use tokio::runtime::Runtime;

/// Diagram files' extension.
pub const EXT: &str = "mmd";
/// Why a new or renamed diagram's name was refused.
pub const TAKEN: &str = "There's already a diagram with that name.";
/// How long edits must pause before the autosave writes.
pub const SAVE_DELAY: Duration = Duration::from_millis(800);

/// A result coming back from the autosave.
#[derive(Debug, PartialEq)]
pub enum Event {
    Saved(PathBuf),
    SaveFailed { path: PathBuf, message: String },
}

/// What [`Backend::check`] found.
#[derive(Debug, PartialEq)]
pub enum Change {
    /// The file has new text, written by something else.
    Edited(String),
    /// The file is gone (deleted or renamed outside BlockLiner).
    Removed,
}

/// The open diagram's file as BlockLiner last read or wrote it.
struct OnDisk {
    path: PathBuf,
    mtime: Option<SystemTime>,
    text: String,
}

struct Core {
    /// The text waiting for the autosave.
    pending: Mutex<Option<(PathBuf, String)>>,
    /// Held across a write and the bookkeeping after it.
    writing: Mutex<()>,
    open: Mutex<Option<OnDisk>>,
    saver: Debouncer,
    tx: Mutex<Sender<Event>>,
    /// Repainted when an event arrives; set by [`Backend::attach`].
    ctx: Mutex<Option<egui::Context>>,
}

pub struct Backend {
    /// Runs the debounced autosave.
    rt: Runtime,
    rx: Receiver<Event>,
    core: Arc<Core>,
    save_delay: Duration,
}

impl Backend {
    pub fn new(save_delay: Duration) -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .thread_name("blockliner-backend")
            .build()
            .expect("tokio runtime");
        let (tx, rx) = channel();
        let core = Arc::new(Core {
            pending: Mutex::new(None),
            writing: Mutex::new(()),
            open: Mutex::new(None),
            saver: Debouncer::new(),
            tx: Mutex::new(tx),
            ctx: Mutex::new(None),
        });
        Self {
            rt,
            rx,
            core,
            save_delay,
        }
    }

    /// The context to repaint when an event arrives.
    pub fn attach(&self, ctx: &egui::Context) {
        let mut slot = self.core.ctx.lock().unwrap();
        if slot.is_none() {
            *slot = Some(ctx.clone());
        }
    }

    /// This frame's events, in arrival order.
    pub fn poll(&self) -> Vec<Event> {
        self.rx.try_iter().collect()
    }

    // --- The folder ------------------------------------------------------------

    /// The diagrams in `folder` (`.mmd` files, not in subfolders), by name,
    /// sorted case-insensitively.
    pub fn list(folder: &Path) -> io::Result<Vec<String>> {
        let mut names: Vec<String> = fs::read_dir(folder)?
            .filter_map(|e| {
                let path = e.ok()?.path();
                if !path.is_file() || path.extension()? != EXT {
                    return None;
                }
                Some(path.file_stem()?.to_str()?.to_owned())
            })
            .filter(|n| !n.starts_with('.'))
            .collect();
        names.sort_by_key(|n| (n.to_lowercase(), n.clone()));
        Ok(names)
    }

    /// A diagram's text.
    pub fn read(path: &Path) -> Result<String> {
        fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))
    }

    /// Watches the diagram just opened from `path`, with `text`, for
    /// outside changes (see [`Backend::check`]).
    pub fn watch(&self, path: &Path, text: &str) {
        *self.core.open.lock().unwrap() = Some(OnDisk {
            path: path.to_owned(),
            mtime: mtime(path),
            text: text.to_owned(),
        });
    }

    /// Stops watching the open diagram (it was closed).
    pub fn close(&self) {
        *self.core.open.lock().unwrap() = None;
    }

    /// A new diagram file with `text`; fails if there is one by that name.
    pub fn create(&self, path: &Path, text: &str) -> Result<()> {
        if path.exists() {
            return Err(anyhow!(TAKEN));
        }
        laankit_core::json::write_string(path, text)
    }

    /// Renames a diagram file, first writing any edits waiting for it.
    pub fn rename(&self, from: &Path, to: &Path) -> Result<()> {
        // A case-only rename is the same file on Windows and macOS.
        let same = from.to_string_lossy().to_lowercase() == to.to_string_lossy().to_lowercase();
        if to.exists() && !same {
            return Err(anyhow!(TAKEN));
        }
        self.flush();
        let _w = self.core.writing.lock().unwrap();
        fs::rename(from, to).with_context(|| format!("renaming {}", from.display()))?;
        if let Some(open) = self.core.open.lock().unwrap().as_mut() {
            if open.path == from {
                open.path = to.to_owned();
            }
        }
        Ok(())
    }

    /// Deletes a diagram file, dropping any edits waiting for it.
    pub fn delete(&self, path: &Path) -> Result<()> {
        {
            let mut pending = self.core.pending.lock().unwrap();
            if pending.as_ref().is_some_and(|(p, _)| p == path) {
                *pending = None;
                self.core.saver.cancel();
            }
        }
        let _w = self.core.writing.lock().unwrap();
        fs::remove_file(path).with_context(|| format!("deleting {}", path.display()))?;
        let mut open = self.core.open.lock().unwrap();
        if open.as_ref().is_some_and(|o| o.path == path) {
            *open = None;
        }
        Ok(())
    }

    // --- Autosave --------------------------------------------------------------

    /// Saves `text` to `path` once edits pause. A newer save replaces it.
    pub fn save(&self, path: PathBuf, text: String) {
        // Edits for another file can't wait behind this one.
        let other = self
            .core
            .pending
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|(p, _)| *p != path);
        if other {
            self.flush();
        }
        *self.core.pending.lock().unwrap() = Some((path, text));
        let core = self.core.clone();
        // The debouncer spawns on the current runtime: this backend's own
        // (`debounce::set_runtime` is process-wide and keeps the first one).
        let _rt = self.rt.enter();
        self.core
            .saver
            .schedule(self.save_delay, move || async move {
                tokio::task::spawn_blocking(move || core.write_pending());
            });
    }

    /// Writes any waiting edits now, on this thread. `false` when that
    /// write failed (which also arrives as [`Event::SaveFailed`]).
    pub fn flush(&self) -> bool {
        self.core.saver.cancel();
        self.core.write_pending()
    }

    /// Edits are waiting for the autosave.
    pub fn is_saving(&self) -> bool {
        self.core.pending.lock().unwrap().is_some()
    }

    #[cfg(test)]
    pub fn set_save_delay(&mut self, delay: Duration) {
        self.save_delay = delay;
    }

    // --- Outside changes -------------------------------------------------------

    /// Whether the open diagram's file changed on disk since BlockLiner last
    /// read or wrote it. Nothing is reported while our own edits are waiting:
    /// they win, and the autosave overwrites the file.
    pub fn check(&self) -> Option<Change> {
        if self.is_saving() {
            return None;
        }
        let _w = self.core.writing.lock().unwrap();
        let mut slot = self.core.open.lock().unwrap();
        let open = slot.as_mut()?;
        let now = mtime(&open.path);
        if now.is_none() {
            *slot = None;
            return Some(Change::Removed);
        }
        if now == open.mtime {
            return None;
        }
        open.mtime = now;
        let text = fs::read_to_string(&open.path).ok()?;
        if text == open.text {
            return None;
        }
        open.text = text.clone();
        Some(Change::Edited(text))
    }
}

impl Core {
    /// `false` when the write failed; `true` when it succeeded, or there
    /// was nothing to write.
    fn write_pending(&self) -> bool {
        let _w = self.writing.lock().unwrap();
        // Still pending while it's written, so `is_saving` stays true until
        // the file is on disk.
        let Some((path, text)) = self.pending.lock().unwrap().clone() else {
            return true;
        };
        let result = laankit_core::json::write_string(&path, &text);
        {
            let mut pending = self.pending.lock().unwrap();
            if pending.as_ref() == Some(&(path.clone(), text.clone())) {
                *pending = None;
            }
        }
        let written = result.is_ok();
        let event = match result {
            Ok(()) => {
                if let Some(open) = self.open.lock().unwrap().as_mut() {
                    if open.path == path {
                        open.mtime = mtime(&path);
                        open.text = text;
                    }
                }
                Event::Saved(path)
            }
            Err(e) => Event::SaveFailed {
                path,
                message: format!("{e:#}"),
            },
        };
        let _ = self.tx.lock().unwrap().send(event);
        if let Some(ctx) = self.ctx.lock().unwrap().as_ref() {
            ctx.request_repaint();
        }
        written
    }
}

fn mtime(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// A diagram's file in `folder`.
pub fn file(folder: &Path, name: &str) -> PathBuf {
    folder.join(format!("{name}.{EXT}"))
}

/// A diagram name the user typed, trimmed, or why it can't be a file name.
pub fn valid_name(name: &str) -> Result<String, &'static str> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Enter a name.");
    }
    if name.starts_with('.') {
        return Err("A name can't start with a dot.");
    }
    if name.chars().any(|c| {
        matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control()
    }) {
        return Err("A name can't contain / \\ : * ? \" < > |");
    }
    if name.chars().count() > 100 {
        return Err("That name is too long.");
    }
    Ok(name.to_owned())
}

/// `base`, or `base 2`, `base 3`, … so it isn't one of `names`
/// (case-insensitively).
pub fn unique_name(base: &str, names: &[String]) -> String {
    let taken = |n: &str| names.iter().any(|m| m.to_lowercase() == n.to_lowercase());
    if !taken(base) {
        return base.to_owned();
    }
    (2..)
        .map(|i| format!("{base} {i}"))
        .find(|n| !taken(n))
        .expect("some number is free")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn backend() -> (tempfile::TempDir, Backend) {
        (
            tempfile::tempdir().unwrap(),
            Backend::new(Duration::from_millis(20)),
        )
    }

    fn wait_until(mut done: impl FnMut() -> bool) {
        for _ in 0..200 {
            if done() {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("timed out");
    }

    #[test]
    fn lists_only_diagrams_sorted() {
        let (tmp, _b) = backend();
        for f in ["b.mmd", "A.mmd", "c.md", ".hidden.mmd", "x.mmd.tmp"] {
            fs::write(tmp.path().join(f), "").unwrap();
        }
        fs::create_dir(tmp.path().join("sub.mmd")).unwrap();
        assert_eq!(Backend::list(tmp.path()).unwrap(), ["A", "b"]);
    }

    #[test]
    fn autosave_writes_the_latest_text_once_edits_pause() {
        let (tmp, b) = backend();
        let path = file(tmp.path(), "d");
        for text in ["one", "two", "three"] {
            b.save(path.clone(), text.into());
        }
        assert!(b.is_saving());
        wait_until(|| !b.is_saving());
        assert_eq!(fs::read_to_string(&path).unwrap(), "three");
        assert_eq!(b.poll(), [Event::Saved(path)]);
    }

    #[test]
    fn flush_writes_now_and_switching_files_flushes() {
        let (tmp, b) = backend();
        let (one, two) = (file(tmp.path(), "one"), file(tmp.path(), "two"));
        b.save(one.clone(), "1".into());
        b.save(two.clone(), "2".into());
        assert_eq!(fs::read_to_string(&one).unwrap(), "1");
        b.flush();
        assert!(!b.is_saving());
        assert_eq!(fs::read_to_string(&two).unwrap(), "2");
        assert_eq!(b.poll(), [Event::Saved(one), Event::Saved(two)]);
    }

    #[test]
    fn outside_changes_but_not_our_own() {
        let (tmp, b) = backend();
        let path = file(tmp.path(), "d");
        fs::write(&path, "first").unwrap();
        let text = Backend::read(&path).unwrap();
        assert_eq!(text, "first");
        b.watch(&path, &text);
        assert_eq!(b.check(), None);

        b.save(path.clone(), "ours".into());
        assert_eq!(b.check(), None, "our edits are waiting");
        b.flush();
        assert_eq!(b.check(), None, "our own write");

        // Some file systems keep whole-second times: make sure it moves.
        std::thread::sleep(Duration::from_millis(20));
        fs::write(&path, "theirs").unwrap();
        let f = fs::File::options().write(true).open(&path).unwrap();
        f.set_modified(SystemTime::now() + Duration::from_secs(2))
            .unwrap();
        assert_eq!(b.check(), Some(Change::Edited("theirs".into())));
        assert_eq!(b.check(), None, "reported once");

        fs::remove_file(&path).unwrap();
        assert_eq!(b.check(), Some(Change::Removed));
        assert_eq!(b.check(), None);
    }

    #[test]
    fn create_rename_delete() {
        let (tmp, b) = backend();
        let (a, c) = (file(tmp.path(), "a"), file(tmp.path(), "c"));
        b.create(&a, "x").unwrap();
        assert!(b.create(&a, "y").is_err(), "no overwriting");
        b.create(&c, "c").unwrap();
        assert!(b.rename(&a, &c).is_err());

        // Waiting edits go to the old name before the rename.
        b.watch(&a, "x");
        b.save(a.clone(), "edited".into());
        let renamed = file(tmp.path(), "A");
        b.rename(&a, &renamed).unwrap();
        assert_eq!(Backend::list(tmp.path()).unwrap(), ["A", "c"]);
        assert_eq!(fs::read_to_string(&renamed).unwrap(), "edited");
        assert_eq!(b.check(), None, "still watching it under the new name");

        // Deleting drops edits waiting for it.
        b.save(renamed.clone(), "lost".into());
        b.delete(&renamed).unwrap();
        assert!(!b.is_saving());
        assert_eq!(Backend::list(tmp.path()).unwrap(), ["c"]);
    }

    #[test]
    fn names() {
        assert_eq!(valid_name("  Orders  "), Ok("Orders".into()));
        assert!(valid_name(" ").is_err());
        assert!(valid_name("a/b").is_err());
        assert!(valid_name(".x").is_err());
        let names = ["Untitled".to_string(), "untitled 2".to_string()];
        assert_eq!(unique_name("Untitled", &names), "Untitled 3");
        assert_eq!(unique_name("Other", &names), "Other");
    }
}
