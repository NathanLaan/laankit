//! Import Photos: choose a device and a folder, see what will be copied,
//! then watch it copy.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use egui::RichText;
use laankit_egui::dialog::{self, ButtonKind, Dialog, Size};
use laankit_egui::icons;
use laankit_egui::settings;
use laankit_egui::theme;
use laankit_egui::tree::{self, TreeRow};
use medialiner_core::devices::Volume;
use medialiner_core::import::{self, Item, Options, Outcome, Progress, Record};
use medialiner_core::info::human_size;
use medialiner_core::scan::{self, file_name};
use medialiner_core::MediaKind;

const WIDTH: f32 = 560.0;
const ROW_HEIGHT: f32 = 20.0;

/// What is known of the chosen device's files.
pub enum Scan {
    /// Looking, with the count so far.
    Scanning(usize),
    Ready {
        items: Vec<Item>,
        record: Option<Record>,
    },
    Failed(String),
}

pub enum Phase {
    Choose,
    Running {
        progress: Progress,
        cancelling: bool,
    },
    /// Copied; asking whether to delete the copies' originals.
    AskDelete,
    Deleting {
        done: usize,
        total: usize,
    },
    Done,
}

pub struct ImportState {
    pub devices: Vec<Volume>,
    pub device: usize,
    /// Still asking which devices are there.
    pub looking: bool,
    pub scan: Scan,
    /// Only the files not imported from this device before.
    pub new_only: bool,
    pub options: Options,
    pub delete_after: bool,
    pub roots: Vec<PathBuf>,
    pub expanded: HashSet<PathBuf>,
    children: HashMap<PathBuf, Vec<PathBuf>>,
    pub dest: Option<PathBuf>,
    /// The name being typed for a new folder.
    pub new_folder: Option<String>,
    pub new_folder_error: Option<String>,
    focus_new_folder: bool,
    /// The free space at `dest`, read when it's chosen.
    free: Option<(PathBuf, Option<u64>)>,
    pub phase: Phase,
    pub outcome: Option<Outcome>,
    pub deleted: Option<(usize, Vec<(String, String)>)>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ImportAction {
    Close,
    SelectDevice(usize),
    /// The kinds to import changed: remember them.
    Options(Options),
    /// A folder was made in the tree.
    FolderCreated(PathBuf),
    Import,
    /// Stop the running import.
    Cancel,
    /// Whether to delete the copied files from the device.
    Delete(bool),
    /// Close, and show the folder the photos went to.
    Done,
    AddFolder,
}

impl ImportState {
    /// The tree opens down to `dest`, the folder selected in the app.
    pub fn new(roots: Vec<PathBuf>, dest: Option<PathBuf>, options: Options) -> Self {
        let mut expanded = HashSet::new();
        if let Some(dest) = &dest {
            if let Some(root) = roots.iter().find(|r| dest.starts_with(r)) {
                let mut at = dest.parent();
                while let Some(dir) = at.filter(|d| d.starts_with(root)) {
                    expanded.insert(dir.to_owned());
                    at = dir.parent();
                }
            }
        }
        Self {
            devices: Vec::new(),
            device: 0,
            looking: false,
            scan: Scan::Scanning(0),
            new_only: false,
            options,
            delete_after: false,
            roots,
            expanded,
            children: HashMap::new(),
            dest,
            new_folder: None,
            new_folder_error: None,
            focus_new_folder: false,
            free: None,
            phase: Phase::Choose,
            outcome: None,
            deleted: None,
        }
    }

    pub fn volume(&self) -> Option<&Volume> {
        self.devices.get(self.device)
    }

    /// The scan of the chosen device arrived.
    pub fn scanned(&mut self, items: Result<Vec<Item>, String>, record: Option<Record>) {
        self.scan = match items {
            Ok(items) => {
                // Only what's new, when some of it has been imported before.
                self.new_only = record.as_ref().is_some_and(|r| !r.files.is_empty());
                Scan::Ready { items, record }
            }
            Err(e) => Scan::Failed(e),
        };
    }

    /// The files Import will copy.
    pub fn chosen(&self) -> Vec<Item> {
        match &self.scan {
            Scan::Ready { items, record } => {
                import::choose(items, self.options, record.as_ref(), self.new_only)
            }
            _ => Vec::new(),
        }
    }

    pub fn running(&self) -> bool {
        matches!(
            self.phase,
            Phase::Running { .. } | Phase::AskDelete | Phase::Deleting { .. }
        )
    }

