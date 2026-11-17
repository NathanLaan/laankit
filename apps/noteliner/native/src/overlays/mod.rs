//! The dialogs. At most one [`Overlay`] is open at a time; each returns
//! what the user asked for and the app calls the backend.
//!
//! | Overlay                    | For                                            |
//! |----------------------------|------------------------------------------------|
//! | `NewFile`                  | New Item (a note, presentation, diagram or folder), Duplicate |
//! | `Move`                     | Move…, for a note, a diagram or a folder       |
//! | `DeleteFile`, `DeleteFolder`, `DeletePermanently` | the questions before deleting |
//! | `ClearTags`, `RemoveRecent`, `RemoveAttachment`   | the other confirmations |
//! | `Conflict`                 | a note changed on disk while it has unsaved edits |
//! | `Settings`                 | Settings ([`settings`])                        |
//! | `Help`                     | Help ([`help`])                                |
//! | `About`                    | the shared About dialog                        |
//! | `Busy`                     | "Importing …"                                  |
//! | `TagEditor`                | the tag colours ([`tags`])                     |
//!
//! MCP write confirmations ([`mcp::confirm`]) queue separately, on top of
//! whatever is open; the MCP walkthrough opens from Settings
//! ([`mcp::walkthrough`]).

pub mod help;
pub mod mcp;
pub mod settings;
pub mod tags;

use std::collections::BTreeSet;

use egui::RichText;
use laankit_egui::dialog::{self, ButtonKind, Confirm, Dialog};
#[cfg(test)]
use laankit_egui::fonts;
use laankit_egui::{icons, settings as shell, theme};

use noteliner_core::project::FolderCounts;
use noteliner_core::tree;

use crate::backend::DeleteTarget;
use crate::config::Recent;
use crate::model::Entry;

pub enum Overlay {
    NewFile(NewFileForm),
    Move(MoveForm),
    DeleteFile {
        id: String,
        name: String,
    },
    /// Delete a folder and everything in it.
    DeleteFolder {
        path: String,
        counts: FolderCounts,
    },
    /// The trash couldn't take it: delete it for good?
    DeletePermanently {
        target: DeleteTarget,
        name: String,
        message: String,
    },
    /// The note `id` changed on disk while it has unsaved edits: Reload
    /// takes `content`, Keep Mine saves the edits over it.
    Conflict {
        id: String,
        name: String,
        content: String,
    },
    ClearTags {
        id: String,
        name: String,
        count: usize,
    },
    RemoveRecent(Recent),
    Settings(Box<settings::SettingsState>),
    Help(Box<help::HelpState>),
    About,
    TagEditor(tags::TagEditorState),
    /// `Remove "name"?` for an attachment of note `file_id`.
    RemoveAttachment {
        file_id: String,
        attachment: crate::views::attachments::Attachment,
    },
    /// `PresentationSettingsModal` for note `id`.
    PresentationSettings {
        id: String,
        filename: String,
        dialog: Box<noteliner_slides::PresentationSettings>,
    },
    /// A hex value for the selected shapes' fill or stroke, in a diagram.
    Colour(blockliner_egui::dialogs::colour::ColourState),
    /// Export Diagram: PNG or SVG, the grid, and the scale.
    ExportDiagram(blockliner_egui::dialogs::export::ExportState),
    /// "Importing …": no way to close it.
    Busy {
        title: &'static str,
        message: String,
    },
}

/// What a small form asked for.
#[derive(Clone, Debug, PartialEq)]
pub enum FormAction {
    Close,
    Submit,
}

/// Focuses a dialog's first field when it opens: on the first frame it is
/// drawn, and again each time the dialog is opened after being closed.
fn focus_once(ui: &egui::Ui, resp: &egui::Response) {
    let id = resp.id.with("autofocused");
    let frame = ui.ctx().cumulative_frame_nr();
    let last = ui.data(|d| d.get_temp::<u64>(id));
    if last.is_none_or(|last| last + 1 < frame) {
        resp.request_focus();
    }
    ui.data_mut(|d| d.insert_temp(id, frame));
}

