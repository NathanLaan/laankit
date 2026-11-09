//! The app: owns the backend and everything the views show; routes backend
//! events into that state, and commands, view actions, and dialog results
//! to the backend.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use egui::{Frame, Ui, Vec2};
use laankit_egui::about::{AboutAction, AboutIcon, AboutModal};
use laankit_egui::commands::{Recents, Registry};
use laankit_egui::diagnostics::{self, Screenshot};
use laankit_egui::file_dialog::FileDialogs;
use laankit_egui::palette::{CommandPalette, PaletteOutcome};
use laankit_egui::theme::{self, ThemeState};
use laankit_egui::titlebar::{self, TitleBar};
use laankit_egui::toast::Toasts;
use laankit_egui::{fonts, icons, window};
use medialiner_core::info::{human_size, FileInfo};
use medialiner_core::merge;
use medialiner_core::scan::{self, file_name, FileEntry};
use medialiner_core::{reveal, Added, Library, MediaKind};
use serde_json::json;

use crate::backend::{self, Backend, Event, MergeKind, ThumbJob};
use crate::commands::{self, Act, Focus, View};
use crate::config::{self, Paths, Settings, THUMB_MAX, THUMB_MIN};
use crate::overlays::merge::{MergeAction, MergeState, Phase};
use crate::overlays::settings::{SettingsAction, SettingsProps, SettingsState};
use crate::overlays::{self, Overlay};
use crate::textures::{Textures, Thumb};
use crate::views::{
    self, FilesProps, FolderRow, GridProps, HeaderProps, InfoProps, PhotoCommand, PreviewProps,
    PreviewView, ToolbarProps, ViewAction,
};

const THEME_KEY: &str = "medialiner-theme";
const RECENTS_KEY: &str = "medialiner-recents";
const DESCRIPTION: &str =
    "Browse and organize the photos in your folders, where they are. Built on the laankit framework.";
const REPO_URL: &str = "https://github.com/NathanLaan/laankit";
/// How often the selected folder is looked at for changes (the tree's
/// interval when Include Subfolders is on).
const FOLDER_CHECK_SECS: f64 = 1.0;
/// How often the tree's folders are looked at for added or removed ones.
const TREE_CHECK_SECS: f64 = 5.0;
/// The thumbnail size step for Zoom In and Zoom Out.
const THUMB_STEP: f32 = 32.0;

/// How the app was started.
#[derive(Clone, Debug, Default)]
pub struct Launch {
    /// A folder named on the command line: added and selected.
    pub open: Option<PathBuf>,
    /// Tests: merges move by copying, as across drives.
    pub force_copy: bool,
}

/// An answer from a file dialog.
enum Picked {
    Folder(PathBuf),
}

/// The selected folder's listing.
#[derive(Default)]
struct Listing {
    loading: bool,
    in_flight: bool,
    error: Option<String>,
    asked_at: f64,
}

pub struct MediaLinerApp {
    paths: Paths,
    theme: ThemeState,
    theme_dirty: bool,
    custom_titlebar: bool,
    status_bar_visible: bool,
    thumb_size: f32,
    include_subfolders: bool,
    show_location: bool,
    folders_visible: bool,
    files_visible: bool,
    info_visible: bool,
    initialized: bool,
    launch: Launch,
    backend: Backend,
    library: Library,
    expanded: HashSet<PathBuf>,
    /// Subfolders read so far, by folder.
    children: HashMap<PathBuf, Vec<PathBuf>>,
    has_children: HashMap<PathBuf, bool>,
    tree_checked_at: f64,
    folder: Option<PathBuf>,
    files: Vec<FileEntry>,
    /// Indexes of the photos in `files`, and their thumbnail keys.
    photos: Vec<usize>,
    keys: Vec<String>,
    listing: Listing,
    selected: Option<PathBuf>,
    preview: Option<PathBuf>,
    preview_view: PreviewView,
    /// Last frame's preview image and area sizes, for keyboard zoom.
    preview_geometry: Option<(Vec2, Vec2)>,
    textures: Textures,
    info: Option<FileInfo>,
    info_loading: bool,
    focus: Focus,
    columns: usize,
    reveal_file: bool,
    reveal_folder: bool,
    /// The journal of this session's last merge, for Ctrl+Z.
    last_merge: Option<PathBuf>,
    /// A merge or undo running without its dialog: (kind, done, total).
    merge_progress: Option<(MergeKind, usize, usize)>,
    overlay: Option<Overlay>,
    palette: Option<CommandPalette>,
    registry: Registry<Act, View>,
    toasts: Toasts,
    dialogs: FileDialogs<Picked>,
    screenshot: Option<Screenshot>,
    open_on_start: Option<String>,
}

impl MediaLinerApp {
    /// `storage` is eframe's (theme, scale, palette recents); `None` in tests.
    pub fn new(storage: Option<&dyn eframe::Storage>, paths: Paths, launch: Launch) -> Self {
        let prefs = paths.prefs();
        let settings = paths.settings();
        let mut theme: ThemeState = storage
            .and_then(|s| eframe::get_value(s, THEME_KEY))
            .unwrap_or_default();
        diagnostics::theme_overrides(&mut theme);
        let mut registry = commands::registry();
        if let Some(recents) = storage.and_then(|s| eframe::get_value::<Recents>(s, RECENTS_KEY)) {
            registry.load_recents(recents);
        }
        let backend = Backend::new(backend::Config {
            thumbs_dir: paths.thumbs_dir(),
            merges_dir: paths.merges_dir(),
            force_copy: launch.force_copy,
        });
        let mut app = Self {
            paths,
            theme,
            theme_dirty: false,
            custom_titlebar: prefs.custom_titlebar,
            status_bar_visible: prefs.status_bar_visible,
            thumb_size: prefs.thumb_size,
            include_subfolders: prefs.include_subfolders,
            show_location: prefs.show_location,
            folders_visible: prefs.folders_visible,
            files_visible: prefs.files_visible,
            info_visible: prefs.info_visible,
            initialized: false,
            launch,
            backend,
            library: Library::new(settings.folders.clone()),
            expanded: settings.expanded.iter().cloned().collect(),
            children: HashMap::new(),
            has_children: HashMap::new(),
            tree_checked_at: 0.0,
            folder: None,
            files: Vec::new(),
            photos: Vec::new(),
            keys: Vec::new(),
            listing: Listing::default(),
            selected: None,
            preview: None,
            preview_view: PreviewView::default(),
            preview_geometry: None,
            textures: Textures::default(),
            info: None,
            info_loading: false,
            focus: Focus::Content,
            columns: 1,
            reveal_file: false,
            reveal_folder: false,
            last_merge: None,
            merge_progress: None,
            overlay: None,
            palette: None,
            registry,
            toasts: Toasts::default(),
            dialogs: FileDialogs::new(),
            screenshot: Screenshot::from_env(),
            open_on_start: std::env::var("LAANKIT_OPEN").ok(),
        };
        if let Some(folder) = settings.selected_folder {
            if app.library.root_of(&folder).is_some() && folder.is_dir() {
                app.select_folder(folder);
            }
        }
        app
    }

