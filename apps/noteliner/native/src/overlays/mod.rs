//! The dialogs, one per Svelte modal. At most one [`Overlay`] is open at a
//! time; each returns what the user asked for and the app calls the backend.
//!
//! | Overlay                    | Svelte                                         |
//! |----------------------------|------------------------------------------------|
//! | `NewProject`               | `NewProjectModal`                              |
//! | `Setup`                    | `SetupModal`                                   |
//! | `NewFile`                  | `NewFileModal` (New File, New Presentation, Duplicate) |
//! | `MoveFile`                 | `MoveFileModal`                                |
//! | `DeleteFile`, `ClearTags`, `RemoveRecent` | `DeleteFileModal`, `ClearTagsModal`, `RemoveRecentModal` |
//! | `ProjectSettings`          | `ProjectSettingsModal`                         |
//! | `Settings`                 | `SettingsModal` ([`settings`])                 |
//! | `Sync`                     | `SyncModal` ([`sync`])                         |
//! | `Help`                     | the Help window (`HelpApp.svelte`), in-app ([`help`]) |
//! | `About`                    | desktop-ui's `AboutModal`                      |
//! | `Busy`                     | `SyncingModal`, `ImportingModal`               |
//! | `TagEditor`                | `TagEditorModal` ([`tags`])                    |
//! | `RemoveAttachment`         | the Attachments panel's `confirm()`            |
//!
//! MCP write confirmations ([`mcp::confirm`]) queue separately, on top of
//! whatever is open, as in `App.svelte`; the MCP walkthrough opens from
//! Settings ([`mcp::walkthrough`]).

pub mod help;
pub mod mcp;
pub mod settings;
pub mod sync;
pub mod tags;

use std::collections::BTreeSet;

use egui::RichText;
use marina_egui::dialog::{self, ButtonKind, Confirm, Dialog};
use marina_egui::{fonts, icons, settings as shell, theme};

use crate::config::Recent;
use crate::model::Entry;