    fn children(&mut self, dir: &Path) -> Vec<PathBuf> {
        self.children
            .entry(dir.to_owned())
            .or_insert_with(|| scan::subfolders(dir).unwrap_or_default())
            .clone()
    }

    fn rows(&mut self) -> Vec<(PathBuf, usize, Option<bool>)> {
        let mut out = Vec::new();
        for root in self.roots.clone() {
            self.push(&root, 0, &mut out);
        }
        out
    }

    fn push(&mut self, dir: &Path, depth: usize, out: &mut Vec<(PathBuf, usize, Option<bool>)>) {
        let open = self.expanded.contains(dir);
        let kids = if open { self.children(dir) } else { Vec::new() };
        let branch = if open {
            (!kids.is_empty()).then_some(true)
        } else {
            scan::has_subfolders(dir).then_some(false)
        };
        out.push((dir.to_owned(), depth, branch));
        for k in kids {
            self.push(&k, depth + 1, out);
        }
    }

    /// Starts naming a new folder inside the chosen one.
    pub fn begin_new_folder(&mut self) {
        self.new_folder = Some(String::new());
        self.new_folder_error = None;
        self.focus_new_folder = true;
    }

    /// Makes the folder being named inside the chosen one, and chooses it.
    pub fn create_folder(&mut self) -> Option<PathBuf> {
        let parent = self.dest.clone()?;
        let name = self.new_folder.clone()?.trim().to_owned();
        match new_folder(&parent, &name) {
            Ok(path) => {
                self.expanded.insert(parent.clone());
                self.children.remove(&parent);
                self.dest = Some(path.clone());
                self.new_folder = None;
                self.new_folder_error = None;
                Some(path)
            }
            Err(e) => {
                self.new_folder_error = Some(e);
                None
            }
        }
    }

