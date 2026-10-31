//! The dialogs, one per Svelte modal. At most one [`Overlay`] is open at a
//! time (`anyModalOpen`); each returns what the user asked for and the app
//! calls the backend.
//!
//! | Overlay           | Svelte                                                   |
//! |-------------------|----------------------------------------------------------|
//! | `Overlay::AddFeed`, `EditFeed` | `AddFeedModal`, `EditFeedModal`             |
//! | `Overlay::RemoveFeed` | `confirm("Remove …?")` in `Toolbar` / `App`          |
//! | `Overlay::Tags`   | `TagsModal` with `AddTagModal` and `EditTagModal`        |
//! | `Overlay::Settings` | `SettingsModal` ([`settings`])                         |
//! | `Overlay::Sync`   | `SyncModal` ([`sync`])                                   |
//! | `Overlay::Help`   | the Help window (`HelpApp.svelte`), in-app ([`help`])    |
//! | `Overlay::About`  | desktop-ui's `AboutModal`                                |

pub mod help;
pub mod settings;
pub mod sync;

use marina_egui::dialog::{self, ButtonKind, Confirm, Dialog};
use marina_egui::{icons, theme};

use crate::model::Tag;

pub enum Overlay {
    AddFeed(FeedForm),
    EditFeed(FeedForm),
    /// Confirming removal of the feed with this id and title.
    RemoveFeed {
        id: String,
        title: String,
    },
    Tags(TagsState),
    Settings(settings::SettingsState),
    Sync(sync::SyncState),
    Help,
    About,
}

/// The Add Feed / Edit Feed form.
#[derive(Default)]
pub struct FeedForm {
    /// The feed being edited (`None` when adding).
    pub id: Option<String>,
    pub title: String,
    pub url: String,
    pub busy: bool,
    pub error: Option<String>,
}

/// What a small form asked for.
#[derive(Clone, Debug, PartialEq)]
pub enum FormAction {
    Close,
    Submit,
}

/// `AddFeedModal` (no `id`) or `EditFeedModal`.
pub fn feed_form(ctx: &egui::Context, form: &mut FeedForm) -> Option<FormAction> {
    let editing = form.id.is_some();
    let title = if editing { "Edit Feed" } else { "Add Feed" };
    let resp = Dialog::compact(("feed-form", editing), title).show(ctx, |ui| {
        let mut submit = false;
        if editing {
            dialog::field_label(ui, "Title");
            let r = dialog::text_input(ui, &mut form.title, "", !form.busy);
            focus_once(ui, &r);
            submit |= dialog::submitted(ui, &r);
            ui.add_space(10.0);
        }
        dialog::field_label(ui, "Feed URL");
        let r = dialog::text_input(
            ui,
            &mut form.url,
            "https://example.com/feed.xml",
            !form.busy,
        );
        if !editing {
            focus_once(ui, &r);
        }
        submit |= dialog::submitted(ui, &r);
        if let Some(e) = &form.error {
            ui.add_space(6.0);
            dialog::error_text(ui, e);
        }
        let ready = !form.busy
            && !form.url.trim().is_empty()
            && (!editing || !form.title.trim().is_empty());
        let mut out = None;
        dialog::actions(ui, |ui| {
            let label = match (editing, form.busy) {
                (false, false) => "Add Feed",
                (false, true) => "Adding...",
                (true, false) => "Save",
                (true, true) => "Saving...",
            };
            if dialog::button(ui, label, ButtonKind::Primary, ready).clicked() {
                out = Some(FormAction::Submit);
            }
            if dialog::button(ui, "Cancel", ButtonKind::Secondary, !form.busy).clicked() {
                out = Some(FormAction::Close);
            }
            if form.busy {
                ui.spinner();
            }
        });
        if submit && ready {
            out = Some(FormAction::Submit);
        }
        out
    });
    if resp.close {
        return Some(FormAction::Close);
    }
    resp.inner
}

/// Focuses a dialog's first field when it opens (`autofocus`).
fn focus_once(ui: &egui::Ui, resp: &egui::Response) {
    let id = resp.id.with("autofocused");
    if !ui.data(|d| d.get_temp::<bool>(id).unwrap_or(false)) {
        resp.request_focus();
        ui.data_mut(|d| d.insert_temp(id, true));
    }
}

/// The confirmation `Toolbar.handleRemove` asks before removing a feed.
pub fn remove_feed(ctx: &egui::Context, title: &str) -> Option<Confirm> {
    dialog::confirm(
        ctx,
        "remove-feed",
        "Remove Feed",
        &format!("Remove \"{title}\"? This will delete all its entries."),
        None,
        "Remove",
    )
}

/// `TagsModal`'s state, with its nested Add / Edit / Delete dialogs.
#[derive(Default)]
pub struct TagsState {
    pub selected: Option<String>,
    pub sub: Option<TagSub>,
}

pub enum TagSub {
    Add(NameForm),
    Edit(NameForm),
    ConfirmDelete,
}