    fn init(&mut self, ctx: &egui::Context) {
        if self.initialized {
            return;
        }
        self.initialized = true;
        self.backend.attach(ctx);
        self.dialogs.attach(ctx);
        fonts::install(ctx);
        self.theme.apply(ctx);
        if let Some(folder) = self.launch.open.take() {
            self.add_folder(folder);
        }
        match self.open_on_start.take().as_deref() {
            Some("settings") => self.apply(ctx, Act::Settings),
            Some("about") => self.apply(ctx, Act::About),
            Some("palette") => self.apply(ctx, Act::Palette),
            _ => {}
        }
    }

    // --- Accessors for tests ------------------------------------------------

    #[cfg(test)]
    pub fn roots(&self) -> &[PathBuf] {
        &self.library.roots
    }

    #[cfg(test)]
    pub fn folder(&self) -> Option<&Path> {
        self.folder.as_deref()
    }

    #[cfg(test)]
    pub fn file_names(&self) -> Vec<String> {
        self.files.iter().map(|f| f.name.clone()).collect()
    }

    #[cfg(test)]
    pub fn photo_count(&self) -> usize {
        self.photos.len()
    }

    #[cfg(test)]
    pub fn listing_done(&self) -> bool {
        !self.listing.loading
    }

    #[cfg(test)]
    pub fn selected(&self) -> Option<&Path> {
        self.selected.as_deref()
    }

    #[cfg(test)]
    pub fn preview_path(&self) -> Option<&Path> {
        self.preview.as_deref()
    }

    #[cfg(test)]
    pub fn preview_ready(&self) -> bool {
        self.preview
            .as_deref()
            .is_some_and(|p| self.textures.has_preview(p))
    }

    #[cfg(test)]
    pub fn thumbnail_ready(&mut self, path: &Path) -> bool {
        let Some(i) = self.photos.iter().position(|&i| self.files[i].path == path) else {
            return false;
        };
        let key = self.keys[i].clone();
        matches!(self.textures.thumb(path, &key), Some(Thumb::Ready(_)))
    }

    #[cfg(test)]
    pub fn info(&self) -> Option<&FileInfo> {
        self.info.as_ref()
    }

    #[cfg(test)]
    pub fn info_visible(&self) -> bool {
        self.info_visible
    }

    #[cfg(test)]
    pub fn has_overlay(&self) -> bool {
        self.overlay.is_some()
    }

    #[cfg(test)]
    pub fn merge_plan_ready(&self) -> bool {
        matches!(&self.overlay, Some(Overlay::Merge(s)) if matches!(s.plan, Some(Ok(_))))
    }

    #[cfg(test)]
    pub fn merging(&self) -> bool {
        self.backend.merging()
    }

    #[cfg(test)]
    pub fn can_undo_merge(&self) -> bool {
        self.last_merge.is_some()
    }

    #[cfg(test)]
    pub fn expanded(&self, path: &Path) -> bool {
        self.expanded.contains(path)
    }

    #[cfg(test)]
    pub fn add_folder_for_test(&mut self, folder: PathBuf) {
        self.add_folder(folder);
    }

    #[cfg(test)]
    pub fn run(&mut self, ctx: &egui::Context, act: Act) {
        self.apply(ctx, act);
    }

    #[cfg(test)]
    pub fn focus(&self) -> Focus {
        self.focus
    }

    // --- State ----------------------------------------------------------------

    fn view(&self) -> View {
        View {
            modal_open: self.overlay.is_some() || self.palette.is_some(),
            focus: self.focus,
            folder_selected: self.folder.is_some(),
            root_selected: self
                .folder
                .as_deref()
                .is_some_and(|f| self.library.is_root(f)),
            photo_selected: self
                .selected
                .as_deref()
                .is_some_and(|p| MediaKind::of(p).is_photo()),
            has_photos: !self.photos.is_empty(),
            preview_open: self.preview.is_some(),
            can_undo_merge: self.last_merge.is_some() && !self.backend.merging(),
        }
    }

    fn top_inset(&self) -> f32 {
        if self.custom_titlebar {
            theme::TITLEBAR_HEIGHT
        } else {
            0.0
        }
    }

    fn photo(&self, i: usize) -> &FileEntry {
        &self.files[self.photos[i]]
    }

    fn photo_index(&self, path: &Path) -> Option<usize> {
        self.photos.iter().position(|&i| self.files[i].path == path)
    }

    fn save_settings(&mut self) {
        let mut expanded: Vec<PathBuf> = self.expanded.iter().cloned().collect();
        expanded.sort();
        let s = Settings {
            folders: self.library.roots.clone(),
            expanded,
            selected_folder: self.folder.clone(),
        };
        if let Err(e) = self.paths.save_settings(&s) {
            self.toasts.error(format!("Couldn't save settings: {e:#}"));
        }
    }

    fn set_prefs(&mut self, patch: serde_json::Value) {
        if let Err(e) = self.paths.set_prefs(&patch) {
            self.toasts.error(format!("Couldn't save preferences: {e:#}"));
        }
    }