    fn free_at(&mut self, dest: &Path) -> Option<u64> {
        if self.free.as_ref().is_none_or(|(d, _)| d != dest) {
            self.free = Some((dest.to_owned(), import::free_space(dest)));
        }
        self.free.as_ref().and_then(|(_, f)| *f)
    }
}

/// Makes `name` inside `parent`, or says why it can't.
pub fn new_folder(parent: &Path, name: &str) -> Result<PathBuf, String> {
    if name.is_empty() {
        return Err("Give the folder a name.".to_owned());
    }
    if name == "." || name == ".." || name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|'])
    {
        return Err("That name has a character a folder's name can't have.".to_owned());
    }
    let path = parent.join(name);
    if std::fs::symlink_metadata(&path).is_ok() {
        return Err(format!("“{name}” is already there."));
    }
    std::fs::create_dir(&path).map_err(|e| format!("The folder couldn't be made: {e}"))?;
    Ok(path)
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn files(n: usize) -> String {
    plural(n, "file", "files")
}

/// What a device holds: "412 photos, 18 videos, and 3 other files (9.4 GB)".
pub fn found_text(items: &[Item]) -> String {
    let count = |k: MediaKind| items.iter().filter(|i| i.kind == k).count();
    let mut parts = Vec::new();
    for (n, one, many) in [
        (count(MediaKind::Photo), "photo", "photos"),
        (count(MediaKind::Video), "video", "videos"),
        (count(MediaKind::Other), "other file", "other files"),
    ] {
        if n > 0 {
            parts.push(plural(n, one, many));
        }
    }
    let bytes: u64 = items.iter().map(|i| i.size).sum();
    match parts.len() {
        0 => "Nothing to import.".to_owned(),
        1 => format!("{} ({})", parts[0], human_size(bytes)),
        2 => format!("{} and {} ({})", parts[0], parts[1], human_size(bytes)),
        n => format!(
            "{}, and {} ({})",
            parts[..n - 1].join(", "),
            parts[n - 1],
            human_size(bytes)
        ),
    }
}

/// How an import went, for the dialog.
pub fn result_lines(
    outcome: &Outcome,
    dest: &Path,
    deleted: Option<&(usize, Vec<(String, String)>)>,
) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(match outcome.copied.len() {
        0 => "Nothing was copied.".to_owned(),
        n => format!("Copied {} into {}.", files(n), file_name(dest)),
    });
    match outcome.already.len() {
        0 => {}
        1 => lines.push("1 file was already there, and was skipped.".to_owned()),
        n => lines.push(format!("{n} files were already there, and were skipped.")),
    }
    if outcome.cancelled {
        lines.push("Stopped before the rest were copied.".to_owned());
    }
    if outcome.removed {
        lines.push(format!(
            "The device was removed: {} copied.",
            match outcome.failed.len() {
                1 => "1 file wasn't".to_owned(),
                n => format!("{n} files weren't"),
            }
        ));
    } else if !outcome.failed.is_empty() {
        lines.push(format!(
            "{} couldn't be copied:",
            files(outcome.failed.len())
        ));
        for (name, why) in outcome.failed.iter().take(5) {
            lines.push(format!("{name}: {why}"));
        }
        if outcome.failed.len() > 5 {
            lines.push(format!("and {} more.", outcome.failed.len() - 5));
        }
    }
    if let Some((n, failed)) = deleted {
        lines.push(format!("Deleted {} from the device.", files(*n)));
        if !failed.is_empty() {
            lines.push(format!(
                "{} couldn't be deleted and {} still on it.",
                files(failed.len()),
                if failed.len() == 1 { "is" } else { "are" }
            ));
        }
    }
    lines
}

fn bar(ui: &mut egui::Ui, frac: f32) {
    ui.add(egui::ProgressBar::new(frac.clamp(0.0, 1.0)).desired_width(ui.available_width()));
}

pub fn show(ctx: &egui::Context, state: &mut ImportState) -> Vec<ImportAction> {
    let mut out = Vec::new();
    let busy = state.running();
    let resp = Dialog::compact("medialiner-import", "Import Photos")
        .size(Size::Compact { width: WIDTH })
        .show(ctx, |ui| match state.phase {
            Phase::Choose => {
                // In a short window the dialog scrolls.
                let room = (ui.ctx().content_rect().height() - 150.0).max(240.0);
                egui::ScrollArea::vertical()
                    .id_salt("medialiner-import-body")
                    .max_height(room)
                    .min_scrolled_height(room)
                    .show(ui, |ui| choose(ui, state, &mut out));
            }
            _ => progress(ui, state, &mut out),
        });
    if resp.close && !busy {
        out.push(match state.phase {
            Phase::Done => ImportAction::Done,
            _ => ImportAction::Close,
        });
    }
    out
}

fn progress(ui: &mut egui::Ui, state: &mut ImportState, out: &mut Vec<ImportAction>) {
    let p = theme::palette(ui.ctx());
    let dest = state.dest.clone().unwrap_or_default();
    let device = state.volume().map(|v| v.name.clone()).unwrap_or_default();
    match &state.phase {
        Phase::Choose => {}
        Phase::Running {
            progress,
            cancelling,
        } => {
            let text = if *cancelling {
                "Stopping…".to_owned()
            } else {
                format!(
                    "Copying {} of {} ({} of {})",
                    (progress.done + 1).min(progress.total),
                    progress.total,
                    human_size(progress.bytes),
                    human_size(progress.total_bytes)
                )
            };
            ui.label(RichText::new(text).color(p.text_primary));
            ui.add_space(2.0);
            dialog::muted_text(ui, &progress.name);
            ui.add_space(6.0);
            let frac = if progress.total_bytes == 0 {
                progress.done as f32 / progress.total.max(1) as f32
            } else {
                progress.bytes as f32 / progress.total_bytes as f32
            };
            bar(ui, frac);
            ui.add_space(10.0);
            let cancelling = *cancelling;
            dialog::actions(ui, |ui| {
                if dialog::button(ui, "Cancel", ButtonKind::Secondary, !cancelling).clicked() {
                    out.push(ImportAction::Cancel);
                }
            });
        }
        Phase::AskDelete => {
            let copied = state.outcome.as_ref().map_or(0, |o| o.copied.len());
            if let Some(outcome) = &state.outcome {
                for line in result_lines(outcome, &dest, None) {
                    ui.label(RichText::new(line).color(p.text_primary));
                }
            }
            ui.add_space(10.0);
            ui.label(
                RichText::new(format!(
                    "Delete the {} from {device}?",
                    match copied {
                        1 => "copied file".to_owned(),
                        n => format!("{n} copied files"),
                    }
                ))
                .strong()
                .color(p.text_primary),
            );
            dialog::muted_text(
                ui,
                "Only files whose copies are in the folder are deleted. This can't be undone.",
            );
            ui.add_space(10.0);
            dialog::actions(ui, |ui| {
                if dialog::button(ui, "Delete from Device", ButtonKind::Danger, true).clicked() {
                    out.push(ImportAction::Delete(true));
                }
                if dialog::button(ui, "Keep on Device", ButtonKind::Secondary, true).clicked() {
                    out.push(ImportAction::Delete(false));
                }
            });
        }
        Phase::Deleting { done, total } => {
            ui.label(
                RichText::new(format!("Deleting {done} of {total} from {device}…"))
                    .color(p.text_primary),
            );
            ui.add_space(6.0);
            bar(ui, *done as f32 / (*total).max(1) as f32);
        }
        Phase::Done => {
            if let Some(outcome) = &state.outcome {
                for line in result_lines(outcome, &dest, state.deleted.as_ref()) {
                    ui.label(RichText::new(line).color(p.text_primary));
                }
            }
            ui.add_space(10.0);
            dialog::actions(ui, |ui| {
                if dialog::button(ui, "Done", ButtonKind::Primary, true).clicked() {
                    out.push(ImportAction::Done);
                }
            });
        }
    }
}

fn choose(ui: &mut egui::Ui, state: &mut ImportState, out: &mut Vec<ImportAction>) {
    let p = theme::palette(ui.ctx());
    if state.devices.is_empty() {
        let text = if state.looking {
            "Looking for devices…"
        } else {
            "No device found. Plug in a memory card, a USB drive, or a camera, and it will show here."
        };
        ui.label(RichText::new(text).color(p.text_primary));
        ui.add_space(4.0);
        dialog::muted_text(
            ui,
            "A drive has to be mounted by your desktop before MediaLiner can see it.",
        );
        ui.add_space(10.0);
        dialog::actions(ui, |ui| {
            if dialog::button(ui, "Close", ButtonKind::Secondary, true).clicked() {
                out.push(ImportAction::Close);
            }
        });
        return;
    }

    // --- From ---
    dialog::field_label(ui, "From");
    if state.devices.len() > 1 {
        let current = state.volume().map(|v| v.name.clone()).unwrap_or_default();
        let mut picked = state.device;
        egui::ComboBox::from_id_salt("medialiner-import-device")
            .selected_text(current)
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                for (i, v) in state.devices.iter().enumerate() {
                    ui.selectable_value(&mut picked, i, &v.name);
                }
            });
        if picked != state.device {
            out.push(ImportAction::SelectDevice(picked));
        }
    } else if let Some(v) = state.volume() {
        ui.label(RichText::new(&v.name).strong().color(p.text_primary));
    }
    let chosen = state.chosen();
    match &state.scan {
        Scan::Scanning(0) => dialog::muted_text(ui, "Looking at the device…"),
        Scan::Scanning(n) => dialog::muted_text(ui, &format!("Looking at the device… {n} files")),
        Scan::Failed(e) => dialog::error_text(ui, &format!("The device couldn't be read: {e}")),
        Scan::Ready { items, record } => {
            dialog::muted_text(ui, &found_text(items));
            if record.as_ref().is_some_and(|r| !r.files.is_empty()) {
                let all = import::choose(items, state.options, None, false).len();
                let new = import::choose(items, state.options, record.as_ref(), true).len();
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.radio_value(
                        &mut state.new_only,
                        true,
                        format!("New since the last import ({new})"),
                    );
                    ui.add_space(12.0);
                    ui.radio_value(&mut state.new_only, false, format!("All ({all})"));
                });
            }
        }
    }
    ui.add_space(4.0);
    file_list(ui, &chosen, matches!(state.scan, Scan::Ready { .. }));
    ui.add_space(10.0);

    // --- To ---
    dialog::field_label(ui, "To");
    if state.roots.is_empty() {
        dialog::muted_text(ui, "Add a folder to your list to import into.");
        ui.add_space(4.0);
        if dialog::icon_button(
            ui,
            Some(icons::FOLDER_PLUS),
            "Add Folder…",
            ButtonKind::Secondary,
            true,
        )
        .clicked()
        {
            out.push(ImportAction::AddFolder);
        }
    } else {
        folder_tree(ui, state);
        ui.add_space(6.0);
        new_folder_row(ui, state, out);
    }
    ui.add_space(8.0);

    // --- Options ---
    let mut options = state.options;
    settings::toggle_option(ui, "Include videos", &mut options.videos, true);
    settings::toggle_option(ui, "Include RAW and other files", &mut options.others, true);
    if options != state.options {
        state.options = options;
        out.push(ImportAction::Options(options));
    }
    settings::toggle_option(
        ui,
        "Delete from device after import (asks first)",
        &mut state.delete_after,
        true,
    );
    ui.add_space(8.0);

    // --- What will happen ---
    let bytes: u64 = chosen.iter().map(|i| i.size).sum();
    let mut ready = !chosen.is_empty() && state.new_folder.is_none();
    match (state.dest.clone(), &state.scan) {
        (_, Scan::Scanning(_)) | (_, Scan::Failed(_)) => ready = false,
        (None, _) => {
            ready = false;
            if !state.roots.is_empty() {
                dialog::muted_text(ui, "Choose the folder to import into.");
            }
        }
        (Some(dest), Scan::Ready { .. }) => {
            if chosen.is_empty() {
                dialog::muted_text(ui, "Nothing to import.");
            } else {
                ui.label(
                    RichText::new(format!(
                        "{} ({}) into {}",
                        files(chosen.len()),
                        human_size(bytes),
                        dest.display()
                    ))
                    .color(p.text_primary),
                );
                dialog::muted_text(
                    ui,
                    "Photos are copied. Nothing is overwritten, and a file that's already there is skipped.",
                );
            }
            if let Some(free) = state.free_at(&dest) {
                if free < bytes {
                    ready = false;
                    dialog::error_text(
                        ui,
                        &format!(
                            "Not enough space: {} needed, {} free.",
                            human_size(bytes),
                            human_size(free)
                        ),
                    );
                }
            }
        }
    }
    ui.add_space(10.0);
    dialog::actions(ui, |ui| {
        if dialog::button(ui, "Import", ButtonKind::Primary, ready).clicked() {
            out.push(ImportAction::Import);
        }
        if dialog::button(ui, "Cancel", ButtonKind::Secondary, true).clicked() {
            out.push(ImportAction::Close);
        }
    });
}

