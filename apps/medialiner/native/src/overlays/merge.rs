//! Merge Into…: choose where a folder's contents go, see what will happen,
//! then watch it move.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use egui::RichText;
use laankit_egui::dialog::{self, ButtonKind, Dialog};
use laankit_egui::settings;
use laankit_egui::theme;
use laankit_egui::tree::{self, TreeRow};
use laankit_egui::icons;
use medialiner_core::info::human_size;
use medialiner_core::merge::{self as core_merge, MergePlan};
use medialiner_core::scan::{self, file_name};

pub enum Phase {
    Choose,
    Running {
        done: usize,
        total: usize,
        cancelling: bool,
    },
}

pub struct MergeState {
    pub source: PathBuf,
    pub dest: Option<PathBuf>,
    pub roots: Vec<PathBuf>,
    pub expanded: HashSet<PathBuf>,
    children: HashMap<PathBuf, Vec<PathBuf>>,
    pub plan: Option<Result<MergePlan, String>>,
    pub remove_source: bool,
    pub phase: Phase,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MergeAction {
    Close,
    /// Work out the plan for this destination.
    Choose(PathBuf),
    Merge,
    /// Stop the running merge.
    Cancel,
}

impl MergeState {
    /// The tree opens down to the source, so its neighbours are at hand.
    pub fn new(source: PathBuf, roots: Vec<PathBuf>) -> Self {
        let mut expanded = HashSet::new();
        if let Some(root) = roots.iter().filter(|r| source.starts_with(r)).max_by_key(|r| r.components().count()) {
            let mut at = source.parent();
            while let Some(dir) = at {
                if dir.starts_with(root) {
                    expanded.insert(dir.to_owned());
                }
                if dir == root.as_path() {
                    break;
                }
                at = dir.parent();
            }
        }
        Self {
            source,
            dest: None,
            roots,
            expanded,
            children: HashMap::new(),
            plan: None,
            remove_source: true,
            phase: Phase::Choose,
        }
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
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// What will move, for the dialog.
pub fn summary(plan: &MergePlan) -> Vec<String> {
    let mut parts = Vec::new();
    if plan.photos > 0 {
        parts.push(plural(plan.photos, "photo", "photos"));
    }
    if plan.others > 0 {
        parts.push(plural(plan.others, "other file", "other files"));
    }
    if plan.folders > 0 {
        parts.push(plural(plan.folders, "subfolder", "subfolders"));
    }
    let what = match parts.len() {
        0 => "Nothing to move: the folder is empty.".to_string(),
        1 => format!("Moves {} ({}).", parts[0], human_size(plan.bytes)),
        n => format!(
            "Moves {}, and {} ({}).",
            parts[..n - 1].join(", "),
            parts[n - 1],
            human_size(plan.bytes)
        ),
    };
    let mut lines = vec![what];
    if plan.clashes > 0 {
        lines.push(format!(
            "{} the same name as a file already there. Both are kept: the moved {} renamed, as in “photo (2).jpg”.",
            if plan.clashes == 1 { "1 file has".to_string() } else { format!("{} files have", plan.clashes) },
            if plan.clashes == 1 { "one is" } else { "ones are" },
        ));
    }
    if plan.duplicates > 0 {
        lines.push(format!(
            "{} identical to a file already there, so {} set aside in a hidden “{}” folder instead.",
            if plan.duplicates == 1 { "1 file is".to_string() } else { format!("{} files are", plan.duplicates) },
            if plan.duplicates == 1 { "it's" } else { "they're" },
            core_merge::DUPLICATES_DIR,
        ));
    }
    lines
}

pub fn show(ctx: &egui::Context, state: &mut MergeState) -> Vec<MergeAction> {
    let mut out = Vec::new();
    let title = format!("Merge “{}” Into…", file_name(&state.source));
    let running = matches!(state.phase, Phase::Running { .. });
    let resp = Dialog::compact("medialiner-merge", &title).show(ctx, |ui| {
        let p = theme::palette(ui.ctx());
        if let Phase::Running { done, total, cancelling } = state.phase {
            let text = if cancelling {
                "Stopping after the current file…".to_string()
            } else {
                format!("Moving {done} of {total}…")
            };
            ui.label(RichText::new(text).color(p.text_primary));
            ui.add_space(6.0);
            let frac = if total == 0 { 1.0 } else { done as f32 / total as f32 };
            ui.add(egui::ProgressBar::new(frac).desired_width(ui.available_width()));
            ui.add_space(10.0);
            dialog::actions(ui, |ui| {
                if dialog::button(ui, "Cancel", ButtonKind::Secondary, !cancelling).clicked() {
                    out.push(MergeAction::Cancel);
                }
            });
            return;
        }
        ui.label(
            RichText::new(format!(
                "Moves everything in {} into the folder you choose.",
                state.source.display()
            ))
            .color(p.text_secondary),
        );
        ui.add_space(8.0);
        dialog::field_label(ui, "Into");
        let rows = state.rows();
        let labels: Vec<String> = rows.iter().map(|(p, _, _)| file_name(p)).collect();
        let refused: Vec<bool> = rows
            .iter()
            .map(|(p, _, _)| core_merge::check(&state.source, p).is_err())
            .collect();
        let tree_rows: Vec<TreeRow<'_>> = rows
            .iter()
            .enumerate()
            .map(|(i, (path, depth, branch))| {
                let mut r = TreeRow::new(*depth, &labels[i])
                    .selected(state.dest.as_deref() == Some(path.as_path()))
                    .muted(refused[i])
                    .icon(icons::FOLDER);
                if let Some(open) = branch {
                    r = r.branch(*open);
                }
                r
            })
            .collect();
        egui::Frame::NONE
            .stroke(egui::Stroke::new(1.0, p.border))
            .corner_radius(theme::RADIUS)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("medialiner-merge-tree")
                    .max_height(220.0)
                    .min_scrolled_height(220.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let result = tree::show(ui, "medialiner-merge-tree", &tree_rows, |_, _| {});
                        if let Some(i) = result.toggled {
                            let path = &rows[i].0;
                            if !state.expanded.remove(path) {
                                state.expanded.insert(path.clone());
                            }
                        }
                        if let Some(i) = result.clicked {
                            if !refused[i] && state.dest.as_deref() != Some(rows[i].0.as_path()) {
                                out.push(MergeAction::Choose(rows[i].0.clone()));
                            }
                        }
                    });
            });
        ui.add_space(10.0);
        match (&state.dest, &state.plan) {
            (None, _) => dialog::muted_text(ui, "Choose the folder to move it into."),
            (Some(_), None) => dialog::muted_text(ui, "Working out what will move…"),
            (Some(_), Some(Err(e))) => dialog::error_text(ui, e),
            (Some(dest), Some(Ok(plan))) => {
                ui.label(RichText::new(format!("Into {}", dest.display())).color(p.text_primary));
                ui.add_space(4.0);
                for line in summary(plan) {
                    ui.label(RichText::new(line).size(13.0).color(p.text_secondary));
                    ui.add_space(2.0);
                }
                ui.add_space(6.0);
                let label = format!("Remove “{}” once it's empty", file_name(&state.source));
                settings::toggle_option(ui, &label, &mut state.remove_source, true);
                ui.add_space(2.0);
                dialog::muted_text(ui, "Nothing is overwritten, and you can undo the merge afterwards.");
            }
        }
        ui.add_space(10.0);
        let ready = matches!(&state.plan, Some(Ok(_))) && state.dest.is_some();
        dialog::actions(ui, |ui| {
            if dialog::button(ui, "Merge", ButtonKind::Primary, ready).clicked() {
                out.push(MergeAction::Merge);
            }
            if dialog::button(ui, "Cancel", ButtonKind::Secondary, true).clicked() {
                out.push(MergeAction::Close);
            }
        });
    });
    if resp.close && !running {
        out.push(MergeAction::Close);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summaries() {
        let plan = MergePlan {
            source: "/a".into(),
            dest: "/b".into(),
            duplicates_dir: "/d".into(),
            moves: Vec::new(),
            photos: 214,
            others: 1,
            folders: 3,
            bytes: 2_100_000_000,
            clashes: 4,
            duplicates: 1,
        };
        let s = summary(&plan);
        assert_eq!(s[0], "Moves 214 photos, 1 other file, and 3 subfolders (2.1 GB).");
        assert!(s[1].starts_with("4 files have"));
        assert!(s[2].starts_with("1 file is"));
    }
}