fn form_buttons(ui: &mut egui::Ui, ok: &str, ready: bool) -> Option<FormAction> {
    let mut out = None;
    dialog::actions(ui, |ui| {
        if dialog::button(ui, ok, ButtonKind::Primary, ready).clicked() {
            out = Some(FormAction::Submit);
        }
        if dialog::button(ui, "Cancel", ButtonKind::Secondary, true).clicked() {
            out = Some(FormAction::Close);
        }
    });
    out
}

// --- New Item / Duplicate --------------------------------------------------------

/// New Item (a note, a presentation, a diagram or a folder) or Duplicate.
pub struct NewFileForm {
    /// Duplicating this note (hides the template and type pickers).
    pub duplicate_of: Option<String>,
    pub name: String,
    pub tags: BTreeSet<String>,
    /// The folder the file goes in, by path; empty for the top.
    pub folder: String,
    /// `true` for Presentation (`fileType === 'deck'`).
    pub deck: bool,
    /// `true` for Diagram: a blank `.mmd`, with no template.
    pub diagram: bool,
    /// `true` for Folder: `name` is a folder's, made in `folder`, with no
    /// template or tags.
    pub new_folder: bool,
    pub template: Option<String>,
    /// The slide theme picked for a presentation. `None` is the theme of the
    /// template, or with no template, the app theme in use.
    pub theme: Option<String>,
    /// From `templates:list`: `{ id, name, kind }`.
    pub templates: Vec<serde_json::Value>,
}

impl NewFileForm {
    /// A new file in `folder`.
    pub fn new(name: &str, deck: bool, folder: &str) -> Self {
        Self {
            duplicate_of: None,
            name: name.to_string(),
            tags: BTreeSet::new(),
            folder: folder.to_string(),
            deck,
            diagram: false,
            new_folder: false,
            template: None,
            theme: None,
            templates: Vec::new(),
        }
    }

    /// New Diagram.
    pub fn diagram(name: &str, folder: &str) -> Self {
        Self {
            diagram: true,
            ..Self::new(name, false, folder)
        }
    }

    /// New Folder, in `parent`.
    pub fn folder(parent: &str) -> Self {
        Self {
            new_folder: true,
            ..Self::new("", false, parent)
        }
    }

    /// The new folder's path.
    pub fn path(&self) -> String {
        tree::join(&self.folder, self.name.trim())
    }

    /// Why the name can't be used for a folder, if it can't: the name
    /// rules, and a folder of that name already there (ignoring case).
    /// Nothing for the other types, whose names are made to fit.
    pub fn problem(&self, folders: &[String]) -> Option<String> {
        if !self.new_folder || self.name.trim().is_empty() {
            return None;
        }
        if let Err(e) = tree::check_folder_name(&self.name) {
            return Some(e.to_string());
        }
        let path = self.path().to_lowercase();
        folders
            .iter()
            .any(|f| f.to_lowercase() == path)
            .then(|| format!("A folder named {} is already there", self.name.trim()))
    }

    pub fn duplicate(source: &Entry) -> Self {
        Self {
            duplicate_of: Some(source.id.clone()),
            name: format!("{}-Copy", source.name),
            tags: source.tags.iter().cloned().collect(),
            // The copy goes beside its source.
            folder: source.folder.clone(),
            deck: false,
            diagram: false,
            new_folder: false,
            template: None,
            theme: None,
            templates: Vec::new(),
        }
    }

    /// Templates of the chosen type (`visibleTemplates`).
    fn visible_templates(&self) -> Vec<&serde_json::Value> {
        if self.diagram || self.new_folder {
            return Vec::new();
        }
        let kind = if self.deck { "deck" } else { "note" };
        self.templates
            .iter()
            .filter(|t| t["kind"].as_str().unwrap_or("note") == kind)
            .collect()
    }

    /// The slide theme a new presentation is given, where `app_theme` is the
    /// app theme in use. `None` leaves a template's own theme alone.
    pub fn deck_theme(&self, app_theme: &str) -> Option<String> {
        match (&self.theme, &self.template) {
            (Some(theme), _) => Some(theme.clone()),
            (None, Some(_)) => None,
            (None, None) => Some(app_theme.to_string()),
        }
    }
}

