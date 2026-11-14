//! Merging one folder (the source) into another (the destination): moving
//! everything in it, subfolders included, and undoing that.
//!
//! - [`plan`] works out every move before anything changes: where each file
//!   goes, which names clash (the moved file is renamed `name (2).jpg`),
//!   and which files are identical to one already there (set aside in a
//!   duplicates folder rather than kept twice). Nothing is overwritten.
//! - [`run`] moves the files one at a time, writing each move to a journal
//!   before making it, so even an interrupted merge can be undone. A move
//!   to another drive copies, checks the copy's size, then deletes the
//!   original. Empty folders left in the source are removed, and the
//!   source too when asked.
//! - [`undo`] moves the files back, using the journal, and puts back the
//!   folders the merge removed.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};

use crate::media::MediaKind;
use crate::scan::file_name;

/// The name of the folder identical files are set aside in.
pub const DUPLICATES_DIR: &str = ".medialiner-duplicates";

/// Why a merge can't go ahead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    Same,
    /// The destination is inside the source.
    IntoInside,
    /// The destination is the source's own folder: nothing would move.
    IntoParent,
    SourceMissing,
    DestMissing,
}

impl Refusal {
    pub fn message(self) -> &'static str {
        match self {
            Refusal::Same => "A folder can't be merged into itself.",
            Refusal::IntoInside => "A folder can't be merged into a folder inside it.",
            Refusal::IntoParent => "That's the folder it's already in.",
            Refusal::SourceMissing => "The folder to merge isn't there any more.",
            Refusal::DestMissing => "The destination folder isn't there any more.",
        }
    }
}

/// Whether `source` can be merged into `dest`.
pub fn check(source: &Path, dest: &Path) -> Result<(), Refusal> {
    if source == dest {
        return Err(Refusal::Same);
    }
    if dest.starts_with(source) {
        return Err(Refusal::IntoInside);
    }
    if source.parent() == Some(dest) {
        return Err(Refusal::IntoParent);
    }
    if !source.is_dir() {
        return Err(Refusal::SourceMissing);
    }
    if !dest.is_dir() {
        return Err(Refusal::DestMissing);
    }
    Ok(())
}

/// One file to move.
#[derive(Clone, Debug, PartialEq)]
pub struct Move {
    pub from: PathBuf,
    pub to: PathBuf,
    /// Identical to a file already in the destination: set aside.
    pub duplicate: bool,
    /// Its name clashed, so it moves under a new one.
    pub renamed: bool,
}

/// Everything a merge will do, for the dialog to show first.
#[derive(Clone, Debug, PartialEq)]
pub struct MergePlan {
    pub source: PathBuf,
    pub dest: PathBuf,
    pub duplicates_dir: PathBuf,
    pub moves: Vec<Move>,
    pub photos: usize,
    pub others: usize,
    /// Subfolders of the source, at every level.
    pub folders: usize,
    pub bytes: u64,
    /// Files renamed because the name was taken.
    pub clashes: usize,
    /// Files set aside because an identical one is already there.
    pub duplicates: usize,
}

/// Works out the merge of `source` into `dest`. Identical files go under
/// `duplicates_dir`, in a folder named after the source.
pub fn plan(source: &Path, dest: &Path, duplicates_dir: &Path) -> Result<MergePlan> {
    if let Err(r) = check(source, dest) {
        bail!(r.message());
    }
    let mut plan = MergePlan {
        source: source.to_owned(),
        dest: dest.to_owned(),
        duplicates_dir: duplicates_dir.to_owned(),
        moves: Vec::new(),
        photos: 0,
        others: 0,
        folders: 0,
        bytes: 0,
        clashes: 0,
        duplicates: 0,
    };
    let mut reserved = HashSet::new();
    let dup_root = duplicates_dir.join(file_name(source));
    walk(source, dest, &dup_root, &mut plan, &mut reserved)?;
    Ok(plan)
}

fn taken(path: &Path, reserved: &HashSet<PathBuf>) -> bool {
    reserved.contains(path) || fs::symlink_metadata(path).is_ok()
}

