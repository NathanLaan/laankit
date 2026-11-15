//! Importing: copying the photos on a device into one of your folders.
//!
//! - A [`Source`] is a device's files: [`FolderSource`] for a drive, and on
//!   Windows a portable device (see [`crate::portable`]). [`Source::list`]
//!   finds everything worth importing, leaving out hidden files and the
//!   camera's own index files.
//! - [`run`] copies the chosen files into one folder, the device's own
//!   folders flattened. Nothing is overwritten: a file that's already there
//!   with the same contents is skipped, and one with the same name and
//!   different contents is copied as `name (2).jpg`. Each copy is written
//!   under a temporary name, checked against the original's size, given
//!   the original's modification time, and only then put in place.
//! - [`finish`] writes the device's [`Record`] (what has been imported from
//!   it, so the next import can offer only what's new) and the journal of
//!   this import, which [`undo`] uses.
//! - [`delete_copied`] removes from the device the files this import
//!   copied. It's only ever called after the copy, when you ask.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::devices::{Location, Volume};
use crate::media::MediaKind;
use crate::merge::unique;
use crate::natural;
use crate::scan::{self, file_name};

/// The journal of the last import, in the imports folder.
const JOURNAL: &str = "last-import.json";

/// A file on a device.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    /// What the source opens it by: a path, or a device's object id.
    pub id: String,
    pub name: String,
    /// Its place on the device, with `/` between folders.
    pub rel: String,
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub kind: MediaKind,
}

/// A device's files.
pub trait Source {
    /// Every file worth importing, in the device's own order. `found` is
    /// told the count so far.
    fn list(&mut self, cancel: &AtomicBool, found: &mut dyn FnMut(usize)) -> Result<Vec<Item>>;
    fn open(&mut self, item: &Item) -> io::Result<Box<dyn Read + '_>>;
    fn delete(&mut self, item: &Item) -> io::Result<()>;
    /// Whether the device is still there.
    fn present(&self) -> bool {
        true
    }
}

/// Opens `volume`'s files. A portable device must be used on the thread
/// that opened it.
pub fn open_source(volume: &Volume) -> Result<Box<dyn Source>> {
    match &volume.location {
        Location::Folder(path) => Ok(Box::new(FolderSource::new(path))),
        #[cfg(windows)]
        Location::Portable(id) => Ok(Box::new(crate::portable::PortableSource::open(id)?)),
        #[cfg(not(windows))]
        Location::Portable(_) => anyhow::bail!("media devices are only read this way on Windows"),
    }
}

/// Folders that hold nothing of yours.
pub fn skipped_folder(name: &str) -> bool {
    name.starts_with('.')
        || ["System Volume Information", "$RECYCLE.BIN", "LOST.DIR"]
            .iter()
            .any(|s| s.eq_ignore_ascii_case(name))
}

/// A camera's own index and housekeeping files, and hidden files.
pub fn skipped_file(name: &str) -> bool {
    if name.starts_with('.') {
        return true;
    }
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    [
        "thm", "ctg", "bdm", "cpi", "mpl", "bnp", "inp", "int", "ind",
    ]
    .contains(&ext.as_str())
}

/// A drive, or any folder.
pub struct FolderSource {
    root: PathBuf,
}

impl FolderSource {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_owned(),
        }
    }

    fn walk(
        &self,
        dir: &Path,
        out: &mut Vec<Item>,
        cancel: &AtomicBool,
        found: &mut dyn FnMut(usize),
    ) {
        let Ok(rd) = fs::read_dir(dir) else { return };
        let mut entries: Vec<(PathBuf, fs::Metadata)> = rd
            .flatten()
            .filter_map(|e| {
                let path = e.path();
                let meta = fs::symlink_metadata(&path).ok()?;
                (!scan::is_hidden(&path, &meta)).then_some((path, meta))
            })
            .collect();
        entries.sort_by(|a, b| natural::compare(&file_name(&a.0), &file_name(&b.0)));
        let mut folders = Vec::new();
        for (path, meta) in entries {
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            let name = file_name(&path);
            if meta.is_dir() {
                if !skipped_folder(&name) {
                    folders.push(path);
                }
            } else if meta.is_file() && !skipped_file(&name) {
                let rel = path
                    .strip_prefix(&self.root)
                    .unwrap_or(&path)
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                out.push(Item {
                    id: path.to_string_lossy().into_owned(),
                    kind: MediaKind::of(&path),
                    name,
                    rel,
                    size: meta.len(),
                    modified: meta.modified().ok(),
                });
                found(out.len());
            }
        }
        for f in folders {
            self.walk(&f, out, cancel, found);
        }
    }
}