    // --- Folders --------------------------------------------------------------

    fn add_folder(&mut self, folder: PathBuf) {
        if !folder.is_dir() {
            self.toasts.error(format!("{} isn't a folder.", folder.display()));
            return;
        }
        match self.library.add(&folder) {
            Added::Added => {}
            Added::Already => self.toasts.info("That folder is already in your list."),
            Added::Inside(root) => self.toasts.info(format!(
                "That folder is already in your list, inside {}.",
                file_name(&root)
            )),
            Added::Replaced(inside) => self.toasts.info(format!(
                "Added in place of {} it holds.",
                match inside.len() {
                    1 => format!("“{}”, which", file_name(&inside[0])),
                    n => format!("{n} listed folders, which"),
                }
            )),
        }
        self.select_folder(folder);
        self.save_settings();
    }

    fn remove_folder(&mut self, root: &Path) {
        if !self.library.is_root(root) {
            return;
        }
        self.library.remove(root);
        self.expanded.retain(|p| !p.starts_with(root));
        if self.folder.as_deref().is_some_and(|f| f.starts_with(root)) {
            self.clear_folder();
        }
        self.save_settings();
        self.toasts.info(format!(
            "Removed {} from the list. Nothing on disk changed.",
            file_name(root)
        ));
    }

    fn clear_folder(&mut self) {
        self.folder = None;
        self.set_files(Vec::new());
        self.listing = Listing::default();
        self.selected = None;
        self.preview = None;
        self.info = None;
    }

    /// Selects `folder` and opens the tree down to it.
    fn select_folder(&mut self, folder: PathBuf) {
        if let Some(root) = self.library.root_of(&folder).map(Path::to_owned) {
            let mut at = folder.parent();
            while let Some(dir) = at.filter(|d| d.starts_with(&root)) {
                self.expanded.insert(dir.to_owned());
                at = dir.parent();
            }
        }
        self.clear_folder();
        self.folder = Some(folder);
        self.reveal_folder = true;
        self.request_listing(0.0);
        self.save_settings();
    }

    fn request_listing(&mut self, now: f64) {
        let Some(folder) = &self.folder else { return };
        if self.files.is_empty() {
            self.listing.loading = true;
        }
        self.listing.in_flight = true;
        self.listing.asked_at = now;
        self.backend.list(folder, self.include_subfolders);
    }

    fn set_files(&mut self, files: Vec<FileEntry>) {
        self.photos = files
            .iter()
            .enumerate()
            .filter(|(_, f)| f.kind.is_photo())
            .map(|(i, _)| i)
            .collect();
        self.keys = self
            .photos
            .iter()
            .map(|&i| ThumbJob::of(&files[i]).key())
            .collect();
        self.files = files;
    }

    fn children_of(&mut self, dir: &Path) -> Vec<PathBuf> {
        self.children
            .entry(dir.to_owned())
            .or_insert_with(|| scan::subfolders(dir).unwrap_or_default())
            .clone()
    }

    fn has_children(&mut self, dir: &Path) -> bool {
        *self
            .has_children
            .entry(dir.to_owned())
            .or_insert_with(|| scan::has_subfolders(dir))
    }

    /// The tree's visible rows, in order.
    fn folder_rows(&mut self) -> Vec<FolderRow> {
        let mut out = Vec::new();
        let roots = self.library.roots.clone();
        for root in &roots {
            let missing = !root.is_dir();
            let hint = config::shares_name(root, &roots)
                .then(|| root.parent().map(|p| p.display().to_string()))
                .flatten();
            self.push_rows(root, 0, true, missing, hint, &mut out);
        }
        out
    }

    fn push_rows(
        &mut self,
        dir: &Path,
        depth: usize,
        root: bool,
        missing: bool,
        hint: Option<String>,
        out: &mut Vec<FolderRow>,
    ) {
        let open = !missing && self.expanded.contains(dir);
        let kids = if open { self.children_of(dir) } else { Vec::new() };
        let branch = if missing {
            None
        } else if open {
            (!kids.is_empty()).then_some(true)
        } else {
            self.has_children(dir).then_some(false)
        };
        out.push(FolderRow {
            path: dir.to_owned(),
            depth,
            name: file_name(dir),
            hint,
            branch,
            root,
            missing,
        });
        for k in kids {
            self.push_rows(&k, depth + 1, false, false, None, out);
        }
    }

    fn toggle_folder(&mut self, dir: &Path) {
        if !self.expanded.remove(dir) {
            self.expanded.insert(dir.to_owned());
        }
        self.save_settings();
    }

    /// The arrow keys in the tree.
    fn tree_step(&mut self, act: Act) {
        let Some(current) = self.folder.clone() else { return };
        let rows = self.folder_rows();
        let Some(i) = rows.iter().position(|r| r.path == current) else {
            return;
        };
        let row = &rows[i];
        let target = match act {
            Act::FolderAbove => i.checked_sub(1).map(|j| rows[j].path.clone()),
            Act::FolderBelow => rows.get(i + 1).map(|r| r.path.clone()),
            Act::FolderOut => {
                if row.branch == Some(true) {
                    self.toggle_folder(&current);
                    None
                } else if row.root {
                    None
                } else {
                    current.parent().map(Path::to_owned)
                }
            }
            Act::FolderIn => match row.branch {
                Some(false) => {
                    self.toggle_folder(&current);
                    None
                }
                Some(true) => rows.get(i + 1).map(|r| r.path.clone()),
                None => None,
            },
            _ => None,
        };
        if let Some(t) = target {
            if !rows.iter().any(|r| r.path == t && r.missing) {
                self.select_folder(t);
                self.focus = Focus::Tree;
            }
        }
    }

    // --- Photos ---------------------------------------------------------------

    fn select(&mut self, path: PathBuf) {
        if self.selected.as_ref() != Some(&path) {
            self.selected = Some(path);
            self.reveal_file = true;
            self.request_info();
        }
    }

    fn request_info(&mut self) {
        if !self.info_visible {
            return;
        }
        match &self.selected {
            Some(p) => {
                if self.info.as_ref().is_none_or(|i| i.path != *p) {
                    self.info_loading = true;
                    self.backend.info(p);
                }
            }
            None => self.info = None,
        }
    }