/// `dir/name`, or `dir/name (2).ext`, `(3)`, … when that's taken.
fn unique(dir: &Path, name: &str, reserved: &HashSet<PathBuf>) -> PathBuf {
    let first = dir.join(name);
    if !taken(&first, reserved) {
        return first;
    }
    let p = Path::new(name);
    let (stem, ext) = match (p.file_stem(), p.extension()) {
        (Some(s), Some(e)) if !name.starts_with('.') || s != name => (
            s.to_string_lossy().into_owned(),
            format!(".{}", e.to_string_lossy()),
        ),
        _ => (name.to_owned(), String::new()),
    };
    (2..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|c| !taken(c, reserved))
        .expect("a free name")
}

fn walk(
    src: &Path,
    dst: &Path,
    dup: &Path,
    plan: &mut MergePlan,
    reserved: &mut HashSet<PathBuf>,
) -> Result<()> {
    let mut entries: Vec<PathBuf> = fs::read_dir(src)
        .with_context(|| format!("reading {}", src.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| *p != plan.duplicates_dir)
        .collect();
    entries.sort();
    for from in entries {
        let name = file_name(&from);
        let meta = fs::symlink_metadata(&from)?;
        if meta.is_dir() {
            plan.folders += 1;
            let target = dst.join(&name);
            let target = if target.is_dir() && !fs::symlink_metadata(&target)?.is_symlink() {
                target
            } else {
                unique(dst, &name, reserved)
            };
            reserved.insert(target.clone());
            walk(&from, &target, &dup.join(&name), plan, reserved)?;
            continue;
        }
        plan.bytes += meta.len();
        if MediaKind::of(&from).is_photo() {
            plan.photos += 1;
        } else {
            plan.others += 1;
        }
        let target = dst.join(&name);
        let mv = if !taken(&target, reserved) {
            Move {
                from,
                to: target,
                duplicate: false,
                renamed: false,
            }
        } else if meta.is_file() && same_contents(&from, &target) {
            plan.duplicates += 1;
            Move {
                to: unique(dup, &name, reserved),
                from,
                duplicate: true,
                renamed: false,
            }
        } else {
            plan.clashes += 1;
            Move {
                to: unique(dst, &name, reserved),
                from,
                duplicate: false,
                renamed: true,
            }
        };
        reserved.insert(mv.to.clone());
        plan.moves.push(mv);
    }
    Ok(())
}

/// Whether two files have the same size and bytes.
fn same_contents(a: &Path, b: &Path) -> bool {
    let (Ok(ma), Ok(mb)) = (fs::metadata(a), fs::metadata(b)) else {
        return false;
    };
    if !mb.is_file() || ma.len() != mb.len() {
        return false;
    }
    let (Ok(fa), Ok(fb)) = (File::open(a), File::open(b)) else {
        return false;
    };
    let (mut ra, mut rb) = (BufReader::new(fa), BufReader::new(fb));
    let (mut ba, mut bb) = ([0u8; 64 * 1024], [0u8; 64 * 1024]);
    loop {
        let n = match ra.read(&mut ba) {
            Ok(n) => n,
            Err(_) => return false,
        };
        if n == 0 {
            return true;
        }
        if rb.read_exact(&mut bb[..n]).is_err() || ba[..n] != bb[..n] {
            return false;
        }
    }
}

/// Moves one file without overwriting: a rename, or across drives (or
/// when `force_copy`) a copy, a size check, and a delete. The copy keeps
/// the original's modification time.
pub fn move_file(from: &Path, to: &Path, force_copy: bool) -> io::Result<()> {
    if fs::symlink_metadata(to).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "a file with that name is already there",
        ));
    }
    if !force_copy {
        match fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == io::ErrorKind::CrossesDevices => {}
            Err(e) => return Err(e),
        }
    }
    let meta = fs::symlink_metadata(from)?;
    if meta.is_symlink() {
        return Err(io::Error::other("a link can't be moved to another drive"));
    }
    if let Err(e) = fs::copy(from, to) {
        let _ = fs::remove_file(to);
        return Err(e);
    }
    let copied = fs::metadata(to)?.len();
    if copied != meta.len() {
        let _ = fs::remove_file(to);
        return Err(io::Error::other(
            "the copy isn't the same size as the original",
        ));
    }
    if let (Ok(m), Ok(f)) = (meta.modified(), File::options().write(true).open(to)) {
        let _ = f.set_modified(m);
    }
    fs::remove_file(from)
}