impl Source for FolderSource {
    fn list(&mut self, cancel: &AtomicBool, found: &mut dyn FnMut(usize)) -> Result<Vec<Item>> {
        fs::read_dir(&self.root).with_context(|| format!("reading {}", self.root.display()))?;
        let mut out = Vec::new();
        self.walk(&self.root.clone(), &mut out, cancel, found);
        Ok(out)
    }

    fn open(&mut self, item: &Item) -> io::Result<Box<dyn Read + '_>> {
        Ok(Box::new(File::open(&item.id)?))
    }

    fn delete(&mut self, item: &Item) -> io::Result<()> {
        fs::remove_file(&item.id)
    }

    fn present(&self) -> bool {
        self.root.is_dir()
    }
}

/// Which kinds of file to import. Photos always are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    pub videos: bool,
    /// Camera RAW, HEIC, and anything else.
    pub others: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            videos: true,
            others: true,
        }
    }
}

impl Options {
    pub fn wants(self, kind: MediaKind) -> bool {
        match kind {
            MediaKind::Photo => true,
            MediaKind::Video => self.videos,
            MediaKind::Other => self.others,
        }
    }
}

/// Whether a device holds something to offer: a photo or a video.
pub fn worth_offering(items: &[Item]) -> bool {
    items.iter().any(|i| i.kind != MediaKind::Other)
}

/// The files to import: the kinds `options` wants, and with `new_only`,
/// the ones not in `record`.
pub fn choose(
    items: &[Item],
    options: Options,
    record: Option<&Record>,
    new_only: bool,
) -> Vec<Item> {
    items
        .iter()
        .filter(|i| options.wants(i.kind))
        .filter(|i| !(new_only && record.is_some_and(|r| r.contains(i))))
        .cloned()
        .collect()
}

fn secs(t: Option<SystemTime>) -> Option<i64> {
    let t = t?;
    Some(match t.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(e) => -(e.duration().as_secs() as i64),
    })
}

/// A file that has been imported from a device.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Seen {
    pub rel: String,
    pub size: u64,
    /// Seconds since 1970.
    pub modified: Option<i64>,
}

impl Seen {
    fn of(item: &Item) -> Self {
        Self {
            rel: item.rel.clone(),
            size: item.size,
            modified: secs(item.modified),
        }
    }
}

/// What has been imported from one device.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub key: String,
    pub name: String,
    pub files: Vec<Seen>,
}

fn fnv(text: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

impl Record {
    fn file(dir: &Path, key: &str) -> PathBuf {
        dir.join(format!("device-{}.json", fnv(key)))
    }

    /// The record for the device with `key`, if it has one.
    pub fn load(dir: &Path, key: &str) -> Option<Record> {
        let text = fs::read_to_string(Self::file(dir, key)).ok()?;
        let r: Record = serde_json::from_str(&text).ok()?;
        (r.key == key).then_some(r)
    }

    pub fn save(&self, dir: &Path) -> Result<()> {
        fs::create_dir_all(dir)?;
        let file = Self::file(dir, &self.key);
        let tmp = file.with_extension("tmp");
        fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        fs::rename(&tmp, &file)?;
        Ok(())
    }

    pub fn contains(&self, item: &Item) -> bool {
        let seen = Seen::of(item);
        self.files.contains(&seen)
    }

    fn add(&mut self, items: impl Iterator<Item = Seen>) {
        let mut have: HashSet<Seen> = self.files.iter().cloned().collect();
        for s in items {
            if have.insert(s.clone()) {
                self.files.push(s);
            }
        }
    }
}

/// Every device with a record, by name.
pub fn records(dir: &Path) -> Vec<Record> {
    let mut out: Vec<Record> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            n.starts_with("device-") && n.ends_with(".json")
        })
        .filter_map(|e| serde_json::from_str(&fs::read_to_string(e.path()).ok()?).ok())
        .collect();
    out.sort_by(|a, b| natural::compare(&a.name, &b.name));
    out
}