    fn open_preview(&mut self, path: PathBuf) {
        let Some(i) = self.photo_index(&path) else { return };
        self.select(path.clone());
        self.preview = Some(path.clone());
        self.preview_view = PreviewView::default();
        self.focus = Focus::Content;
        let next = (i + 1 < self.photos.len()).then(|| self.photo(i + 1).path.clone());
        let previous = i.checked_sub(1).map(|j| self.photo(j).path.clone());
        let mut keep: Vec<&Path> = vec![&path];
        keep.extend(next.as_deref());
        keep.extend(previous.as_deref());
        self.textures.keep_previews(&keep);
        let ahead: Vec<PathBuf> = next
            .iter()
            .chain(previous.iter())
            .filter(|p| !self.textures.has_preview(p))
            .cloned()
            .collect();
        if self.textures.has_preview(&path) {
            if !ahead.is_empty() {
                self.backend.want_preview(&ahead[0], &ahead[1..]);
            }
        } else {
            self.backend.want_preview(&path, &ahead);
        }
    }

    /// Moves the selection, or the preview, by `step` photos.
    fn step_photo(&mut self, step: isize) {
        if self.photos.is_empty() {
            return;
        }
        let current = self
            .preview
            .as_deref()
            .or(self.selected.as_deref())
            .and_then(|p| self.photo_index(p));
        let n = self.photos.len() as isize;
        let next = match current {
            None => 0,
            Some(i) => (i as isize + step).clamp(0, n - 1),
        } as usize;
        let path = self.photo(next).path.clone();
        if self.preview.is_some() {
            if Some(next) != current {
                self.open_preview(path);
            }
        } else {
            self.select(path);
        }
    }

    fn photo_command(&mut self, ctx: &egui::Context, path: PathBuf, cmd: PhotoCommand) {
        match cmd {
            PhotoCommand::OpenInFolder => {
                std::thread::spawn(move || {
                    if let Err(e) = reveal::reveal(&path) {
                        log::warn!("open in folder: {e}");
                    }
                });
            }
            PhotoCommand::ShowInfo => {
                self.select(path);
                if !self.info_visible {
                    self.info_visible = true;
                    self.set_prefs(json!({ "infoVisible": true }));
                }
                self.request_info();
            }
            PhotoCommand::OpenWithDefault => {
                if let Err(e) = open::that_detached(&path) {
                    self.toasts.error(format!("Couldn't open it: {e}"));
                }
            }
            PhotoCommand::CopyPath => {
                ctx.copy_text(path.display().to_string());
                self.toasts.info("Path copied.");
            }
        }
    }

    fn zoom(&mut self, factor: f32) {
        if self.preview.is_some() {
            if let Some((image, area)) = self.preview_geometry {
                self.preview_view.zoom_by(factor, Vec2::ZERO, image, area);
            }
        } else {
            let step = if factor > 1.0 { THUMB_STEP } else { -THUMB_STEP };
            self.set_thumb_size(self.thumb_size + step);
        }
    }

    fn set_thumb_size(&mut self, size: f32) {
        let size = size.clamp(THUMB_MIN, THUMB_MAX).round();
        if size != self.thumb_size {
            self.thumb_size = size;
            self.reveal_file = true;
            self.set_prefs(json!({ "thumbSize": size }));
        }
    }

    // --- Merging --------------------------------------------------------------

    fn open_merge(&mut self, source: PathBuf, dest: Option<PathBuf>) {
        if self.backend.merging() {
            self.toasts.info("A merge is still running.");
            return;
        }
        if !source.is_dir() {
            self.toasts.error(merge::Refusal::SourceMissing.message());
            return;
        }
        if let Some(dest) = &dest {
            if let Err(r) = merge::check(&source, dest) {
                self.toasts.error(r.message());
                return;
            }
        }
        let mut state = MergeState::new(source, self.library.roots.clone());
        if let Some(dest) = dest {
            self.choose_merge_dest(&mut state, dest);
        }
        self.overlay = Some(Overlay::Merge(Box::new(state)));
    }

    fn choose_merge_dest(&mut self, state: &mut MergeState, dest: PathBuf) {
        let dups = self.duplicates_dir(&state.source, &dest);
        self.backend.plan_merge(&state.source, &dest, &dups);
        state.dest = Some(dest);
        state.plan = None;
    }

    /// Where identical files go: the source's listed folder, or when the
    /// source is one, the destination's.
    fn duplicates_dir(&self, source: &Path, dest: &Path) -> PathBuf {
        let base = if self.library.is_root(source) {
            self.library.root_of(dest).unwrap_or(dest)
        } else {
            self.library
                .root_of(source)
                .unwrap_or_else(|| source.parent().unwrap_or(dest))
        };
        base.join(merge::DUPLICATES_DIR)
    }

    fn undo_merge(&mut self, journal: &Path) {
        if self.backend.merging() {
            self.toasts.info("A merge is still running.");
            return;
        }
        match merge::read_journal(journal) {
            Ok(j) if j.undone => self.toasts.info("That merge has already been undone."),
            Ok(j) => {
                self.merge_progress = Some((MergeKind::Undo, 0, j.moves.len()));
                self.backend.undo_merge(j);
            }
            Err(e) => self.toasts.error(format!("Couldn't read the merge's journal: {e:#}")),
        }
    }