/// How a merge or an undo went.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Outcome {
    pub moved: usize,
    pub duplicates: usize,
    /// Files that couldn't move, and why.
    pub skipped: Vec<(PathBuf, String)>,
    pub cancelled: bool,
    pub source_removed: bool,
    pub journal: PathBuf,
}

/// The journal of a merge.
struct JournalWriter {
    file: File,
}

impl JournalWriter {
    fn line(&mut self, v: Value) -> io::Result<()> {
        writeln!(self.file, "{v}")?;
        self.file.flush()?;
        self.file.sync_data()
    }
}

/// A new journal file's path in `dir`, named by the time.
fn journal_path(dir: &Path) -> PathBuf {
    let now = chrono::Local::now();
    let base = now.format("%Y%m%d-%H%M%S-%3f").to_string();
    (0..)
        .map(|n| match n {
            0 => dir.join(format!("{base}.jsonl")),
            n => dir.join(format!("{base}-{n}.jsonl")),
        })
        .find(|p| !p.exists())
        .expect("a free name")
}

/// Runs `plan`. `progress(done, total)` is called after each file. When
/// `cancel` is set, it stops after the current file. Empty folders left in
/// the source are removed, and the source itself when `remove_source`, as
/// long as nothing was skipped or cancelled.
pub fn run(
    plan: &MergePlan,
    remove_source: bool,
    journal_dir: &Path,
    cancel: &AtomicBool,
    force_copy: bool,
    mut progress: impl FnMut(usize, usize),
) -> Result<Outcome> {
    fs::create_dir_all(journal_dir)?;
    let path = journal_path(journal_dir);
    let mut journal = JournalWriter {
        file: File::create(&path)?,
    };
    journal.line(json!({
        "kind": "start",
        "source": plan.source,
        "dest": plan.dest,
        "time": chrono::Local::now().to_rfc3339(),
        "files": plan.moves.len(),
    }))?;
    let mut out = Outcome {
        journal: path,
        ..Default::default()
    };
    let total = plan.moves.len();
    let mut reserved = HashSet::new();
    for (i, mv) in plan.moves.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            out.cancelled = true;
            break;
        }
        let result = (|| -> io::Result<()> {
            let parent = mv.to.parent().unwrap_or(&plan.dest);
            make_dirs(parent, &mut journal)?;
            // Something may have taken the name since the plan.
            let to = if fs::symlink_metadata(&mv.to).is_ok() {
                unique(parent, &file_name(&mv.to), &reserved)
            } else {
                mv.to.clone()
            };
            reserved.insert(to.clone());
            journal.line(json!({ "kind": "move", "from": mv.from, "to": to }))?;
            move_file(&mv.from, &to, force_copy)
        })();
        match result {
            Ok(()) if mv.duplicate => out.duplicates += 1,
            Ok(()) => out.moved += 1,
            Err(e) => out.skipped.push((mv.from.clone(), e.to_string())),
        }
        progress(i + 1, total);
    }
    // Empty folders left behind, deepest first.
    let clean = !out.cancelled && out.skipped.is_empty();
    let mut removed = Vec::new();
    remove_empty_dirs(&plan.source, &plan.duplicates_dir, &mut removed);
    if remove_source && clean && fs::remove_dir(&plan.source).is_ok() {
        removed.push(plan.source.clone());
        out.source_removed = true;
    }
    for dir in &removed {
        journal.line(json!({ "kind": "rmdir", "path": dir }))?;
    }
    journal.line(json!({
        "kind": "end",
        "moved": out.moved,
        "duplicates": out.duplicates,
        "skipped": out.skipped.len(),
        "cancelled": out.cancelled,
    }))?;
    Ok(out)
}

/// Creates `dir` and any missing parents, journalling each one made.
fn make_dirs(dir: &Path, journal: &mut JournalWriter) -> io::Result<()> {
    if dir.is_dir() {
        return Ok(());
    }
    if let Some(parent) = dir.parent() {
        make_dirs(parent, journal)?;
    }
    journal.line(json!({ "kind": "mkdir", "path": dir }))?;
    match fs::create_dir(dir) {
        Err(e) if e.kind() != io::ErrorKind::AlreadyExists => Err(e),
        _ => Ok(()),
    }
}