/// Forgets what has been imported from the device with `key`.
pub fn forget(dir: &Path, key: &str) -> io::Result<()> {
    match fs::remove_file(Record::file(dir, key)) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// How far an import has got.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Progress {
    pub done: usize,
    pub total: usize,
    pub bytes: u64,
    pub total_bytes: u64,
    /// The file being copied.
    pub name: String,
}

/// A file an import copied.
#[derive(Clone, Debug, PartialEq)]
pub struct Copied {
    pub item: Item,
    pub dest: PathBuf,
}

/// How an import went.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Outcome {
    pub copied: Vec<Copied>,
    /// Already in the destination with the same contents.
    pub already: Vec<Item>,
    /// Files that weren't copied, and why.
    pub failed: Vec<(String, String)>,
    pub cancelled: bool,
    /// The device went away part way through.
    pub removed: bool,
}

/// Whether `path` holds exactly what `item` does.
fn same_as(source: &mut dyn Source, item: &Item, path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() || meta.len() != item.size {
        return false;
    }
    let (Ok(mut a), Ok(mut b)) = (source.open(item), File::open(path)) else {
        return false;
    };
    let (mut ba, mut bb) = (vec![0u8; 64 * 1024], vec![0u8; 64 * 1024]);
    loop {
        let n = match a.read(&mut ba) {
            Ok(n) => n,
            Err(_) => return false,
        };
        if n == 0 {
            // The same size, and every byte matched.
            return true;
        }
        if b.read_exact(&mut bb[..n]).is_err() || ba[..n] != bb[..n] {
            return false;
        }
    }
}

enum CopyEnd {
    Done,
    Cancelled,
}

/// Copies `item` to `to`, which must not exist, by way of a hidden
/// temporary file beside it.
fn copy_item(
    source: &mut dyn Source,
    item: &Item,
    to: &Path,
    cancel: &AtomicBool,
    on_bytes: &mut dyn FnMut(u64),
) -> io::Result<CopyEnd> {
    let dir = to.parent().unwrap_or(Path::new("."));
    let tmp = dir.join(format!(".{}.{}.part", file_name(to), std::process::id()));
    let result = (|| -> io::Result<CopyEnd> {
        let mut reader = source.open(item)?;
        let mut out = File::create(&tmp)?;
        let mut buf = vec![0u8; 256 * 1024];
        let mut written = 0u64;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Ok(CopyEnd::Cancelled);
            }
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n])?;
            written += n as u64;
            on_bytes(n as u64);
        }
        out.flush()?;
        if written != item.size {
            return Err(io::Error::other(
                "the copy isn't the same size as the original",
            ));
        }
        if let Some(m) = item.modified {
            let _ = out.set_modified(m);
        }
        drop(out);
        // A hard link never replaces a file. Where the file system has
        // none (FAT), the name was free a moment ago: rename.
        if fs::hard_link(&tmp, to).is_ok() {
            let _ = fs::remove_file(&tmp);
        } else if fs::symlink_metadata(to).is_ok() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "a file with that name is already there",
            ));
        } else {
            fs::rename(&tmp, to)?;
        }
        Ok(CopyEnd::Done)
    })();
    if !matches!(result, Ok(CopyEnd::Done)) {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// Copies `items` from `source` into `dest`. Stops between or within
/// files when `cancel` is set, leaving no part of a file behind.
pub fn run(
    source: &mut dyn Source,
    items: &[Item],
    dest: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(&Progress),
) -> Outcome {
    let mut out = Outcome::default();
    let mut state = Progress {
        total: items.len(),
        total_bytes: items.iter().map(|i| i.size).sum(),
        ..Progress::default()
    };
    if let Err(e) = fs::create_dir_all(dest) {
        out.failed = items
            .iter()
            .map(|i| (i.name.clone(), format!("the folder can't be used: {e}")))
            .collect();
        return out;
    }
    let reserved = HashSet::new();
    for (index, item) in items.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            out.cancelled = true;
            break;
        }
        state.name = item.name.clone();
        progress(&state);
        let before = state.bytes;
        let first = dest.join(&item.name);
        if fs::symlink_metadata(&first).is_ok() && same_as(source, item, &first) {
            out.already.push(item.clone());
        } else {
            let to = unique(dest, &item.name, &reserved);
            let mut on_bytes = |n: u64| {
                state.bytes += n;
                progress(&state);
            };
            match copy_item(source, item, &to, cancel, &mut on_bytes) {
                Ok(CopyEnd::Done) => out.copied.push(Copied {
                    item: item.clone(),
                    dest: to,
                }),
                Ok(CopyEnd::Cancelled) => {
                    out.cancelled = true;
                    break;
                }
                Err(e) => {
                    if !source.present() {
                        out.removed = true;
                        out.failed.extend(
                            items[index..]
                                .iter()
                                .map(|i| (i.name.clone(), "the device was removed".to_owned())),
                        );
                        break;
                    }
                    out.failed.push((item.name.clone(), e.to_string()));
                }
            }
        }
        state.bytes = before + item.size;
        state.done = index + 1;
        progress(&state);
    }
    out
}