    fn merge_done(&mut self, kind: MergeKind, outcome: Result<merge::Outcome, String>) {
        self.backend.merge_finished();
        self.merge_progress = None;
        let source = match &self.overlay {
            Some(Overlay::Merge(s)) => Some((s.source.clone(), s.dest.clone())),
            _ => None,
        };
        if kind == MergeKind::Merge && matches!(self.overlay, Some(Overlay::Merge(_))) {
            self.overlay = None;
        }
        match outcome {
            Err(e) => self.toasts.error(match kind {
                MergeKind::Merge => format!("The merge failed: {e}"),
                MergeKind::Undo => format!("Couldn't undo the merge: {e}"),
            }),
            Ok(out) => {
                let moved = out.moved + out.duplicates;
                let files = |n: usize| format!("{n} {}", if n == 1 { "file" } else { "files" });
                let mut text = match (kind, out.cancelled) {
                    (MergeKind::Merge, true) => format!("Merge stopped after {}.", files(moved)),
                    (MergeKind::Merge, false) => format!("Merged: moved {}.", files(moved)),
                    (MergeKind::Undo, _) => format!("Undone: moved {} back.", files(out.moved)),
                };
                if !out.skipped.is_empty() {
                    text.push_str(&format!(" {} couldn't move.", files(out.skipped.len())));
                    for (p, why) in &out.skipped {
                        log::warn!("{}: {why}", p.display());
                    }
                }
                if kind == MergeKind::Merge {
                    text.push_str(" Ctrl+Z undoes it.");
                    self.last_merge = Some(out.journal.clone());
                } else if self.last_merge.as_ref() == Some(&out.journal) {
                    self.last_merge = None;
                }
                if out.skipped.is_empty() {
                    self.toasts.success(text);
                } else {
                    self.toasts.error(text);
                }
                if kind == MergeKind::Merge && out.source_removed {
                    if let Some((src, dest)) = source {
                        if self.library.is_root(&src) {
                            self.library.remove(&src);
                        }
                        if self.folder.as_deref().is_some_and(|f| f.starts_with(&src)) {
                            if let Some(dest) = dest {
                                self.select_folder(dest);
                            }
                        }
                    }
                }
            }
        }
        if self.folder.as_deref().is_some_and(|f| !f.is_dir()) {
            self.clear_folder();
        }
        self.children.clear();
        self.has_children.clear();
        self.request_listing(0.0);
        self.save_settings();
        if let Some(Overlay::Settings(s)) = &mut self.overlay {
            s.journals = None;
            self.backend.journals();
        }
    }

    // --- Commands -------------------------------------------------------------

    fn apply(&mut self, ctx: &egui::Context, act: Act) {
        match act {
            Act::AddFolder => {
                if !self.dialogs.busy() {
                    self.dialogs.start(|| {
                        rfd::FileDialog::new()
                            .set_title("Add Folder")
                            .pick_folder()
                            .map(Picked::Folder)
                    });
                }
            }
            Act::RemoveFolder => {
                if let Some(f) = self.folder.clone() {
                    self.remove_folder(&f);
                }
            }
            Act::Refresh => {
                self.children.clear();
                self.has_children.clear();
                self.request_listing(ctx.input(|i| i.time));
            }
            Act::Back => {
                self.preview = None;
                self.reveal_file = true;
            }
            Act::OpenPreview => {
                if let Some(p) = self.selected.clone() {
                    self.open_preview(p);
                }
            }
            Act::PreviousPhoto => self.step_photo(-1),
            Act::NextPhoto => self.step_photo(1),
            Act::PhotoAbove => self.step_photo(-(self.columns as isize)),
            Act::PhotoBelow => self.step_photo(self.columns as isize),
            Act::FirstPhoto => self.step_photo(-(self.photos.len() as isize)),
            Act::LastPhoto => self.step_photo(self.photos.len() as isize),
            Act::FolderAbove | Act::FolderBelow | Act::FolderOut | Act::FolderIn => {
                self.tree_step(act)
            }
            Act::ZoomIn => self.zoom(1.25),
            Act::ZoomOut => self.zoom(0.8),
            Act::ZoomFit => self.preview_view = PreviewView::default(),
            Act::ZoomActual => {
                self.preview_view = PreviewView {
                    fit: false,
                    zoom: 1.0,
                    pan: Vec2::ZERO,
                }
            }
            Act::ShowInfo => {
                self.info_visible = !self.info_visible;
                self.set_prefs(json!({ "infoVisible": self.info_visible }));
                self.request_info();
            }
            Act::ToggleFolders => {
                self.folders_visible = !self.folders_visible;
                self.set_prefs(json!({ "foldersVisible": self.folders_visible }));
            }
            Act::ToggleFiles => {
                self.files_visible = !self.files_visible;
                self.set_prefs(json!({ "filesVisible": self.files_visible }));
            }
            Act::ToggleSubfolders => {
                self.include_subfolders = !self.include_subfolders;
                self.set_prefs(json!({ "includeSubfolders": self.include_subfolders }));
                self.set_files(Vec::new());
                self.request_listing(ctx.input(|i| i.time));
            }
            Act::ToggleStatusBar => {
                self.status_bar_visible = !self.status_bar_visible;
                self.set_prefs(json!({ "statusBarVisible": self.status_bar_visible }));
            }
            Act::MergeInto => {
                if let Some(f) = self.folder.clone() {
                    self.open_merge(f, None);
                }
            }
            Act::UndoMerge => {
                if let Some(j) = self.last_merge.clone() {
                    self.undo_merge(&j);
                }
            }
            Act::OpenInFolder => {
                if let Some(p) = self.selected.clone() {
                    self.photo_command(ctx, p, PhotoCommand::OpenInFolder);
                }
            }
            Act::OpenWithDefault => {
                if let Some(p) = self.selected.clone() {
                    self.photo_command(ctx, p, PhotoCommand::OpenWithDefault);
                }
            }
            Act::CopyPath => {
                if let Some(p) = self.selected.clone() {
                    self.photo_command(ctx, p, PhotoCommand::CopyPath);
                }
            }
            Act::Settings => self.open_settings("folders"),
            Act::About => self.overlay = Some(Overlay::About),
            Act::Palette => self.palette = Some(CommandPalette::new()),
        }
    }

    fn open_settings(&mut self, tab: &str) {
        self.overlay = Some(Overlay::Settings(SettingsState::new(
            tab,
            self.custom_titlebar,
            self.status_bar_visible,
        )));
        self.backend.cache_usage();
        self.backend.journals();
    }