/// The label of the project folder itself, whose path is empty.
const TOP: &str = "(Top of the project)";

/// The Theme choice that keeps the theme a presentation template has.
const FROM_TEMPLATE: &str = "(From the template)";

/// Wide enough for the four type buttons on one line.
const NEW_ITEM_WIDTH: f32 = 520.0;
/// The Tags list shows about five tags and scrolls for more.
const TAG_LIST_HEIGHT: f32 = 130.0;

pub fn new_file(
    ctx: &egui::Context,
    form: &mut NewFileForm,
    folders: &[String],
    all_tags: &[String],
    app_theme: &str,
) -> Option<FormAction> {
    let duplicating = form.duplicate_of.is_some();
    // One title whatever the type: the Type row says which is chosen.
    let title = if duplicating { "Duplicate" } else { "New Item" };
    let resp = Dialog::compact("new-file", title)
        .size(dialog::Size::Compact {
            width: NEW_ITEM_WIDTH,
        })
        .show(ctx, |ui| {
            let p = theme::palette(ui.ctx());
            let mut submit = false;
            if !duplicating {
                dialog::field_label(ui, "Type");
                // Wrapped, so the four fit a narrow dialog at a large UI scale.
                ui.horizontal_wrapped(|ui| {
                    for (deck, diagram, folder, glyph, label) in [
                        (false, false, false, icons::FILE_LINES, "Note"),
                        (true, false, false, icons::PERSON_CHALKBOARD, "Presentation"),
                        (false, true, false, icons::OBJECT_GROUP, "Diagram"),
                        (false, false, true, icons::FOLDER, "Folder"),
                    ] {
                        let chosen = (form.deck, form.diagram, form.new_folder);
                        let kind = if chosen == (deck, diagram, folder) {
                            ButtonKind::Primary
                        } else {
                            ButtonKind::Secondary
                        };
                        // The name and the folder are kept across types.
                        if dialog::icon_button(ui, Some(glyph), label, kind, true).clicked() {
                            form.deck = deck;
                            form.diagram = diagram;
                            form.new_folder = folder;
                        }
                    }
                });
                // Keep the template valid when the type flips under it.
                if let Some(t) = form.template.clone() {
                    if !form
                        .visible_templates()
                        .iter()
                        .any(|v| v["id"] == t.as_str())
                    {
                        form.template = None;
                    }
                }
                ui.add_space(10.0);
            }
            dialog::field_label(ui, "Name");
            let r = dialog::text_input(ui, &mut form.name, "Untitled", true);
            focus_once(ui, &r);
            submit |= dialog::submitted(ui, &r);
            ui.add_space(10.0);

            dialog::field_label(ui, "Folder");
            {
                let mut options: Vec<(String, String)> = vec![(String::new(), TOP.into())];
                for (path, name, depth) in crate::model::folder_options(folders, None) {
                    options.push((path, format!("{}{name}", "\u{a0}\u{a0}".repeat(depth + 1))));
                }
                // A folder made a moment ago may not be listed yet.
                if !options.iter().any(|o| o.0 == form.folder) {
                    options.push((form.folder.clone(), form.folder.clone()));
                }
                let opts: Vec<(&str, &str)> = options
                    .iter()
                    .map(|(a, b)| (a.as_str(), b.as_str()))
                    .collect();
                if let Some(v) = shell::select(ui, "Folder", &opts, &form.folder) {
                    form.folder = v;
                }
            }

            let visible = form.visible_templates();
            if !duplicating && !form.diagram && (!visible.is_empty() || form.deck) {
                ui.add_space(10.0);
                dialog::field_label(ui, "Template");
                let blank = if form.deck {
                    "Blank Presentation"
                } else {
                    "Blank"
                };
                let mut options: Vec<(String, String)> = vec![(String::new(), blank.into())];
                for t in &visible {
                    options.push((
                        t["id"].as_str().unwrap_or_default().to_string(),
                        t["name"].as_str().unwrap_or_default().to_string(),
                    ));
                }
                let opts: Vec<(&str, &str)> = options
                    .iter()
                    .map(|(a, b)| (a.as_str(), b.as_str()))
                    .collect();
                if let Some(v) = shell::select(
                    ui,
                    "Template",
                    &opts,
                    form.template.as_deref().unwrap_or(""),
                ) {
                    form.template = (!v.is_empty()).then_some(v);
                    // A template's own theme stands until another is picked.
                    form.theme = None;
                }
            }

            if !duplicating && form.deck {
                ui.add_space(10.0);
                dialog::field_label(ui, "Theme");
                let mut opts = noteliner_slides::SlideTheme::options();
                if form.template.is_some() {
                    opts.insert(0, ("", FROM_TEMPLATE));
                }
                let shown = form.deck_theme(app_theme).unwrap_or_default();
                if let Some(v) = shell::select(ui, "Theme", &opts, &shown) {
                    form.theme = (!v.is_empty()).then_some(v);
                }
            }

            if !all_tags.is_empty() && !form.new_folder {
                ui.add_space(10.0);
                dialog::field_label(ui, "Tags");
                // Alphabetical whatever the case, one tag to a row.
                let mut sorted: Vec<&String> = all_tags.iter().collect();
                sorted.sort_by_key(|t| (t.to_lowercase(), t.as_str()));
                egui::Frame::new()
                    .fill(p.bg_surface)
                    .stroke(egui::Stroke::new(1.0, p.border))
                    .corner_radius(6)
                    .inner_margin(6)
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("new-file-tags")
                            .max_height(TAG_LIST_HEIGHT)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                for tag in sorted {
                                    let mut on = form.tags.contains(tag);
                                    if ui
                                        .checkbox(&mut on, RichText::new(tag).color(p.text_primary))
                                        .changed()
                                    {
                                        if on {
                                            form.tags.insert(tag.clone());
                                        } else {
                                            form.tags.remove(tag);
                                        }
                                    }
                                }
                            });
                    });
            }
            // A folder's name is checked against the folder picked above.
            let problem = form.problem(folders);
            if let Some(problem) = &problem {
                ui.add_space(6.0);
                dialog::error_text(ui, problem);
            }
            let ready = !form.name.trim().is_empty() && problem.is_none();
            let out = form_buttons(ui, "OK", ready);
            if submit && ready {
                return Some(FormAction::Submit);
            }
            out
        });
    if resp.close {
        return Some(FormAction::Close);
    }
    resp.inner
}