/// The files that will be copied: each name with its date.
fn file_list(ui: &mut egui::Ui, items: &[Item], scanned: bool) {
    let p = theme::palette(ui.ctx());
    egui::Frame::NONE
        .stroke(egui::Stroke::new(1.0, p.border))
        .corner_radius(laankit_egui::theme::shape(ui.ctx()).control())
        .inner_margin(egui::Margin::symmetric(8, 4))
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("medialiner-import-files")
                .max_height(ROW_HEIGHT * 5.0)
                .min_scrolled_height(ROW_HEIGHT * 5.0)
                .auto_shrink([false, false])
                .show_rows(ui, ROW_HEIGHT, items.len().max(1), |ui, range| {
                    if items.is_empty() {
                        let text = if scanned { "No files to import." } else { "" };
                        ui.label(RichText::new(text).size(13.0).color(p.text_muted));
                        return;
                    }
                    for item in &items[range] {
                        let date = import::date_text(item.modified);
                        ui.allocate_ui_with_layout(
                            egui::vec2(ui.available_width(), ROW_HEIGHT),
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                ui.label(RichText::new(&date).size(12.0).color(p.text_muted));
                                ui.with_layout(
                                    egui::Layout::left_to_right(egui::Align::Center),
                                    |ui| {
                                        let resp = ui
                                            .add(
                                                egui::Label::new(
                                                    RichText::new(&item.name)
                                                        .size(13.0)
                                                        .color(p.text_primary),
                                                )
                                                .selectable(false)
                                                .truncate(),
                                            )
                                            .on_hover_text(&item.rel);
                                        resp.widget_info(|| {
                                            egui::WidgetInfo::labeled(
                                                egui::Role::ListItem,
                                                true,
                                                format!("{} {date}", item.name),
                                            )
                                        });
                                    },
                                );
                            },
                        );
                    }
                });
        });
}