    fn view_action(&mut self, ctx: &egui::Context, a: ViewAction) {
        match a {
            ViewAction::Act(act) => self.apply(ctx, act),
            ViewAction::FocusTree => self.focus = Focus::Tree,
            ViewAction::FocusContent => self.focus = Focus::Content,
            ViewAction::SelectFolder(p) => self.select_folder(p),
            ViewAction::ToggleFolder(p) => self.toggle_folder(&p),
            ViewAction::MergeFolder(p) => self.open_merge(p, None),
            ViewAction::DropFolder { source, dest } => self.open_merge(source, Some(dest)),
            ViewAction::OpenFolderInFileManager(p) => {
                if let Err(e) = open::that_detached(&p) {
                    self.toasts.error(format!("Couldn't open the folder: {e}"));
                }
            }
            ViewAction::RefreshFolder(p) => {
                self.children.retain(|k, _| !k.starts_with(&p));
                self.has_children.retain(|k, _| !k.starts_with(&p));
                if self.folder.as_deref().is_some_and(|f| f.starts_with(&p)) {
                    self.request_listing(ctx.input(|i| i.time));
                }
            }
            ViewAction::RemoveFolder(p) => self.remove_folder(&p),
            ViewAction::Select(p) => self.select(p),
            ViewAction::OpenPreview(p) => self.open_preview(p),
            ViewAction::Photo(p, cmd) => self.photo_command(ctx, p, cmd),
            ViewAction::SetThumbSize(s) => self.set_thumb_size(s),
            ViewAction::ToggleSubfolders => self.apply(ctx, Act::ToggleSubfolders),
            ViewAction::CloseInfo => {
                self.info_visible = false;
                self.set_prefs(json!({ "infoVisible": false }));
            }
            ViewAction::ShowLocation(on) => {
                self.show_location = on;
                self.set_prefs(json!({ "showLocation": on }));
            }
            ViewAction::Copy(text) => {
                ctx.copy_text(text);
                self.toasts.info("Copied.");
            }
            ViewAction::OpenUrl(url) => ctx.open_url(egui::OpenUrl::new_tab(url)),
        }
    }

    // --- Backend events -------------------------------------------------------

    fn backend_events(&mut self, ctx: &egui::Context) {
        for event in self.backend.poll() {
            match event {
                Event::Listing { dir, deep, files } => {
                    if self.folder.as_ref() != Some(&dir) || deep != self.include_subfolders {
                        continue;
                    }
                    self.listing.in_flight = false;
                    self.listing.loading = false;
                    match files {
                        Ok(files) => {
                            self.listing.error = None;
                            if scan::fingerprint(&files) != scan::fingerprint(&self.files) {
                                self.set_files(files);
                                if self
                                    .selected
                                    .as_ref()
                                    .is_some_and(|s| !self.files.iter().any(|f| &f.path == s))
                                {
                                    self.selected = None;
                                    self.info = None;
                                }
                                if self
                                    .preview
                                    .as_ref()
                                    .is_some_and(|s| self.photo_index(s).is_none())
                                {
                                    self.preview = None;
                                }
                            }
                        }
                        Err(e) => {
                            self.listing.error = Some(e);
                            self.set_files(Vec::new());
                        }
                    }
                }
                Event::Thumb {
                    path,
                    key,
                    image,
                    quick,
                } => self.textures.push_thumb(path, key, image, quick),
                Event::Preview { path, image } => self.textures.set_preview(ctx, path, image),
                Event::Info { path, info } => {
                    if self.selected.as_ref() == Some(&path) {
                        self.info_loading = false;
                        match info {
                            Ok(i) => self.info = Some(i),
                            Err(e) => {
                                self.info = None;
                                log::warn!("info for {}: {e}", path.display());
                            }
                        }
                    }
                }
                Event::MergePlanned { source, dest, plan } => {
                    if let Some(Overlay::Merge(s)) = &mut self.overlay {
                        if s.source == source && s.dest.as_ref() == Some(&dest) {
                            s.plan = Some(plan);
                        }
                    }
                }
                Event::MergeProgress { done, total } => {
                    if let Some(Overlay::Merge(state)) = &mut self.overlay {
                        if let Phase::Running { done: d, total: t, .. } = &mut state.phase {
                            *d = done;
                            *t = total;
                        }
                    }
                    if let Some((_, d, t)) = &mut self.merge_progress {
                        *d = done;
                        *t = total;
                    }
                }
                Event::MergeDone { kind, outcome } => self.merge_done(kind, outcome),
                Event::CacheUsage { bytes, count } => {
                    if let Some(Overlay::Settings(s)) = &mut self.overlay {
                        s.cache = Some((bytes, count));
                    }
                }
                Event::Journals(j) => {
                    if let Some(Overlay::Settings(s)) = &mut self.overlay {
                        s.journals = Some(j);
                    }
                }
            }
        }
    }