// --- Move -------------------------------------------------------------------------

/// What Move… moves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MoveTarget {
    /// A note or diagram, by id.
    File(String),
    /// A folder, by path.
    Folder(String),
}

/// Move…, with a searchable list of folders.
pub struct MoveForm {
    pub target: MoveTarget,
    pub name: String,
    /// The folder it is in now.
    pub current: String,
    /// The folder chosen, by path; empty for the top.
    pub folder: String,
    /// The search text.
    pub query: String,
    /// Focus the search and scroll to the chosen folder on the next frame.
    reveal: bool,
}

impl MoveForm {
    pub fn file(file: &Entry) -> Self {
        Self {
            target: MoveTarget::File(file.id.clone()),
            name: file.name.clone(),
            current: file.folder.clone(),
            folder: file.folder.clone(),
            query: String::new(),
            reveal: true,
        }
    }

    pub fn folder(path: &str) -> Self {
        let parent = tree::folder_of(path).to_string();
        Self {
            target: MoveTarget::Folder(path.to_string()),
            name: crate::model::folder_name(path).to_string(),
            current: parent.clone(),
            folder: parent,
            query: String::new(),
            reveal: true,
        }
    }

    /// The folder being moved: it and the folders under it aren't offered.
    fn excluded(&self) -> Option<&str> {
        match &self.target {
            MoveTarget::Folder(path) => Some(path),
            MoveTarget::File(_) => None,
        }
    }
}

/// The height of the folder list.
const FOLDER_LIST_HEIGHT: f32 = 196.0;
const FOLDER_ROW_HEIGHT: f32 = 28.0;