fn folder_tree(ui: &mut egui::Ui, state: &mut ImportState) {
    let p = theme::palette(ui.ctx());
    let rows = state.rows();
    let labels: Vec<String> = rows.iter().map(|(p, _, _)| file_name(p)).collect();
    let tree_rows: Vec<TreeRow<'_>> = rows
        .iter()
        .enumerate()
        .map(|(i, (path, depth, branch))| {
            let mut r = TreeRow::new(*depth, &labels[i])
                .selected(state.dest.as_deref() == Some(path.as_path()))
                .icon(icons::FOLDER);
            if let Some(open) = branch {
                r = r.branch(*open);
            }
            r
        })
        .collect();
    egui::Frame::NONE
        .stroke(egui::Stroke::new(1.0, p.border))
        .corner_radius(laankit_egui::theme::shape(ui.ctx()).control())
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("medialiner-import-tree")
                .max_height(130.0)
                .min_scrolled_height(130.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let result = tree::show(ui, "medialiner-import-tree", &tree_rows, |_, _| {});
                    if let Some(i) = result.toggled {
                        let path = &rows[i].0;
                        if !state.expanded.remove(path) {
                            state.expanded.insert(path.clone());
                        }
                    }
                    if let Some(i) = result.clicked {
                        state.dest = Some(rows[i].0.clone());
                        state.new_folder = None;
                        state.new_folder_error = None;
                    }
                });
        });
}