/// A file in an import's journal.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JournalFile {
    pub path: PathBuf,
    pub rel: String,
    pub size: u64,
    pub modified: Option<i64>,
}

/// The journal of the last import.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Journal {
    /// RFC 3339, local time.
    pub time: String,
    pub key: String,
    pub device: String,
    pub dest: PathBuf,
    /// The copied files were deleted from the device, so it can't be undone.
    pub deleted: bool,
    pub files: Vec<JournalFile>,
}

/// Writes the device's record and the journal of this import.
pub fn finish(dir: &Path, volume: &Volume, dest: &Path, outcome: &Outcome) -> Result<()> {
    let mut record = Record::load(dir, &volume.key).unwrap_or_else(|| Record {
        key: volume.key.clone(),
        ..Record::default()
    });
    record.name = volume.name.clone();
    record.add(
        outcome
            .copied
            .iter()
            .map(|c| Seen::of(&c.item))
            .chain(outcome.already.iter().map(Seen::of)),
    );
    record.save(dir)?;
    if outcome.copied.is_empty() {
        return Ok(());
    }
    let journal = Journal {
        time: chrono::Local::now().to_rfc3339(),
        key: volume.key.clone(),
        device: volume.name.clone(),
        dest: dest.to_owned(),
        deleted: false,
        files: outcome
            .copied
            .iter()
            .map(|c| JournalFile {
                path: c.dest.clone(),
                rel: c.item.rel.clone(),
                size: c.item.size,
                modified: secs(c.item.modified),
            })
            .collect(),
    };
    write_journal(dir, &journal)
}

fn write_journal(dir: &Path, journal: &Journal) -> Result<()> {
    fs::create_dir_all(dir)?;
    let file = dir.join(JOURNAL);
    let tmp = file.with_extension("tmp");
    fs::write(&tmp, serde_json::to_string_pretty(journal)?)?;
    fs::rename(&tmp, &file)?;
    Ok(())
}

/// The last import, if there's one to undo.
pub fn last(dir: &Path) -> Option<Journal> {
    serde_json::from_str(&fs::read_to_string(dir.join(JOURNAL)).ok()?).ok()
}

/// Removes from the device the files `outcome` copied. Returns how many
/// went, and the ones that didn't with the reason.
pub fn delete_copied(
    source: &mut dyn Source,
    outcome: &Outcome,
    dir: &Path,
    progress: &mut dyn FnMut(usize, usize),
) -> (usize, Vec<(String, String)>) {
    let total = outcome.copied.len();
    let mut failed = Vec::new();
    let mut deleted = 0;
    for (i, c) in outcome.copied.iter().enumerate() {
        // Only a file whose copy is still in place is deleted.
        let copy_ok = fs::metadata(&c.dest).is_ok_and(|m| m.len() == c.item.size);
        if !copy_ok {
            failed.push((c.item.name.clone(), "its copy is missing".to_owned()));
        } else {
            match source.delete(&c.item) {
                Ok(()) => deleted += 1,
                Err(e) => failed.push((c.item.name.clone(), e.to_string())),
            }
        }
        progress(i + 1, total);
    }
    if deleted > 0 {
        if let Some(mut j) = last(dir) {
            j.deleted = true;
            if let Err(e) = write_journal(dir, &j) {
                log::warn!("marking the import's journal: {e:#}");
            }
        }
    }
    (deleted, failed)
}