/// The folders to move to as `(path, label)`: every folder outside
/// `exclude` whose name contains `query`, ignoring case, with the top of the
/// project first. Without a query the labels are indented as a tree; with
/// one they are the folder's path, since the matches' parents may be
/// filtered out.
fn folder_matches(folders: &[String], exclude: Option<&str>, query: &str) -> Vec<(String, String)> {
    let query = query.trim().to_lowercase();
    let mut out = Vec::new();
    if TOP.to_lowercase().contains(&query) {
        out.push((String::new(), TOP.to_string()));
    }
    for (path, name, depth) in crate::model::folder_options(folders, exclude) {
        if query.is_empty() {
            out.push((path, format!("{}{name}", "\u{a0}\u{a0}".repeat(depth + 1))));
        } else if name.to_lowercase().contains(&query) {
            let label = path.replace('/', " / ");
            out.push((path, label));
        }
    }
    out
}

/// A row of the folder list.
fn folder_row(ui: &mut egui::Ui, label: &str, selected: bool) -> egui::Response {
    let p = theme::palette(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), FOLDER_ROW_HEIGHT),
        egui::Sense::click(),
    );
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::ListBoxOption, true, selected, label)
    });
    if selected {
        ui.painter().rect_filled(rect, 4.0, p.bg_selected);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, p.bg_item_hover);
    }
    ui.painter().with_clip_rect(rect).text(
        egui::pos2(rect.left() + 10.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(theme::BODY_SIZE),
        if selected { p.accent } else { p.text_primary },
    );
    resp
}

pub fn move_dialog(
    ctx: &egui::Context,
    form: &mut MoveForm,
    folders: &[String],
) -> Option<FormAction> {
    let title = match form.target {
        MoveTarget::File(_) => "Move File",
        MoveTarget::Folder(_) => "Move Folder",
    };
    let resp = Dialog::compact("move-file", title).show(ctx, |ui| {
        let p = theme::palette(ui.ctx());
        dialog::field_label(
            ui,
            match form.target {
                MoveTarget::File(_) => "File",
                MoveTarget::Folder(_) => "Folder",
            },
        );
        ui.label(RichText::new(&form.name).color(p.text_primary));
        ui.add_space(10.0);
        dialog::field_label(ui, "Current Folder");
        ui.label(
            RichText::new(if form.current.is_empty() {
                TOP
            } else {
                form.current.as_str()
            })
            .color(p.text_primary),
        );
        ui.add_space(10.0);
        dialog::field_label(ui, "Move To");
        let search = dialog::text_input(ui, &mut form.query, "Search folders…", true);
        let mut reveal = std::mem::take(&mut form.reveal);
        if reveal {
            search.request_focus();
        }

        let matches = folder_matches(folders, form.excluded(), &form.query);
        let mut at = matches.iter().position(|m| m.0 == form.folder);
        let mut pick = None;
        // Searching keeps a match chosen, so Enter moves to the first one.
        if at.is_none() && search.changed() {
            pick = (!matches.is_empty()).then_some(0);
        }
        let (up, down) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::ArrowUp),
                i.key_pressed(egui::Key::ArrowDown),
            )
        });
        if !matches.is_empty() && (up || down) {
            pick = Some(match at {
                Some(i) if down => (i + 1).min(matches.len() - 1),
                Some(i) => i.saturating_sub(1),
                None => 0,
            });
        }
        if let Some(i) = pick {
            form.folder = matches[i].0.clone();
            at = Some(i);
            reveal = true;
        }

        let mut submit = false;
        ui.add_space(2.0);
        egui::Frame::new()
            .fill(p.bg_surface)
            .stroke(egui::Stroke::new(1.0, p.border))
            .corner_radius(6)
            .inner_margin(4)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("move-file-folders")
                    .max_height(FOLDER_LIST_HEIGHT)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        if matches.is_empty() {
                            dialog::muted_text(ui, "No matching folders.");
                        }
                        for (i, (path, label)) in matches.iter().enumerate() {
                            let row = folder_row(ui, label, at == Some(i));
                            if row.clicked() {
                                form.folder = path.clone();
                            }
                            submit |= row.double_clicked();
                            if reveal && at == Some(i) {
                                row.scroll_to_me(None);
                            }
                        }
                    });
            });

        // The chosen folder must be one of the matches, i.e. visible.
        let ready = at.is_some() || matches.iter().any(|m| m.0 == form.folder);
        submit |= ui.input(|i| i.key_pressed(egui::Key::Enter));
        let out = form_buttons(ui, "OK", ready);
        if submit && ready {
            return Some(FormAction::Submit);
        }
        out
    });
    if resp.close {
        return Some(FormAction::Close);
    }
    resp.inner
}