fn new_folder_row(ui: &mut egui::Ui, state: &mut ImportState, out: &mut Vec<ImportAction>) {
    let Some(mut name) = state.new_folder.clone() else {
        let can = state.dest.is_some();
        let resp = dialog::icon_button(
            ui,
            Some(icons::FOLDER_PLUS),
            "New Folder…",
            ButtonKind::Secondary,
            can,
        );
        if resp.clicked() {
            state.begin_new_folder();
        }
        return;
    };
    let parent = state.dest.as_deref().map(file_name).unwrap_or_default();
    dialog::muted_text(ui, &format!("New folder in {parent}"));
    let mut create = false;
    let mut cancel = false;
    ui.horizontal(|ui| {
        let width = (ui.available_width() - 190.0).max(120.0);
        let resp = ui
            .scope(|ui| {
                ui.set_width(width);
                dialog::text_input(ui, &mut name, "Folder name", true)
            })
            .inner;
        if std::mem::take(&mut state.focus_new_folder) {
            resp.request_focus();
        }
        create |= dialog::submitted(ui, &resp);
        create |= dialog::button(ui, "Create", ButtonKind::Primary, true).clicked();
        cancel |= dialog::button(ui, "Cancel", ButtonKind::Secondary, true).clicked();
    });
    state.new_folder = Some(name);
    if let Some(e) = &state.new_folder_error {
        dialog::error_text(ui, e);
    }
    if cancel {
        state.new_folder = None;
        state.new_folder_error = None;
    } else if create {
        if let Some(path) = state.create_folder() {
            out.push(ImportAction::FolderCreated(path));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use medialiner_core::import::Copied;

    fn item(name: &str, size: u64) -> Item {
        Item {
            id: name.into(),
            name: name.into(),
            rel: name.into(),
            size,
            modified: None,
            kind: MediaKind::of(Path::new(name)),
        }
    }

    #[test]
    fn says_what_was_found_and_how_it_went() {
        let items = [
            item("a.jpg", 1_000_000),
            item("b.jpg", 1_000_000),
            item("c.mp4", 3_000_000),
            item("d.arw", 500_000),
        ];
        assert_eq!(
            found_text(&items),
            "2 photos, 1 video, and 1 other file (5.5 MB)"
        );
        assert_eq!(found_text(&items[..1]), "1 photo (1.0 MB)");
        assert_eq!(found_text(&[]), "Nothing to import.");

        let outcome = Outcome {
            copied: vec![Copied {
                item: items[0].clone(),
                dest: "/p/Trip/a.jpg".into(),
            }],
            already: vec![items[1].clone()],
            failed: vec![("c.mp4".into(), "no room".into())],
            cancelled: false,
            removed: false,
        };
        let lines = result_lines(&outcome, Path::new("/p/Trip"), Some(&(1, Vec::new())));
        assert_eq!(
            lines,
            [
                "Copied 1 file into Trip.",
                "1 file was already there, and was skipped.",
                "1 file couldn't be copied:",
                "c.mp4: no room",
                "Deleted 1 file from the device.",
            ]
        );
    }

    #[test]
    fn new_folders_are_checked_and_made() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(new_folder(tmp.path(), "").is_err());
        assert!(new_folder(tmp.path(), "a/b").is_err());
        assert!(new_folder(tmp.path(), "..").is_err());
        let made = new_folder(tmp.path(), "2026 Trip").unwrap();
        assert!(made.is_dir());
        assert!(new_folder(tmp.path(), "2026 Trip")
            .unwrap_err()
            .contains("already there"));

        let mut state = ImportState::new(
            vec![tmp.path().to_owned()],
            Some(made.clone()),
            Options::default(),
        );
        assert!(state.expanded.contains(tmp.path()));
        state.begin_new_folder();
        state.new_folder = Some("  Day 1 ".into());
        let day = state.create_folder().unwrap();
        assert_eq!(day, made.join("Day 1"));
        assert_eq!(state.dest.as_deref(), Some(day.as_path()));
        assert!(state.expanded.contains(&made));
    }
}