/// How an undo went.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Undone {
    pub removed: usize,
    /// Files left because they changed after the import.
    pub kept: usize,
    pub dest: PathBuf,
}

/// Removes the files the last import copied, leaving any that have changed
/// since, and takes them out of the device's record so they count as new.
pub fn undo(dir: &Path) -> Result<Undone> {
    let journal = last(dir).context("there's no import to undo")?;
    if journal.deleted {
        anyhow::bail!("the photos were deleted from the device, so the import can't be undone");
    }
    let mut out = Undone {
        dest: journal.dest.clone(),
        ..Undone::default()
    };
    let mut gone = HashSet::new();
    for f in &journal.files {
        let Ok(meta) = fs::symlink_metadata(&f.path) else {
            // Already gone: nothing to remove, and it's new again.
            gone.insert((f.rel.clone(), f.size));
            continue;
        };
        let unchanged = meta.is_file()
            && meta.len() == f.size
            && (f.modified.is_none() || secs(meta.modified().ok()) == f.modified);
        if unchanged && fs::remove_file(&f.path).is_ok() {
            out.removed += 1;
            gone.insert((f.rel.clone(), f.size));
        } else {
            out.kept += 1;
        }
    }
    if let Some(mut record) = Record::load(dir, &journal.key) {
        record
            .files
            .retain(|s| !gone.contains(&(s.rel.clone(), s.size)));
        record.save(dir)?;
    }
    fs::remove_file(dir.join(JOURNAL))?;
    Ok(out)
}

/// The free space where `path` is, in bytes.
#[cfg(unix)]
pub fn free_space(path: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut s = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: `c` is a NUL-terminated path and `s` is room for one statvfs.
    let rc = unsafe { libc::statvfs(c.as_ptr(), s.as_mut_ptr()) };
    if rc != 0 {
        return None;
    }
    // SAFETY: statvfs returned 0, so it filled `s`.
    let s = unsafe { s.assume_init() };
    // The field types differ between systems.
    #[allow(clippy::unnecessary_cast)]
    let free = s.f_bavail as u64 * s.f_frsize as u64;
    Some(free)
}

/// The free space where `path` is, in bytes.
#[cfg(windows)]
pub fn free_space(path: &Path) -> Option<u64> {
    crate::portable::free_space(path)
}

#[cfg(not(any(unix, windows)))]
pub fn free_space(_path: &Path) -> Option<u64> {
    None
}

/// A time for the dialog's list: `2026-11-14 09:30`.
pub fn date_text(t: Option<SystemTime>) -> String {
    match t {
        Some(t) => chrono::DateTime::<chrono::Local>::from(t)
            .format("%Y-%m-%d %H:%M")
            .to_string(),
        None => String::new(),
    }
}