    /// Looks at the selected folder, and the tree, for changes.
    fn watch(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
        if self.folder.is_some() {
            // A whole subtree is read less often than one folder.
            let every = if self.include_subfolders {
                TREE_CHECK_SECS
            } else {
                FOLDER_CHECK_SECS
            };
            if focused && !self.listing.in_flight && now - self.listing.asked_at >= every {
                self.request_listing(now);
            }
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(every));
        }
        if focused && now - self.tree_checked_at >= TREE_CHECK_SECS {
            self.tree_checked_at = now;
            self.children.clear();
            self.has_children.clear();
        }
    }

    // --- Frame ----------------------------------------------------------------

    pub fn show(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        self.init(&ctx);
        if std::mem::take(&mut self.theme_dirty) {
            self.theme.apply(&ctx);
        }
        self.backend_events(&ctx);
        if let Some(Picked::Folder(folder)) = self.dialogs.poll() {
            self.add_folder(folder);
        }
        self.watch(&ctx);
        self.textures.begin_frame(&ctx);
        if !ctx.egui_wants_keyboard_input() {
            let view = self.view();
            for act in self.registry.dispatch_keys(&ctx, &view) {
                self.apply(&ctx, act);
            }
        }
        if self.custom_titlebar {
            self.titlebar(ui);
        }
        self.main_view(ui);
        self.overlays(&ctx);
        self.palette(&ctx);
        self.toasts.show(&ctx);
        if self.custom_titlebar {
            window::window_edges(&ctx);
        }
        if let Some(shot) = &mut self.screenshot {
            shot.tick(&ctx);
        }
    }

    fn titlebar(&mut self, ui: &mut Ui) {
        let bar = TitleBar {
            app_name: "MediaLiner",
            toolbar_visible: true,
        };
        let mut act = None;
        titlebar::show(ui, &bar, &mut self.theme, |ui| {
            if titlebar::button(ui, icons::CIRCLE_QUESTION, "About", false).clicked() {
                act = Some(Act::About);
            }
            if titlebar::button(ui, icons::GEAR, "Settings", false).clicked() {
                act = Some(Act::Settings);
            }
        });
        if let Some(a) = act {
            let ctx = ui.ctx().clone();
            self.apply(&ctx, a);
        }
    }

    fn status_parts(&self) -> Vec<String> {
        let mut parts = Vec::new();
        let Some(folder) = &self.folder else {
            let n = self.library.roots.len();
            parts.push(match n {
                0 => "No folders".to_string(),
                1 => "1 folder".to_string(),
                n => format!("{n} folders"),
            });
            return parts;
        };
        parts.push(file_name(folder));
        let photos = self.photos.len();
        let others = self.files.len() - photos;
        parts.push(match photos {
            1 => "1 photo".to_string(),
            n => format!("{n} photos"),
        });
        if others > 0 {
            parts.push(match others {
                1 => "1 other file".to_string(),
                n => format!("{n} other files"),
            });
        }
        let total: u64 = self.files.iter().map(|f| f.size).sum();
        if total > 0 {
            parts.push(human_size(total));
        }
        if let Some(sel) = self
            .selected
            .as_ref()
            .and_then(|s| self.files.iter().find(|f| &f.path == s))
        {
            parts.push(format!("Selected: {} ({})", sel.name, human_size(sel.size)));
        }
        parts
    }

    fn main_view(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let p = theme::palette(&ctx);
        let mut actions: Vec<ViewAction> = Vec::new();
        let props = ToolbarProps {
            folders_visible: self.folders_visible,
            files_visible: self.files_visible,
            info_visible: self.info_visible,
            settings_open: matches!(self.overlay, Some(Overlay::Settings(_))),
            about_open: matches!(self.overlay, Some(Overlay::About)),
        };
        actions.extend(views::toolbar(ui, &props).into_iter().map(ViewAction::Act));
        if self.status_bar_visible {
            let parts = self.status_parts();
            let busy = self.merge_progress.map(|(kind, done, total)| match kind {
                MergeKind::Merge => format!("Merging… {done} of {total}"),
                MergeKind::Undo => format!("Undoing the merge… {done} of {total}"),
            });
            egui::Panel::bottom("medialiner-status")
                .exact_size(views::STATUS_BAR_HEIGHT)
                .resizable(false)
                .show_separator_line(false)
                .frame(Frame::NONE)
                .show(ui, |ui| views::status_bar(ui, &parts, busy.as_deref()));
        }
        if self.folders_visible {
            let rows = self.folder_rows();
            let reveal = std::mem::take(&mut self.reveal_folder);
            let selected = self.folder.clone();
            let (min, default, max) = views::FOLDERS_WIDTH;
            egui::Panel::left("medialiner-folders")
                .resizable(true)
                .default_size(default)
                .size_range(min..=max)
                .frame(Frame::NONE.fill(p.bg_surface))
                .show(ui, |ui| {
                    actions.extend(views::folders_pane(ui, &rows, selected.as_deref(), reveal));
                });
        }
        let reveal_file = std::mem::take(&mut self.reveal_file);
        if self.files_visible {
            let props = FilesProps {
                folder: self.folder.as_deref(),
                files: &self.files,
                loading: self.listing.loading,
                error: self.listing.error.as_deref(),
                selected: self.selected.as_deref(),
                include_subfolders: self.include_subfolders,
                reveal: reveal_file,
            };
            let (min, default, max) = views::FILES_WIDTH;
            egui::Panel::left("medialiner-files")
                .resizable(true)
                .default_size(default)
                .size_range(min..=max)
                .frame(Frame::NONE.fill(p.bg_surface))
                .show(ui, |ui| actions.extend(views::files_pane(ui, &props)));
        }
        if self.info_visible {
            let props = InfoProps {
                info: self
                    .info
                    .as_ref()
                    .filter(|i| self.selected.as_ref() == Some(&i.path)),
                loading: self.info_loading,
                show_location: self.show_location,
            };
            let (min, default, max) = views::INFO_WIDTH;
            egui::Panel::right("medialiner-info")
                .resizable(true)
                .default_size(default)
                .size_range(min..=max)
                .frame(Frame::NONE.fill(p.bg_surface))
                .show(ui, |ui| actions.extend(views::info_pane(ui, &props)));
        }
        let mut wanted = Vec::new();
        egui::CentralPanel::default()
            .frame(Frame::NONE.fill(p.bg_base))
            .show(ui, |ui| {
                let Some(folder) = self.folder.clone() else {
                    if views::welcome(ui, !self.library.roots.is_empty()) {
                        actions.push(ViewAction::Act(Act::AddFolder));
                    }
                    return;
                };
                let title = match &self.preview {
                    Some(p) => file_name(p),
                    None => file_name(&folder),
                };
                let subtitle = match &self.preview {
                    Some(p) => self
                        .photo_index(p)
                        .map(|i| format!("{} of {}", i + 1, self.photos.len())),
                    None => Some(folder.display().to_string()),
                };
                let zoom_label = self.preview_geometry.map(|(image, area)| {
                    format!("{:.0}%", self.preview_view.effective(image, area) * 100.0)
                });
                let header = HeaderProps {
                    title: &title,
                    subtitle,
                    preview_open: self.preview.is_some(),
                    thumb_size: self.thumb_size,
                    zoom_label,
                };
                actions.extend(views::centre_header(ui, &header));
                if let Some(path) = self.preview.clone() {
                    let thumb_key = self.photo_index(&path).map(|i| self.keys[i].clone());
                    if let Some(key) = &thumb_key {
                        if self.textures.needs(&path, key) {
                            if let Some(i) = self.photo_index(&path) {
                                wanted.push(ThumbJob::of(self.photo(i)));
                            }
                        }
                    }
                    let thumb = thumb_key.and_then(|k| match self.textures.thumb(&path, &k) {
                        Some(Thumb::Ready(t)) => Some(t.clone()),
                        _ => None,
                    });
                    let texture = self.textures.preview(&path).cloned();
                    self.preview_geometry = match &texture {
                        Some(Ok(t)) => Some((t.size_vec2(), ui.available_size())),
                        _ => None,
                    };
                    let props = PreviewProps {
                        path: &path,
                        texture: texture.as_ref(),
                        thumb: thumb.as_ref(),
                    };
                    actions.extend(views::preview(ui, &props, &mut self.preview_view));
                    return;
                }
                if self.photos.is_empty() {
                    views::no_photos(
                        ui,
                        self.listing.loading,
                        self.listing.error.as_deref(),
                        self.files.len(),
                    );
                    return;
                }
                let photos: Vec<&FileEntry> = self.photos.iter().map(|&i| &self.files[i]).collect();
                let props = GridProps {
                    folder: &folder,
                    photos: &photos,
                    keys: &self.keys,
                    selected: self.selected.as_deref(),
                    thumb_size: self.thumb_size,
                    reveal: reveal_file,
                };
                let out = views::grid(ui, &props, &mut self.textures);
                self.columns = out.columns;
                for i in out.visible {
                    if self.textures.needs(&photos[i].path, &self.keys[i]) {
                        wanted.push(ThumbJob::of(photos[i]));
                    }
                }
                actions.extend(out.actions);
            });
        self.backend.want_thumbs(wanted);
        for a in actions {
            self.view_action(&ctx, a);
        }
    }

    // --- Dialogs --------------------------------------------------------------

    fn overlays(&mut self, ctx: &egui::Context) {
        let Some(mut overlay) = self.overlay.take() else {
            return;
        };
        let top_inset = self.top_inset();
        let mut keep = true;
        match &mut overlay {
            Overlay::Merge(state) => {
                for a in overlays::merge::show(ctx, state) {
                    match a {
                        MergeAction::Close => keep = false,
                        MergeAction::Choose(dest) => self.choose_merge_dest(state, dest),
                        MergeAction::Merge => {
                            if let Some(Ok(plan)) = &state.plan {
                                state.phase = Phase::Running {
                                    done: 0,
                                    total: plan.moves.len(),
                                    cancelling: false,
                                };
                                self.backend.run_merge(plan.clone(), state.remove_source);
                            }
                        }
                        MergeAction::Cancel => {
                            self.backend.cancel_merge();
                            if let Phase::Running { cancelling, .. } = &mut state.phase {
                                *cancelling = true;
                            }
                        }
                    }
                }
            }
            Overlay::Settings(state) => {
                let props = SettingsProps {
                    folders: &self.library.roots,
                    merging: self.backend.merging(),
                    cache_dir: &self.paths.thumbs_dir(),
                };
                let actions = overlays::settings::show(ctx, state, &mut self.theme, &props, top_inset);
                for a in actions {
                    match a {
                        SettingsAction::Close => keep = false,
                        SettingsAction::ThemeChanged => {}
                        SettingsAction::SetCustomTitlebar(on) => {
                            self.set_prefs(json!({ "customTitlebar": on }));
                        }
                        SettingsAction::Restart => restart(ctx),
                        SettingsAction::SetStatusBar(on) => {
                            self.status_bar_visible = on;
                            self.set_prefs(json!({ "statusBarVisible": on }));
                        }
                        SettingsAction::AddFolder => self.apply(ctx, Act::AddFolder),
                        SettingsAction::RemoveFolder(p) => self.remove_folder(&p),
                        SettingsAction::ClearCache => {
                            state.cache = None;
                            self.backend.clear_cache();
                        }
                        SettingsAction::Undo(journal) => {
                            self.undo_merge(&journal);
                            state.journals = None;
                        }
                    }
                }
            }
            Overlay::About => {
                let modal = AboutModal::new("MediaLiner", env!("CARGO_PKG_VERSION"))
                    .description(DESCRIPTION)
                    .repo(REPO_URL, Some("github.com/NathanLaan/laankit"))
                    .icon(AboutIcon::Glyph(icons::IMAGES));
                match modal.show(ctx) {
                    Some(AboutAction::Close) => keep = false,
                    Some(AboutAction::OpenUrl(url)) => ctx.open_url(egui::OpenUrl::new_tab(url)),
                    Some(_) | None => {}
                }
            }
        }
        if keep && self.overlay.is_none() {
            self.overlay = Some(overlay);
        }
    }

    fn palette(&mut self, ctx: &egui::Context) {
        let Some(mut palette) = self.palette.take() else {
            return;
        };
        let view = View {
            modal_open: false,
            ..self.view()
        };
        match palette.show(ctx, &mut self.registry, &view, &mut |_| Vec::new()) {
            PaletteOutcome::Open => self.palette = Some(palette),
            PaletteOutcome::Closed => {}
            PaletteOutcome::Run(act) => self.apply(ctx, act),
        }
    }

    pub fn save_state(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, THEME_KEY, &self.theme);
        eframe::set_value(storage, RECENTS_KEY, &self.registry.recents());
    }

    /// Settings → Merges, for tests.
    #[cfg(test)]
    pub fn journals(&self) -> Vec<merge::Journal> {
        merge::journals(&self.paths.merges_dir())
    }
}

impl eframe::App for MediaLinerApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.save_state(storage);
    }
}

/// Starts a new copy with the same arguments, then closes this one.
fn restart(ctx: &egui::Context) {
    match std::env::current_exe() {
        Ok(exe) => {
            let args: Vec<String> = std::env::args().skip(1).collect();
            if let Err(e) = std::process::Command::new(exe).args(args).spawn() {
                log::error!("relaunch failed: {e}");
                return;
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        Err(e) => log::error!("relaunch failed: {e}"),
    }
}