/// Removes the empty folders below `dir`, deepest first (not `dir`).
fn remove_empty_dirs(dir: &Path, keep: &Path, removed: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p == keep || !e.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        remove_empty_dirs(&p, keep, removed);
        if fs::remove_dir(&p).is_ok() {
            removed.push(p);
        }
    }
}

/// A merge, read back from its journal.
#[derive(Clone, Debug, PartialEq)]
pub struct Journal {
    pub path: PathBuf,
    pub source: PathBuf,
    pub dest: PathBuf,
    /// RFC 3339, local time.
    pub time: String,
    /// `(from, to)`, in the order they were made.
    pub moves: Vec<(PathBuf, PathBuf)>,
    pub made_dirs: Vec<PathBuf>,
    pub removed_dirs: Vec<PathBuf>,
    /// It reached its end (not interrupted).
    pub finished: bool,
    pub undone: bool,
}

pub fn read_journal(path: &Path) -> Result<Journal> {
    let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut j = Journal {
        path: path.to_owned(),
        source: PathBuf::new(),
        dest: PathBuf::new(),
        time: String::new(),
        moves: Vec::new(),
        made_dirs: Vec::new(),
        removed_dirs: Vec::new(),
        finished: false,
        undone: false,
    };
    let p = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).map(PathBuf::from);
    for line in BufReader::new(file).lines() {
        // A line cut short by a crash ends the journal.
        let Ok(v) = serde_json::from_str::<Value>(&line?) else {
            break;
        };
        match v.get("kind").and_then(Value::as_str) {
            Some("start") => {
                j.source = p(&v, "source").unwrap_or_default();
                j.dest = p(&v, "dest").unwrap_or_default();
                j.time = v
                    .get("time")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
            }
            Some("move") => {
                if let (Some(from), Some(to)) = (p(&v, "from"), p(&v, "to")) {
                    j.moves.push((from, to));
                }
            }
            Some("mkdir") => j.made_dirs.extend(p(&v, "path")),
            Some("rmdir") => j.removed_dirs.extend(p(&v, "path")),
            Some("end") => j.finished = true,
            Some("undone") => j.undone = true,
            _ => {}
        }
    }
    if j.source.as_os_str().is_empty() {
        bail!("{} isn't a merge journal", path.display());
    }
    Ok(j)
}

/// The journals in `dir`, newest first.
pub fn journals(dir: &Path) -> Vec<Journal> {
    let Ok(rd) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "jsonl"))
        .collect();
    paths.sort();
    paths.reverse();
    paths.iter().filter_map(|p| read_journal(p).ok()).collect()
}

/// Keeps the newest `keep` journals in `dir` and deletes the rest.
pub fn prune(dir: &Path, keep: usize) {
    for j in journals(dir).into_iter().skip(keep) {
        let _ = fs::remove_file(&j.path);
    }
}