/// A time from seconds since 1970, as devices report it.
pub fn time_from_secs(secs: i64) -> SystemTime {
    if secs >= 0 {
        UNIX_EPOCH + Duration::from_secs(secs as u64)
    } else {
        UNIX_EPOCH - Duration::from_secs(secs.unsigned_abs())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, bytes: &[u8]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    /// A card as a camera leaves it.
    fn card(root: &Path) {
        write(&root.join("DCIM/100MSDCF/DSC0001.JPG"), b"one");
        write(&root.join("DCIM/100MSDCF/DSC0002.JPG"), b"two!");
        write(&root.join("DCIM/100MSDCF/DSC0002.ARW"), b"raw");
        write(&root.join("DCIM/101MSDCF/DSC0001.JPG"), b"other one");
        write(&root.join("DCIM/101MSDCF/.hidden.jpg"), b"x");
        write(&root.join("MP_ROOT/100ANV01/MAH0001.MP4"), b"video");
        write(&root.join("MP_ROOT/100ANV01/MAH0001.THM"), b"thumb");
        write(&root.join("PRIVATE/AVCHD/BDMV/INDEX.BDM"), b"index");
        write(&root.join("System Volume Information/x.jpg"), b"x");
        write(&root.join(".Trashes/gone.jpg"), b"x");
    }

    fn list(root: &Path) -> Vec<Item> {
        FolderSource::new(root)
            .list(&AtomicBool::new(false), &mut |_| {})
            .unwrap()
    }

    fn import(root: &Path, items: &[Item], dest: &Path) -> Outcome {
        let mut src = FolderSource::new(root);
        run(&mut src, items, dest, &AtomicBool::new(false), &mut |_| {})
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }

    #[test]
    fn lists_what_is_worth_importing() {
        let tmp = tempfile::tempdir().unwrap();
        card(tmp.path());
        let items = list(tmp.path());
        let rels: Vec<&str> = items.iter().map(|i| i.rel.as_str()).collect();
        assert_eq!(
            rels,
            [
                "DCIM/100MSDCF/DSC0001.JPG",
                "DCIM/100MSDCF/DSC0002.ARW",
                "DCIM/100MSDCF/DSC0002.JPG",
                "DCIM/101MSDCF/DSC0001.JPG",
                "MP_ROOT/100ANV01/MAH0001.MP4",
            ]
        );
        assert!(worth_offering(&items));
        let all = Options::default();
        assert_eq!(choose(&items, all, None, false).len(), 5);
        let photos = Options {
            videos: false,
            others: false,
        };
        assert_eq!(choose(&items, photos, None, false).len(), 3);
        // A stick of documents isn't offered.
        let docs = tempfile::tempdir().unwrap();
        write(&docs.path().join("report.docx"), b"x");
        assert!(!worth_offering(&list(docs.path())));
    }

    #[test]
    fn copies_flat_without_overwriting() {
        let tmp = tempfile::tempdir().unwrap();
        let (cardp, dest) = (tmp.path().join("card"), tmp.path().join("dest"));
        card(&cardp);
        // Already there with the same contents, and a different file of
        // the same name as another.
        write(&dest.join("DSC0002.JPG"), b"two!");
        write(&dest.join("MAH0001.MP4"), b"not the same");
        let t = SystemTime::now() - Duration::from_secs(86_400);
        File::options()
            .write(true)
            .open(cardp.join("DCIM/100MSDCF/DSC0001.JPG"))
            .unwrap()
            .set_modified(t)
            .unwrap();
        let items = list(&cardp);
        let mut seen = Vec::new();
        let mut src = FolderSource::new(&cardp);
        let out = run(&mut src, &items, &dest, &AtomicBool::new(false), &mut |p| {
            seen.push(p.clone())
        });
        assert_eq!(out.copied.len(), 4);
        assert_eq!(out.already.len(), 1);
        assert!(out.failed.is_empty() && !out.cancelled);
        assert_eq!(
            names(&dest),
            [
                "DSC0001 (2).JPG",
                "DSC0001.JPG",
                "DSC0002.ARW",
                "DSC0002.JPG",
                "MAH0001 (2).MP4",
                "MAH0001.MP4",
            ]
        );
        // The two cameras' folders restart numbering: both are kept.
        assert_eq!(fs::read(dest.join("DSC0001.JPG")).unwrap(), b"one");
        assert_eq!(
            fs::read(dest.join("DSC0001 (2).JPG")).unwrap(),
            b"other one"
        );
        assert_eq!(fs::read(dest.join("MAH0001.MP4")).unwrap(), b"not the same");
        // The modification time is the original's.
        let copied = fs::metadata(dest.join("DSC0001.JPG")).unwrap().modified();
        assert_eq!(secs(copied.ok()), secs(Some(t)));
        // The card is untouched.
        assert_eq!(list(&cardp).len(), 5);
        let last = seen.last().unwrap();
        assert_eq!((last.done, last.total), (5, 5));
        assert_eq!(last.bytes, last.total_bytes);
    }

    #[test]
    fn a_cancelled_or_failed_copy_leaves_nothing_behind() {
        let tmp = tempfile::tempdir().unwrap();
        let (cardp, dest) = (tmp.path().join("card"), tmp.path().join("dest"));
        card(&cardp);
        let items = list(&cardp);
        let cancel = AtomicBool::new(false);
        let mut src = FolderSource::new(&cardp);
        let out = run(&mut src, &items, &dest, &cancel, &mut |p| {
            if p.done == 2 {
                cancel.store(true, Ordering::Relaxed);
            }
        });
        assert!(out.cancelled);
        assert_eq!(out.copied.len(), 2);
        assert_eq!(names(&dest), ["DSC0001.JPG", "DSC0002.ARW"]);

        // A file that goes missing fails on its own; the rest are copied.
        let dest2 = tmp.path().join("dest2");
        fs::remove_file(cardp.join("DCIM/100MSDCF/DSC0002.ARW")).unwrap();
        let out = import(&cardp, &items, &dest2);
        assert_eq!(out.copied.len(), 4);
        assert_eq!(out.failed.len(), 1);
        assert_eq!(out.failed[0].0, "DSC0002.ARW");
        assert!(!names(&dest2).iter().any(|n| n.ends_with(".part")));

        // The device pulled out: the rest are reported, not tried.
        let dest3 = tmp.path().join("dest3");
        fs::remove_dir_all(&cardp).unwrap();
        let out = import(&cardp, &items, &dest3);
        assert!(out.removed);
        assert_eq!(out.failed.len(), 5);
        assert!(names(&dest3).is_empty());
    }

    #[test]
    fn the_record_offers_only_what_is_new_and_undo_forgets() {
        let tmp = tempfile::tempdir().unwrap();
        let (cardp, dest, dir) = (
            tmp.path().join("card"),
            tmp.path().join("dest"),
            tmp.path().join("imports"),
        );
        card(&cardp);
        let volume = Volume::folder(&cardp, "CARD", "uuid:1234");
        assert!(Record::load(&dir, &volume.key).is_none());
        let items = list(&cardp);
        let out = import(&cardp, &items, &dest);
        finish(&dir, &volume, &dest, &out).unwrap();

        let record = Record::load(&dir, &volume.key).unwrap();
        assert_eq!(record.files.len(), 5);
        assert_eq!(records(&dir).len(), 1);
        assert_eq!(records(&dir)[0].name, "CARD");
        // A new photo on the card is the only one offered.
        write(&cardp.join("DCIM/101MSDCF/DSC0009.JPG"), b"new");
        let items = list(&cardp);
        let new = choose(&items, Options::default(), Some(&record), true);
        assert_eq!(new.len(), 1);
        assert_eq!(new[0].name, "DSC0009.JPG");

        // Undo removes the copies, except one changed since.
        fs::write(dest.join("DSC0002.JPG"), b"edited since").unwrap();
        let journal = last(&dir).unwrap();
        assert_eq!(journal.files.len(), 5);
        let undone = undo(&dir).unwrap();
        assert_eq!((undone.removed, undone.kept), (4, 1));
        assert_eq!(names(&dest), ["DSC0002.JPG"]);
        assert!(last(&dir).is_none());
        let record = Record::load(&dir, &volume.key).unwrap();
        assert_eq!(record.files.len(), 1);

        forget(&dir, &volume.key).unwrap();
        assert!(records(&dir).is_empty());
        forget(&dir, &volume.key).unwrap();
    }

    #[test]
    fn deleting_from_the_device_follows_a_checked_copy() {
        let tmp = tempfile::tempdir().unwrap();
        let (cardp, dest, dir) = (
            tmp.path().join("card"),
            tmp.path().join("dest"),
            tmp.path().join("imports"),
        );
        card(&cardp);
        let volume = Volume::folder(&cardp, "CARD", "k");
        let items = list(&cardp);
        let out = import(&cardp, &items, &dest);
        finish(&dir, &volume, &dest, &out).unwrap();
        // One copy has gone: its original stays on the card.
        fs::remove_file(dest.join("DSC0002.ARW")).unwrap();
        let mut src = FolderSource::new(&cardp);
        let (deleted, failed) = delete_copied(&mut src, &out, &dir, &mut |_, _| {});
        assert_eq!(deleted, 4);
        assert_eq!(failed.len(), 1);
        let left = list(&cardp);
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].name, "DSC0002.ARW");
        // It can't be undone now.
        assert!(last(&dir).unwrap().deleted);
        assert!(undo(&dir).is_err());
    }

    #[test]
    fn free_space_is_known_for_a_real_folder() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(free_space(tmp.path()).is_some_and(|n| n > 0));
        assert_eq!(
            date_text(Some(time_from_secs(0))).len(),
            "1970-01-01 00:00".len()
        );
    }
}