pub enum Overlay {
    NewProject(NewProjectForm),
    Setup(SetupForm),
    NewFile(NewFileForm),
    MoveFile(MoveForm),
    DeleteFile {
        id: String,
        name: String,
    },
    ClearTags {
        id: String,
        name: String,
        count: usize,
    },
    RemoveRecent(Recent),
    ProjectSettings(ProjectSettingsForm),
    Settings(Box<settings::SettingsState>),
    Sync(sync::SyncState),
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
    /// "Syncing changes to remote..." / "Importing …": no way to close it.
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

/// Focuses a dialog's first field when it opens (`autofocus`).
fn focus_once(ui: &egui::Ui, resp: &egui::Response) {
    let id = resp.id.with("autofocused");
    if !ui.data(|d| d.get_temp::<bool>(id).unwrap_or(false)) {
        resp.request_focus();
        ui.data_mut(|d| d.insert_temp(id, true));
    }
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

// --- New Project ------------------------------------------------------------

/// `NewProjectModal`: location, git user name and email.
pub struct NewProjectForm {
    pub location: String,
    pub name: String,
    pub email: String,
    pub error: Option<String>,
}

impl NewProjectForm {
    /// The Svelte defaults: `~/docs/new-project-YYYY-MM-DD`, the user name and
    /// `user@host`.
    pub fn with_defaults() -> Self {
        let (user, host, home) = crate::backend::system_info();
        let today = chrono::Local::now().format("%Y-%m-%d");
        Self {
            location: home
                .join("docs")
                .join(format!("new-project-{today}"))
                .display()
                .to_string(),
            email: format!("{user}@{host}"),
            name: user,
            error: None,
        }
    }

    /// `handleOk`'s checks; `None` when the form is complete.
    pub fn validate(&self) -> Option<&'static str> {
        if self.location.trim().is_empty() {
            Some("Project location is required.")
        } else if self.name.trim().is_empty() || self.email.trim().is_empty() {
            Some("User name and email are required.")
        } else {
            None
        }
    }
}

/// Also returns `Some(Browse)` through `browse` when the folder button is clicked.
pub fn new_project(
    ctx: &egui::Context,
    form: &mut NewProjectForm,
    browse: &mut bool,
) -> Option<FormAction> {
    let resp = Dialog::compact("new-project", "New Project").show(ctx, |ui| {
        if let Some(e) = &form.error {
            dialog::error_text(ui, e);
            ui.add_space(8.0);
        }
        let mut submit = false;
        dialog::field_label(ui, "Project Location");
        ui.horizontal(|ui| {
            ui.allocate_ui(egui::vec2(ui.available_width() - 44.0, 34.0), |ui| {
                let r = dialog::text_input(ui, &mut form.location, "", true);
                focus_once(ui, &r);
                submit |= dialog::submitted(ui, &r);
            });
            if dialog::icon_button(
                ui,
                Some(icons::FOLDER_OPEN),
                "",
                ButtonKind::Secondary,
                true,
            )
            .on_hover_text("Browse for folder")
            .clicked()
            {
                *browse = true;
            }
        });
        ui.add_space(10.0);
        dialog::field_label(ui, "User Name");
        let r = dialog::text_input(ui, &mut form.name, "", true);
        submit |= dialog::submitted(ui, &r);
        ui.add_space(10.0);
        dialog::field_label(ui, "User Email");
        let r = dialog::text_input(ui, &mut form.email, "", true);
        submit |= dialog::submitted(ui, &r);
        let out = form_buttons(ui, "OK", true);
        if submit {
            return Some(FormAction::Submit);
        }
        out
    });
    if resp.close {
        return Some(FormAction::Close);
    }
    resp.inner
}

// --- Set Up Project ------------------------------------------------------------

/// `SetupModal`: an existing folder that isn't a NoteLiner project yet.
pub struct SetupForm {
    pub folder: String,
    /// Entering a URL to clone (`mode === 'remote'`).
    pub remote: bool,
    pub url: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SetupAction {
    Cancel,
    /// Create a new repository (`None`) or clone this URL.
    Setup(Option<String>),
}

pub fn setup(ctx: &egui::Context, form: &mut SetupForm) -> Option<SetupAction> {
    let resp = Dialog::compact("setup", "Set Up Project").show(ctx, |ui| {
        let p = theme::palette(ui.ctx());
        ui.label(
            RichText::new(&form.folder)
                .font(fonts::mono(12.0))
                .color(p.text_secondary),
        );
        ui.add_space(10.0);
        let mut out = None;
        if !form.remote {
            ui.label(
                RichText::new(
                    "This folder is not a NoteLiner project. How would you like to set it up?",
                )
                .color(p.text_primary),
            );
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if dialog::icon_button(
                    ui,
                    Some(icons::PLUS),
                    "Create New Repository",
                    ButtonKind::Secondary,
                    true,
                )
                .clicked()
                {
                    out = Some(SetupAction::Setup(None));
                }
                if dialog::icon_button(
                    ui,
                    Some(icons::CLOUD_ARROW_DOWN),
                    "Clone from Remote",
                    ButtonKind::Secondary,
                    true,
                )
                .clicked()
                {
                    form.remote = true;
                }
            });
        } else {
            ui.label(RichText::new("Enter the remote Git repository URL:").color(p.text_primary));
            ui.add_space(6.0);
            let r = dialog::text_input(ui, &mut form.url, "https://github.com/user/repo.git", true);
            focus_once(ui, &r);
            let url = form.url.trim().to_string();
            if dialog::submitted(ui, &r) && !url.is_empty() {
                out = Some(SetupAction::Setup(Some(url.clone())));
            }
            dialog::actions(ui, |ui| {
                if dialog::button(ui, "Clone", ButtonKind::Primary, !url.is_empty()).clicked() {
                    out = Some(SetupAction::Setup(Some(url.clone())));
                }
                if dialog::button(ui, "Back", ButtonKind::Secondary, true).clicked() {
                    form.remote = false;
                }
            });
        }
        ui.add_space(8.0);
        dialog::actions(ui, |ui| {
            if dialog::button(ui, "Cancel", ButtonKind::Secondary, true).clicked() {
                out = Some(SetupAction::Cancel);
            }
        });
        out
    });
    if resp.close {
        return Some(SetupAction::Cancel);
    }
    resp.inner
}

// --- New File / Duplicate --------------------------------------------------------

/// `NewFileModal`, in New File, New Presentation or Duplicate mode.
pub struct NewFileForm {
    /// Duplicating this note (hides the template and type pickers).
    pub duplicate_of: Option<String>,
    pub name: String,
    pub tags: BTreeSet<String>,
    pub parent: Option<String>,
    /// `true` for Presentation (`fileType === 'deck'`).
    pub deck: bool,
    pub template: Option<String>,
    /// From `templates:list`: `{ id, name, kind }`.
    pub templates: Vec<serde_json::Value>,
}

impl NewFileForm {
    pub fn new(name: &str, deck: bool) -> Self {
        Self {
            duplicate_of: None,
            name: name.to_string(),
            tags: BTreeSet::new(),
            parent: None,
            deck,
            template: None,
            templates: Vec::new(),
        }
    }