// --- Confirmations -----------------------------------------------------------------

pub fn delete_file(ctx: &egui::Context, name: &str) -> Option<Confirm> {
    dialog::confirm(
        ctx,
        "delete-file",
        "Delete File",
        &format!("Are you sure you want to delete \"{name}\"?"),
        Some("The file and its attachments go to the trash."),
        "Delete",
    )
}

/// What a folder holds, in words: "3 notes, 1 diagram and 2 other files".
pub fn counts_text(c: &FolderCounts) -> String {
    let plural = |n: usize, w: &str| format!("{n} {w}{}", if n == 1 { "" } else { "s" });
    let mut parts = Vec::new();
    if c.notes > 0 {
        parts.push(plural(c.notes, "note"));
    }
    if c.diagrams > 0 {
        parts.push(plural(c.diagrams, "diagram"));
    }
    if c.others > 0 {
        parts.push(plural(c.others, "other file"));
    }
    match parts.len() {
        0 => "nothing".to_string(),
        1 => parts.remove(0),
        _ => {
            let last = parts.pop().unwrap_or_default();
            format!("{} and {last}", parts.join(", "))
        }
    }
}

/// The question before a folder is deleted, with what is inside it.
pub fn delete_folder(ctx: &egui::Context, path: &str, counts: &FolderCounts) -> Option<Confirm> {
    let name = crate::model::folder_name(path);
    let empty = counts.notes + counts.diagrams + counts.others == 0;
    dialog::confirm(
        ctx,
        "delete-folder",
        "Delete Folder",
        &format!("Are you sure you want to delete the folder \"{name}\"?"),
        Some(&if empty {
            "The folder is empty. It goes to the trash.".to_string()
        } else {
            format!(
                "It holds {}. The folder and everything in it go to the trash.",
                counts_text(counts)
            )
        }),
        "Delete",
    )
}

/// The trash couldn't take it: ask before removing anything.
pub fn delete_permanently(ctx: &egui::Context, name: &str, message: &str) -> Option<Confirm> {
    dialog::confirm(
        ctx,
        "delete-permanently",
        "Delete Permanently?",
        &format!("\"{name}\" can't be moved to the trash. Delete it permanently?"),
        Some(&format!("{message}\nThis action cannot be undone.")),
        "Delete Permanently",
    )
}

/// What the user chose for a note that changed on disk under their edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictAction {
    /// Take the file from disk; the unsaved edits are dropped.
    Reload,
    /// Save the edits over the file on disk.
    KeepMine,
}

/// A note changed on disk while it has unsaved edits. There is no way to
/// close this without choosing: nothing is lost without asking.
pub fn conflict(ctx: &egui::Context, name: &str) -> Option<ConflictAction> {
    Dialog::compact("file-conflict", "File Changed on Disk")
        .show(ctx, |ui| {
            let p = theme::palette(ui.ctx());
            ui.label(
                RichText::new(format!(
                    "\"{name}\" was changed by another program, and you have unsaved edits."
                ))
                .color(p.text_primary),
            );
            ui.add_space(6.0);
            dialog::muted_text(
                ui,
                "Reload takes the file from disk and drops your edits. Keep Mine saves your edits over it.",
            );
            let mut out = None;
            dialog::actions(ui, |ui| {
                if dialog::button(ui, "Keep Mine", ButtonKind::Primary, true).clicked() {
                    out = Some(ConflictAction::KeepMine);
                }
                if dialog::button(ui, "Reload", ButtonKind::Secondary, true).clicked() {
                    out = Some(ConflictAction::Reload);
                }
            });
            out
        })
        .inner
}