/// `AddTagModal` / `EditTagModal`'s field.
#[derive(Default)]
pub struct NameForm {
    pub name: String,
    pub busy: bool,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TagsAction {
    Close,
    Add(String),
    Edit { id: String, name: String },
    Delete(String),
}

/// `TagsModal`: an action column (add, edit, delete) beside the tag list.
pub fn tags(ctx: &egui::Context, state: &mut TagsState, tags: &[Tag]) -> Option<TagsAction> {
    let p = theme::palette(ctx);
    if state
        .selected
        .as_ref()
        .is_some_and(|id| !tags.iter().any(|t| &t.id == id))
    {
        state.selected = None;
    }
    let selected = state
        .selected
        .as_ref()
        .and_then(|id| tags.iter().find(|t| &t.id == id))
        .cloned();
    let mut out = None;
    let resp = Dialog::compact("tags", "Tags").show(ctx, |ui| {
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                let action = |ui: &mut egui::Ui, glyph: &str, label: &str, enabled: bool| {
                    marina_egui::toolbar::button(
                        ui,
                        marina_egui::toolbar::ToolbarButton::new(glyph, label).disabled(!enabled),
                    )
                    .clicked()
                };
                if action(ui, icons::PLUS, "Add Tag", true) {
                    state.sub = Some(TagSub::Add(NameForm::default()));
                }
                if action(ui, icons::PEN, "Edit Tag", selected.is_some()) {
                    let name = selected
                        .as_ref()
                        .map(|t| t.name.clone())
                        .unwrap_or_default();
                    state.sub = Some(TagSub::Edit(NameForm {
                        name,
                        ..Default::default()
                    }));
                }
                if action(ui, icons::TRASH, "Delete Tag", selected.is_some()) {
                    state.sub = Some(TagSub::ConfirmDelete);
                }
            });
            ui.add_space(12.0);
            egui::Frame::new()
                .fill(p.bg_base)
                .stroke(egui::Stroke::new(1.0, p.border))
                .corner_radius(theme::RADIUS)
                .show(ui, |ui| {
                    ui.set_min_size(egui::vec2(ui.available_width(), 220.0));
                    ui.set_max_width(ui.available_width());
                    ui.vertical(|ui| {
                        egui::ScrollArea::vertical()
                            .max_height(260.0)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.spacing_mut().item_spacing.y = 0.0;
                                if tags.is_empty() {
                                    ui.add_space(16.0);
                                    ui.vertical_centered(|ui| {
                                        dialog::muted_text(ui, "No tags created yet.")
                                    });
                                }
                                for tag in tags {
                                    let active = state.selected.as_deref() == Some(tag.id.as_str());
                                    if tag_row(ui, &p, &tag.name, active).clicked() {
                                        state.selected = Some(tag.id.clone());
                                    }
                                }
                            })
                    });
                });
        });
    });
    // The nested dialogs sit on top of the Tags dialog.
    match &mut state.sub {
        None => {}
        Some(TagSub::Add(form)) => {
            match name_form(ctx, "Add Tag", "e.g. Technology", form, false) {
                Some(FormAction::Close) => state.sub = None,
                Some(FormAction::Submit) => {
                    out = Some(TagsAction::Add(form.name.trim().to_string()))
                }
                None => {}
            }
        }
        Some(TagSub::Edit(form)) => match name_form(ctx, "Edit Tag", "Tag name", form, true) {
            Some(FormAction::Close) => state.sub = None,
            Some(FormAction::Submit) => {
                if let Some(t) = &selected {
                    out = Some(TagsAction::Edit {
                        id: t.id.clone(),
                        name: form.name.trim().to_string(),
                    });
                }
            }
            None => {}
        },
        Some(TagSub::ConfirmDelete) => {
            let name = selected.as_ref().map_or("", |t| t.name.as_str());
            match dialog::confirm(
                ctx,
                "delete-tag",
                "Delete Tag",
                &format!("Delete tag \"{name}\"? It will be removed from all feeds."),
                None,
                "Delete",
            ) {
                Some(Confirm::Yes) => {
                    state.sub = None;
                    if let Some(t) = &selected {
                        out = Some(TagsAction::Delete(t.id.clone()));
                    }
                    state.selected = None;
                }
                Some(Confirm::No) => state.sub = None,
                None => {}
            }
        }
    }
    if resp.close && state.sub.is_none() {
        return Some(TagsAction::Close);
    }
    out
}

fn tag_row(ui: &mut egui::Ui, p: &theme::Palette, name: &str, active: bool) -> egui::Response {
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 30.0), egui::Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::ListItem, true, active, name));
    if active {
        ui.painter().rect(
            rect,
            0.0,
            p.bg_selected,
            egui::Stroke::new(1.0, p.accent),
            egui::StrokeKind::Inside,
        );
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 0.0, p.bg_item_hover);
    }
    ui.painter().text(
        egui::pos2(rect.left() + 12.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        name,
        egui::FontId::proportional(13.0),
        p.text_primary,
    );
    resp
}

/// `AddTagModal` / `EditTagModal`.
fn name_form(
    ctx: &egui::Context,
    title: &str,
    hint: &str,
    form: &mut NameForm,
    editing: bool,
) -> Option<FormAction> {
    let resp = Dialog::compact(("tag-form", editing), title).show(ctx, |ui| {
        dialog::field_label(ui, "Tag Name");
        let r = dialog::text_input(ui, &mut form.name, hint, !form.busy);
        focus_once(ui, &r);
        let enter = dialog::submitted(ui, &r);
        if let Some(e) = &form.error {
            ui.add_space(6.0);
            dialog::error_text(ui, e);
        }
        let ready = !form.busy && !form.name.trim().is_empty();
        let mut out = None;
        dialog::actions(ui, |ui| {
            let label = match (editing, form.busy) {
                (false, false) => "Add",
                (false, true) => "Adding...",
                (true, false) => "Save",
                (true, true) => "Saving...",
            };
            if dialog::button(ui, label, ButtonKind::Primary, ready).clicked() {
                out = Some(FormAction::Submit);
            }
            if dialog::button(ui, "Cancel", ButtonKind::Secondary, !form.busy).clicked() {
                out = Some(FormAction::Close);
            }
        });
        if enter && ready {
            out = Some(FormAction::Submit);
        }
        out
    });
    if resp.close {
        return Some(FormAction::Close);
    }
    resp.inner
}