/// Moves a merge's files back. A file that has been moved or deleted
/// since, or whose old place is taken, is skipped; the rest go back.
pub fn undo(
    journal: &Journal,
    force_copy: bool,
    mut progress: impl FnMut(usize, usize),
) -> Result<Outcome> {
    if journal.undone {
        bail!("That merge has already been undone.");
    }
    let mut out = Outcome {
        journal: journal.path.clone(),
        ..Default::default()
    };
    for dir in journal.removed_dirs.iter().rev() {
        let _ = fs::create_dir_all(dir);
    }
    let total = journal.moves.len();
    for (i, (from, to)) in journal.moves.iter().rev().enumerate() {
        let result = if fs::symlink_metadata(to).is_err() {
            Err("it has been moved or deleted since".to_string())
        } else if fs::symlink_metadata(from).is_ok() {
            Err("another file is in its old place".to_string())
        } else {
            from.parent()
                .map_or(Ok(()), fs::create_dir_all)
                .and_then(|()| move_file(to, from, force_copy))
                .map_err(|e| e.to_string())
        };
        match result {
            Ok(()) => out.moved += 1,
            Err(why) => out.skipped.push((to.clone(), why)),
        }
        progress(i + 1, total);
    }
    for dir in journal.made_dirs.iter().rev() {
        let _ = fs::remove_dir(dir);
    }
    let mut f = fs::OpenOptions::new().append(true).open(&journal.path)?;
    writeln!(f, "{}", json!({ "kind": "undone", "restored": out.moved }))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(p: &Path, s: &str) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, s).unwrap();
    }

    fn read(p: &Path) -> String {
        fs::read_to_string(p).unwrap()
    }

    struct Setup {
        _tmp: tempfile::TempDir,
        root: PathBuf,
        src: PathBuf,
        dst: PathBuf,
        dups: PathBuf,
        journals: PathBuf,
    }

    /// Photos/Trip (the source) and Photos/Best (the destination).
    fn setup() -> Setup {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("Photos");
        let src = root.join("Trip");
        let dst = root.join("Best");
        write(&src.join("a.jpg"), "a");
        write(&src.join("clash.jpg"), "new");
        write(&src.join("same.jpg"), "same");
        write(&src.join("notes.txt"), "n");
        write(&src.join("Day 1/b.jpg"), "b");
        write(&src.join("Day 1/c.jpg"), "c");
        write(&dst.join("clash.jpg"), "old");
        write(&dst.join("same.jpg"), "same");
        write(&dst.join("Day 1/c.jpg"), "other c");
        Setup {
            dups: root.join(DUPLICATES_DIR),
            journals: tmp.path().join("merges"),
            _tmp: tmp,
            root,
            src,
            dst,
        }
    }

    #[test]
    fn refuses_itself_inside_and_parent() {
        let s = setup();
        assert_eq!(check(&s.src, &s.src), Err(Refusal::Same));
        assert_eq!(
            check(&s.src, &s.src.join("Day 1")),
            Err(Refusal::IntoInside)
        );
        assert_eq!(
            check(&s.src.join("Day 1"), &s.src),
            Err(Refusal::IntoParent)
        );
        assert_eq!(
            check(&s.src, &s.root.join("nope")),
            Err(Refusal::DestMissing)
        );
        assert_eq!(check(&s.src, &s.dst), Ok(()));
    }

    #[test]
    fn plans_clashes_duplicates_and_subfolders() {
        let s = setup();
        let p = plan(&s.src, &s.dst, &s.dups).unwrap();
        assert_eq!((p.photos, p.others, p.folders), (5, 1, 1));
        assert_eq!((p.clashes, p.duplicates), (2, 1));
        let to = |name: &str| {
            p.moves
                .iter()
                .find(|m| m.from.ends_with(name))
                .unwrap()
                .to
                .clone()
        };
        assert_eq!(to("clash.jpg"), s.dst.join("clash (2).jpg"));
        assert_eq!(to("same.jpg"), s.dups.join("Trip/same.jpg"));
        assert_eq!(to("Day 1/b.jpg"), s.dst.join("Day 1/b.jpg"));
        assert_eq!(to("Day 1/c.jpg"), s.dst.join("Day 1/c (2).jpg"));
    }

    fn merge(s: &Setup, remove: bool, force_copy: bool) -> Outcome {
        let p = plan(&s.src, &s.dst, &s.dups).unwrap();
        let mut seen = 0;
        let out = run(
            &p,
            remove,
            &s.journals,
            &AtomicBool::new(false),
            force_copy,
            |d, _| seen = d,
        )
        .unwrap();
        assert_eq!(seen, 6);
        out
    }

    fn check_merged(s: &Setup) {
        assert_eq!(read(&s.dst.join("a.jpg")), "a");
        assert_eq!(read(&s.dst.join("clash.jpg")), "old");
        assert_eq!(read(&s.dst.join("clash (2).jpg")), "new");
        assert_eq!(read(&s.dst.join("Day 1/c.jpg")), "other c");
        assert_eq!(read(&s.dst.join("Day 1/c (2).jpg")), "c");
        assert_eq!(read(&s.dups.join("Trip/same.jpg")), "same");
    }

    #[test]
    fn merges_removes_the_source_and_undoes() {
        let s = setup();
        let out = merge(&s, true, false);
        assert_eq!((out.moved, out.duplicates), (5, 1));
        assert!(out.skipped.is_empty() && out.source_removed);
        assert!(!s.src.exists());
        check_merged(&s);

        let j = read_journal(&out.journal).unwrap();
        assert!(j.finished && !j.undone);
        assert_eq!(j.moves.len(), 6);
        let back = undo(&j, false, |_, _| {}).unwrap();
        assert_eq!(back.moved, 6);
        assert_eq!(read(&s.src.join("clash.jpg")), "new");
        assert_eq!(read(&s.src.join("Day 1/c.jpg")), "c");
        assert_eq!(read(&s.dst.join("clash.jpg")), "old");
        assert!(!s.dst.join("a.jpg").exists());
        assert!(!s.dups.exists(), "the duplicates folder it made is gone");
        assert!(read_journal(&out.journal).unwrap().undone);
        assert!(undo(&read_journal(&out.journal).unwrap(), false, |_, _| {}).is_err());
    }

    #[test]
    fn across_drives_by_copying_and_keeps_the_source_when_asked() {
        let s = setup();
        let before = fs::metadata(s.src.join("a.jpg"))
            .unwrap()
            .modified()
            .unwrap();
        let out = merge(&s, false, true);
        assert!(!out.source_removed);
        assert!(s.src.is_dir());
        assert!(
            !s.src.join("Day 1").exists(),
            "empty subfolders are removed"
        );
        check_merged(&s);
        let after = fs::metadata(s.dst.join("a.jpg"))
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(before, after);
    }

    #[test]
    fn cancel_and_a_failed_move_keep_the_source_and_still_undo() {
        let s = setup();
        let p = plan(&s.src, &s.dst, &s.dups).unwrap();
        let cancel = AtomicBool::new(false);
        let out = run(&p, true, &s.journals, &cancel, false, |d, _| {
            if d == 2 {
                cancel.store(true, Ordering::Relaxed);
            }
        })
        .unwrap();
        assert!(out.cancelled && !out.source_removed);
        assert_eq!(out.moved + out.duplicates, 2);
        let j = read_journal(&out.journal).unwrap();
        assert_eq!(undo(&j, false, |_, _| {}).unwrap().moved, 2);
        assert_eq!(fs::read_dir(&s.src).unwrap().count(), 5);

        // A planned file that's gone by the time it runs is skipped.
        let p = plan(&s.src, &s.dst, &s.dups).unwrap();
        fs::remove_file(s.src.join("a.jpg")).unwrap();
        let out = run(
            &p,
            true,
            &s.journals,
            &AtomicBool::new(false),
            false,
            |_, _| {},
        )
        .unwrap();
        assert_eq!(out.skipped.len(), 1);
        assert!(s.src.exists());
    }

    #[test]
    fn an_interrupted_journal_still_undoes() {
        let s = setup();
        let out = merge(&s, true, false);
        // Cut the journal after the third move, as a crash would.
        let text = read(&out.journal);
        let kept: Vec<&str> = text.lines().take(1 + 3 + 1).collect();
        fs::write(&out.journal, kept.join("\n") + "\n{\"kind\":\"mo").unwrap();
        let j = read_journal(&out.journal).unwrap();
        assert!(!j.finished);
        assert!(undo(&j, false, |_, _| {}).unwrap().moved >= 2);
    }

    #[test]
    fn journals_newest_first_and_pruned() {
        let s = setup();
        merge(&s, false, false);
        let j = journals(&s.journals);
        assert_eq!(j.len(), 1);
        assert_eq!(j[0].source, s.src);
        prune(&s.journals, 0);
        assert!(journals(&s.journals).is_empty());
    }

    #[test]
    fn unique_names() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        write(&d.join("a.jpg"), "");
        write(&d.join("a (2).jpg"), "");
        write(&d.join(".hidden"), "");
        let r = HashSet::new();
        assert_eq!(unique(d, "a.jpg", &r), d.join("a (3).jpg"));
        assert_eq!(unique(d, ".hidden", &r), d.join(".hidden (2)"));
        assert_eq!(unique(d, "new.jpg", &r), d.join("new.jpg"));
    }
}