pub fn clear_tags(ctx: &egui::Context, name: &str, count: usize) -> Option<Confirm> {
    dialog::confirm(
        ctx,
        "clear-tags",
        "Clear Tags",
        &format!(
            "Remove all {count} tag{} from \"{name}\"?",
            if count == 1 { "" } else { "s" }
        ),
        None,
        "Clear Tags",
    )
}

pub fn remove_recent(ctx: &egui::Context, r: &Recent) -> Option<Confirm> {
    dialog::confirm(
        ctx,
        "remove-recent",
        "Remove Recent Project",
        &format!("Remove \"{}\" from the recent projects list?", r.name),
        Some(&format!(
            "{}\nThis only removes it from the list. The project files are not deleted.",
            r.path
        )),
        "Remove",
    )
}

// --- Busy -----------------------------------------------------------------------

/// "Importing …": a spinner and a message, no close.
pub fn busy(ctx: &egui::Context, title: &str, message: &str) {
    Dialog::compact(("busy", title), title).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.spinner();
            dialog::muted_text(ui, message);
        });
    });
}

/// `Remove "name"?` for an attachment.
pub fn remove_attachment(ctx: &egui::Context, name: &str) -> Option<Confirm> {
    dialog::confirm(
        ctx,
        "remove-attachment",
        "Remove Attachment",
        &format!("Remove \"{name}\"?"),
        Some("The file goes to the trash."),
        "Remove",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_presentation_takes_the_app_theme_unless_told() {
        let mut f = NewFileForm::new("Talk", true, "");
        assert_eq!(f.deck_theme("midnight").as_deref(), Some("midnight"));
        // A template keeps its own theme until one is picked.
        f.template = Some("pitch.md".into());
        assert_eq!(f.deck_theme("midnight"), None);
        f.theme = Some("light".into());
        assert_eq!(f.deck_theme("midnight").as_deref(), Some("light"));
    }

    #[test]
    fn duplicate_defaults() {
        let e = Entry {
            id: "a".into(),
            name: "Alpha".into(),
            tags: vec!["x".into()],
            filename: "Work/alpha.md".into(),
            folder: "Work".into(),
            ..Default::default()
        };
        let f = NewFileForm::duplicate(&e);
        assert_eq!(f.name, "Alpha-Copy");
        assert!(f.tags.contains("x"));
        // The copy goes beside its source.
        assert_eq!(f.folder, "Work");
        assert_eq!(NewFileForm::diagram("", "Work/Sub").folder, "Work/Sub");
    }

    /// Projects ─ Garden ─ Tomatoes, and Recipes.
    fn folders() -> Vec<String> {
        [
            "Projects",
            "Projects/Garden",
            "Projects/Garden/Tomatoes",
            "Recipes",
        ]
        .map(String::from)
        .to_vec()
    }

    #[test]
    fn folder_search() {
        let folders = folders();
        let labels = |q: &str, exclude: Option<&str>| -> Vec<String> {
            folder_matches(&folders, exclude, q)
                .into_iter()
                .map(|m| m.1)
                .collect()
        };
        // No query: the tree, indented under the top of the project.
        assert_eq!(
            labels("", None),
            [
                TOP,
                "\u{a0}\u{a0}Projects",
                "\u{a0}\u{a0}\u{a0}\u{a0}Garden",
                "\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}Tomatoes",
                "\u{a0}\u{a0}Recipes"
            ]
        );
        // A query matches names, ignoring case, and labels them by path.
        assert_eq!(labels(" TOMA ", None), ["Projects / Garden / Tomatoes"]);
        assert_eq!(labels("top", None), [TOP]);
        assert!(labels("zzz", None).is_empty());
        // A moved folder, and the folders under it, are never offered.
        assert!(labels("tom", Some("Projects")).is_empty());
        assert_eq!(labels("", Some("Projects/Garden")).len(), 3);
    }

    #[test]
    fn move_forms_start_where_the_thing_is() {
        let e = Entry {
            id: "a".into(),
            name: "Alpha".into(),
            folder: "Work".into(),
            ..Default::default()
        };
        let f = MoveForm::file(&e);
        assert_eq!((f.current.as_str(), f.folder.as_str()), ("Work", "Work"));
        assert_eq!(f.excluded(), None);
        let f = MoveForm::folder("Projects/Garden");
        assert_eq!(f.name, "Garden");
        assert_eq!(f.current, "Projects");
        assert_eq!(f.excluded(), Some("Projects/Garden"));
        assert_eq!(MoveForm::folder("Recipes").current, "");
    }

    #[test]
    fn new_folder_names() {
        let folders = folders();
        let form = |parent: &str, name: &str| NewFileForm {
            name: name.into(),
            ..NewFileForm::folder(parent)
        };
        assert_eq!(form("Projects", " Shed ").path(), "Projects/Shed");
        assert_eq!(form("", "Shed").path(), "Shed");
        assert_eq!(form("", "").problem(&folders), None);
        assert_eq!(form("", "Shed").problem(&folders), None);
        // The name rules, and a folder already there, ignoring case.
        for bad in [".hidden", "_x", "a/b", "what?", ".."] {
            assert!(form("", bad).problem(&folders).is_some(), "{bad}");
        }
        assert!(form("", "recipes").problem(&folders).is_some());
        assert!(form("Projects", "GARDEN").problem(&folders).is_some());
        assert!(form("Recipes", "Garden").problem(&folders).is_none());
        // A note's name has no such rules, and a folder has no templates.
        let mut note = form("", "recipes");
        note.templates = vec![serde_json::json!({ "id": "t.md", "name": "T", "kind": "note" })];
        assert!(note.visible_templates().is_empty());
        note.new_folder = false;
        assert_eq!(note.problem(&folders), None);
        assert_eq!(note.visible_templates().len(), 1);
    }

    #[test]
    fn folder_counts_in_words() {
        let c = |notes, diagrams, others| FolderCounts {
            notes,
            diagrams,
            others,
        };
        assert_eq!(counts_text(&c(0, 0, 0)), "nothing");
        assert_eq!(counts_text(&c(1, 0, 0)), "1 note");
        assert_eq!(counts_text(&c(3, 1, 0)), "3 notes and 1 diagram");
        assert_eq!(
            counts_text(&c(3, 1, 2)),
            "3 notes, 1 diagram and 2 other files"
        );
    }

    #[test]
    fn move_dialog_filters_and_submits_the_first_match() {
        use egui_kittest::kittest::Queryable;

        struct State {
            form: MoveForm,
            folders: Vec<String>,
            action: Option<FormAction>,
        }
        let folders = folders();
        let form = MoveForm::folder("Recipes");
        let mut h = egui_kittest::Harness::builder()
            .with_context_setup(fonts::install)
            .with_size(egui::vec2(800.0, 700.0))
            .build_ui_state(
                |ui, s: &mut State| {
                    if let Some(a) = move_dialog(ui.ctx(), &mut s.form, &s.folders) {
                        s.action = Some(a);
                    }
                },
                State {
                    form,
                    folders,
                    action: None,
                },
            );
        h.run_steps(3);
        assert!(h.query_by_label_contains("Tomatoes").is_some());
        // The folder being moved isn't in the list.
        assert!(h
            .query_by_role_and_label(egui::accesskit::Role::ListBoxOption, "\u{a0}\u{a0}Recipes")
            .is_none());

        // The search is focused when the dialog opens.
        h.get_by_role(egui::accesskit::Role::TextInput)
            .type_text("gard");
        h.run_steps(3);
        assert!(h.query_by_label("Projects / Garden").is_some());
        assert!(h.query_by_label_contains("Tomatoes").is_none());
        assert_eq!(h.state().form.folder, "Projects/Garden");

        h.key_press(egui::Key::Enter);
        h.run_steps(2);
        assert!(matches!(h.state().action, Some(FormAction::Submit)));
    }
}