    pub fn duplicate(source: &Entry) -> Self {
        Self {
            duplicate_of: Some(source.id.clone()),
            name: format!("{}-Copy", source.name),
            tags: source.tags.iter().cloned().collect(),
            parent: source.parent_id.clone(),
            deck: false,
            template: None,
            templates: Vec::new(),
        }
    }

    /// Templates of the chosen type (`visibleTemplates`).
    fn visible_templates(&self) -> Vec<&serde_json::Value> {
        let kind = if self.deck { "deck" } else { "note" };
        self.templates
            .iter()
            .filter(|t| t["kind"].as_str().unwrap_or("note") == kind)
            .collect()
    }
}

pub fn new_file(
    ctx: &egui::Context,
    form: &mut NewFileForm,
    entries: &[Entry],
    all_tags: &[String],
    current: Option<&Entry>,
) -> Option<FormAction> {
    let duplicating = form.duplicate_of.is_some();
    let title = match (duplicating, form.deck) {
        (true, _) => "Duplicate",
        (false, true) => "New Presentation",
        (false, false) => "New File",
    };
    let resp = Dialog::compact("new-file", title).show(ctx, |ui| {
        let p = theme::palette(ui.ctx());
        let mut submit = false;
        if !duplicating {
            dialog::field_label(ui, "Type");
            ui.horizontal(|ui| {
                for (deck, glyph, label) in [
                    (false, icons::FILE_LINES, "Note"),
                    (true, icons::PERSON_CHALKBOARD, "Presentation"),
                ] {
                    let kind = if form.deck == deck {
                        ButtonKind::Primary
                    } else {
                        ButtonKind::Secondary
                    };
                    if dialog::icon_button(ui, Some(glyph), label, kind, true).clicked() {
                        form.deck = deck;
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
        dialog::field_label(ui, "File Name");
        let r = dialog::text_input(ui, &mut form.name, "Untitled", true);
        focus_once(ui, &r);
        submit |= dialog::submitted(ui, &r);
        ui.add_space(10.0);

        dialog::field_label(ui, "Parent");
        ui.horizontal(|ui| {
            let mut options: Vec<(String, String)> = vec![(String::new(), "(None)".into())];
            for (id, name, depth) in crate::model::parent_options(entries, None) {
                options.push((id, format!("{}{name}", "\u{a0}\u{a0}".repeat(depth))));
            }
            let opts: Vec<(&str, &str)> = options
                .iter()
                .map(|(a, b)| (a.as_str(), b.as_str()))
                .collect();
            ui.scope(|ui| {
                ui.set_max_width(ui.available_width() - 120.0);
                if let Some(v) =
                    shell::select(ui, "Parent", &opts, form.parent.as_deref().unwrap_or(""))
                {
                    form.parent = (!v.is_empty()).then_some(v);
                }
            });
            let label = current.map_or("No file selected in FILES".to_string(), |c| {
                format!("Set parent to \"{}\"", c.name)
            });
            if dialog::button(ui, "Current File", ButtonKind::Secondary, current.is_some())
                .on_hover_text(label)
                .clicked()
            {
                form.parent = current.map(|c| c.id.clone());
            }
        });

        let visible = form.visible_templates();
        if !duplicating && (!visible.is_empty() || form.deck) {
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
            }
        }

        if !all_tags.is_empty() {
            ui.add_space(10.0);
            dialog::field_label(ui, "Tags");
            ui.horizontal_wrapped(|ui| {
                for tag in all_tags {
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
        }
        let ready = !form.name.trim().is_empty();
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

// --- Move File -----------------------------------------------------------------

/// `MoveFileModal`.
pub struct MoveForm {
    pub id: String,
    pub name: String,
    pub current_parent: String,
    pub parent: Option<String>,
}

impl MoveForm {
    pub fn new(file: &Entry, entries: &[Entry]) -> Self {
        let current_parent = file
            .parent_id
            .as_ref()
            .and_then(|p| entries.iter().find(|e| &e.id == p))
            .map_or("(None)".to_string(), |e| e.name.clone());
        Self {
            id: file.id.clone(),
            name: file.name.clone(),
            current_parent,
            parent: file.parent_id.clone(),
        }
    }
}

pub fn move_file(
    ctx: &egui::Context,
    form: &mut MoveForm,
    entries: &[Entry],
) -> Option<FormAction> {
    let resp = Dialog::compact("move-file", "Move File").show(ctx, |ui| {
        let p = theme::palette(ui.ctx());
        dialog::field_label(ui, "File");
        ui.label(RichText::new(&form.name).color(p.text_primary));
        ui.add_space(10.0);
        dialog::field_label(ui, "Current Parent");
        ui.label(RichText::new(&form.current_parent).color(p.text_primary));
        ui.add_space(10.0);
        dialog::field_label(ui, "New Parent");
        let mut options: Vec<(String, String)> = vec![(String::new(), "(None)".into())];
        for (id, name, depth) in crate::model::parent_options(entries, Some(&form.id)) {
            options.push((id, format!("{}{name}", "\u{a0}\u{a0}".repeat(depth))));
        }
        let opts: Vec<(&str, &str)> = options
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        if let Some(v) = shell::select(
            ui,
            "New Parent",
            &opts,
            form.parent.as_deref().unwrap_or(""),
        ) {
            form.parent = (!v.is_empty()).then_some(v);
        }
        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
        let out = form_buttons(ui, "OK", true);
        if enter {
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
        Some("This action cannot be undone."),
        "Delete",
    )
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

// --- Project Settings ----------------------------------------------------------

/// `ProjectSettingsModal`: the project folder and the repo's git identity.
#[derive(Default)]
pub struct ProjectSettingsForm {
    pub folder: String,
    pub name: String,
    pub email: String,
    pub error: Option<String>,
    /// Opened because a commit needs an identity (`required`).
    pub required: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ProjectSettingsAction {
    Close,
    Save,
    OpenFolder,
}

pub fn project_settings(
    ctx: &egui::Context,
    form: &mut ProjectSettingsForm,
    top_inset: f32,
) -> Option<ProjectSettingsAction> {
    let resp = Dialog::full("project-settings", "Project Settings")
        .size(dialog::Size::Full { top_inset })
        .show(ctx, |ui| {
            let p = theme::palette(ui.ctx());
            let mut out = None;
            if form.required {
                ui.label(
                    RichText::new("Git requires a name and email to save changes. These are stored in the local repository config.")
                        .color(p.accent),
                );
                ui.add_space(12.0);
            }
            dialog::field_label(ui, "Project Location");
            ui.horizontal(|ui| {
                ui.label(RichText::new(&form.folder).font(fonts::mono(12.0)).color(p.text_secondary));
                if dialog::icon_button(ui, Some(icons::FOLDER_OPEN), "", ButtonKind::Secondary, true)
                    .on_hover_text("Open in file manager")
                    .clicked()
                {
                    out = Some(ProjectSettingsAction::OpenFolder);
                }
            });
            ui.add_space(16.0);
            dialog::field_label(ui, "Git Configuration");
            if let Some(e) = &form.error {
                dialog::error_text(ui, e);
                ui.add_space(6.0);
            }
            ui.label(RichText::new("User Name").size(12.0).color(p.text_secondary));
            let r = dialog::text_input(ui, &mut form.name, "Your Name", true);
            focus_once(ui, &r);
            let mut submit = dialog::submitted(ui, &r);
            ui.add_space(8.0);
            ui.label(RichText::new("User Email").size(12.0).color(p.text_secondary));
            let r = dialog::text_input(ui, &mut form.email, "you@example.com", true);
            submit |= dialog::submitted(ui, &r);
            dialog::actions(ui, |ui| {
                if dialog::button(ui, "OK", ButtonKind::Primary, true).clicked() {
                    submit = true;
                }
            });
            if submit {
                out = Some(ProjectSettingsAction::Save);
            }
            out
        });
    if resp.close {
        return Some(ProjectSettingsAction::Close);
    }
    resp.inner
}

impl ProjectSettingsForm {
    /// `handleSave`'s check.
    pub fn validate(&mut self) -> bool {
        if self.name.trim().is_empty() || self.email.trim().is_empty() {
            self.error = Some("Both name and email are required.".into());
            return false;
        }
        true
    }
}

// --- Busy -----------------------------------------------------------------------

/// `SyncingModal` / `ImportingModal`: a spinner and a message, no close.
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
        Some("The file is deleted from the project's _attachments folder."),
        "Remove",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_project_validation() {
        let mut f = NewProjectForm {
            location: " ".into(),
            name: "n".into(),
            email: "e".into(),
            error: None,
        };
        assert_eq!(f.validate(), Some("Project location is required."));
        f.location = "/tmp/x".into();
        f.email.clear();
        assert_eq!(f.validate(), Some("User name and email are required."));
        f.email = "e@h".into();
        assert_eq!(f.validate(), None);
    }

    #[test]
    fn duplicate_defaults() {
        let e = Entry {
            id: "a".into(),
            name: "Alpha".into(),
            tags: vec!["x".into()],
            parent_id: Some("p".into()),
            ..Default::default()
        };
        let f = NewFileForm::duplicate(&e);
        assert_eq!(f.name, "Alpha-Copy");
        assert!(f.tags.contains("x"));
        assert_eq!(f.parent.as_deref(), Some("p"));
    }
}
