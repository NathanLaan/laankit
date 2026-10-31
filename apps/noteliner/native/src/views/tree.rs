//! The Files pane's tree (`FileTree.svelte`): the notes nested by
//! `parentId`, with selection, inline rename (double-click), drag-and-drop
//! to reorder or re-parent, and the right-click menu.
//!
//! Drag-and-drop works as in the Svelte tree: the top quarter of a row drops
//! before it, the bottom quarter after it, the middle makes the dragged note
//! its child; in a sort mode other than User Order only "child" applies.
//! Dropping a note into its own subtree is refused (the Svelte tree allowed
//! it, which detached the subtree from the tree). Branches can be collapsed
//! with their chevron, a native addition kept for the session.

use std::collections::HashSet;

use egui::{pos2, vec2, Align2, CornerRadius, FontId, Key, Rect, Sense, Stroke, TextEdit, Ui};
use marina_egui::icons;
use marina_egui::theme::{self, Palette};

use crate::backend::ExportKind;
use crate::model::{self, DropPos, Entry, SortMode};

/// `.file-item`: 4 px padding around a 13 px line.
pub const ROW_HEIGHT: f32 = 26.0;
/// `padding-left: 12px + depth × 16px`, plus room for the branch chevron.
const INDENT: f32 = 16.0;
const LEFT_PAD: f32 = 20.0;

/// The right-click menu's actions (`handleContextAction`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextAction {
    OpenInFileSystem,
    Preview,
    Export(ExportKind),
    ConvertToPresentation,
    ConvertToNote,
    PresentationSettings,
    Duplicate,
    Move,
    ClearTags,
    Delete,
}

/// What the user did in the tree.
#[derive(Clone, Debug, PartialEq)]
pub enum TreeAction {
    Select(String),
    /// Inline rename committed with a new name.
    Rename {
        id: String,
        name: String,
    },
    Drop {
        dragged: String,
        target: String,
        pos: DropPos,
    },
    Context(ContextAction, String),
    /// Arrow keys: the previous or next note in display order.
    Step(i32),
}

/// Tree state that outlives a frame.
#[derive(Default)]
pub struct TreeState {
    /// The row being renamed, and the name typed so far.
    editing: Option<(String, String)>,
    focus_edit: bool,
    /// The note being dragged.
    dragging: Option<String>,
    collapsed: HashSet<String>,
    /// The tree was clicked last, so arrow keys move the selection.
    active: bool,
}

impl TreeState {
    /// The note being dragged, if any (the TAGS pane takes drops too).
    pub fn dragging(&self) -> Option<&str> {
        self.dragging.as_deref()
    }

    /// `startRename`.
    pub fn start_rename(&mut self, id: &str, name: &str) {
        self.editing = Some((id.to_string(), name.to_string()));
        self.focus_edit = true;
    }
}

/// Draws the tree; `selected` is the open note.
pub fn show(
    ui: &mut Ui,
    state: &mut TreeState,
    entries: &[Entry],
    sort: SortMode,
    hidden: &HashSet<String>,
    selected: Option<&str>,
) -> Vec<TreeAction> {
    let p = theme::palette(ui.ctx());
    let mut out = Vec::new();
    let rows = visible_rows(entries, sort, hidden, &state.collapsed);
    let has_kids: HashSet<&str> = entries
        .iter()
        .filter_map(|e| e.parent_id.as_deref())
        .collect();
    let pointer = ui.ctx().pointer_interact_pos();
    let released = ui.input(|i| i.pointer.any_released());
    let mut drop_target: Option<(String, DropPos, Rect)> = None;

    egui::ScrollArea::vertical()
        .id_salt("noteliner-file-tree")
        .auto_shrink(false)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            if rows.is_empty() {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.add_space(LEFT_PAD);
                    ui.label(
                        egui::RichText::new(if entries.is_empty() {
                            "No files yet"
                        } else {
                            "No files match the tag filter"
                        })
                        .size(13.0)
                        .color(p.text_muted),
                    );
                });
            }
            for (entry, depth) in &rows {
                let (rect, resp) = ui.allocate_exact_size(
                    vec2(ui.available_width(), ROW_HEIGHT),
                    Sense::click_and_drag(),
                );
                let is_selected = selected == Some(entry.id.as_str());
                resp.widget_info(|| {
                    egui::WidgetInfo::selected(egui::Role::TreeItem, true, is_selected, &entry.name)
                });
                let editing = state
                    .editing
                    .as_ref()
                    .is_some_and(|(id, _)| *id == entry.id);
                let indent = LEFT_PAD + *depth as f32 * INDENT;

                // Chevron for branches.
                let chevron = Rect::from_min_size(
                    pos2(rect.left() + indent - 14.0, rect.top()),
                    vec2(14.0, ROW_HEIGHT),
                );
                let branch = has_kids.contains(entry.id.as_str());
                let mut toggled = false;
                if branch {
                    let c = ui.interact(
                        chevron,
                        ui.scope_id().with(("chevron", &entry.id)),
                        Sense::click(),
                    );
                    if c.clicked() {
                        toggled = true;
                        if !state.collapsed.remove(&entry.id) {
                            state.collapsed.insert(entry.id.clone());
                        }
                    }
                }

                paint_row(
                    ui,
                    &p,
                    rect,
                    entry,
                    indent,
                    is_selected,
                    resp.hovered(),
                    branch.then(|| !state.collapsed.contains(&entry.id)),
                );

                if editing {
                    rename_field(ui, state, rect, indent, &mut out);
                } else if !toggled {
                    if resp.double_clicked() {
                        state.start_rename(&entry.id, &entry.name);
                    } else if resp.clicked() {
                        state.active = true;
                        out.push(TreeAction::Select(entry.id.clone()));
                    }
                    if resp.drag_started() {
                        state.dragging = Some(entry.id.clone());
                    }
                }
                resp.context_menu(|ui| {
                    if !is_selected {
                        out.push(TreeAction::Select(entry.id.clone()));
                    }
                    context_menu(ui, entry, &mut out, state);
                });

                // Where a drag would land on this row.
                if let (Some(dragged), Some(pos)) = (&state.dragging, pointer) {
                    if rect.contains(pos) && *dragged != entry.id {
                        let side = drop_pos(rect, pos.y, sort);
                        drop_target = Some((entry.id.clone(), side, rect));
                    }
                }
            }
        });

    // The drop indicator, then the drop itself on release.
    if let Some(dragged) = state.dragging.clone() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        if let Some((target, side, rect)) = &drop_target {
            let valid = !model::subtree(entries, &dragged).contains(target);
            let color = if valid { p.accent } else { p.text_faint };
            let painter = ui.painter();
            match side {
                DropPos::Before => {
                    painter.hline(rect.x_range(), rect.top() + 1.0, Stroke::new(2.0, color));
                }
                DropPos::After => {
                    painter.hline(rect.x_range(), rect.bottom() - 1.0, Stroke::new(2.0, color));
                }
                DropPos::Child => {
                    painter.rect_stroke(
                        rect.shrink(1.0),
                        CornerRadius::same(3),
                        Stroke::new(1.0, color),
                        egui::StrokeKind::Inside,
                    );
                }
            }
        }
        if released {
            if let Some((target, side, _)) = drop_target {
                out.push(TreeAction::Drop {
                    dragged,
                    target,
                    pos: side,
                });
            }
            state.dragging = None;
        }
    }

    // Arrow keys move the selection (`handleKeydown`) once the tree was
    // clicked and nothing else has the keyboard.
    if state.active && state.editing.is_none() && !ui.ctx().egui_wants_keyboard_input() {
        let (up, down, enter) = ui.input(|i| {
            (
                i.key_pressed(Key::ArrowUp),
                i.key_pressed(Key::ArrowDown),
                i.key_pressed(Key::F2),
            )
        });
        if up {
            out.push(TreeAction::Step(-1));
        }
        if down {
            out.push(TreeAction::Step(1));
        }
        if let (true, Some(id)) = (enter, selected) {
            if let Some(e) = entries.iter().find(|e| e.id == id) {
                state.start_rename(&e.id, &e.name);
            }
        }
    }
    if ui.input(|i| i.pointer.any_pressed()) && !ui.ui_contains_pointer() {
        state.active = false;
    }
    out
}

/// The rows to draw: display order minus collapsed branches' children.
fn visible_rows<'a>(
    entries: &'a [Entry],
    sort: SortMode,
    hidden: &HashSet<String>,
    collapsed: &HashSet<String>,
) -> Vec<(&'a Entry, usize)> {
    let mut out: Vec<(&Entry, usize)> = Vec::new();
    let mut skip_below: Option<usize> = None;
    for (e, depth) in model::flat_list(entries, sort, hidden) {
        if let Some(d) = skip_below {
            if depth > d {
                continue;
            }
            skip_below = None;
        }
        if collapsed.contains(&e.id) {
            skip_below = Some(depth);
        }
        out.push((e, depth));
    }
    out
}

/// `handleDragOver`: before / child / after by the pointer's height in the row.
pub fn drop_pos(rect: Rect, y: f32, sort: SortMode) -> DropPos {
    if sort != SortMode::User {
        return DropPos::Child;
    }
    let at = (y - rect.top()) / rect.height();
    if at < 0.25 {
        DropPos::Before
    } else if at > 0.75 {
        DropPos::After
    } else {
        DropPos::Child
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_row(
    ui: &Ui,
    p: &Palette,
    rect: Rect,
    entry: &Entry,
    indent: f32,
    selected: bool,
    hovered: bool,
    open: Option<bool>,
) {
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, 0.0, p.bg_selected);
        painter.vline(
            rect.left() + 1.0,
            rect.y_range(),
            Stroke::new(2.0, p.accent),
        );
    } else if hovered {
        painter.rect_filled(rect, 0.0, p.bg_item_hover);
    }
    let cy = rect.center().y;
    if let Some(open) = open {
        painter.text(
            pos2(rect.left() + indent - 7.0, cy),
            Align2::CENTER_CENTER,
            if open {
                icons::CHEVRON_DOWN
            } else {
                icons::CHEVRON_RIGHT
            },
            icons::font(9.0),
            p.text_muted,
        );
    }
    let icon = if entry.deck {
        icons::PERSON_CHALKBOARD
    } else {
        icons::FILE_LINES
    };
    painter.text(
        pos2(rect.left() + indent + 6.0, cy),
        Align2::CENTER_CENTER,
        icon,
        icons::font(12.0),
        if selected { p.accent } else { p.text_muted },
    );
    // The tag count and paperclip sit at the right; the name fills the rest.
    let mut right = rect.right() - 10.0;
    if !entry.tags.is_empty() {
        let text = entry.tags.len().to_string();
        let galley = painter.layout_no_wrap(text, FontId::proportional(10.0), p.text_secondary);
        let pill = Rect::from_center_size(
            pos2(right - galley.size().x / 2.0 - 5.0, cy),
            vec2(galley.size().x + 10.0, 16.0),
        );
        painter.rect_filled(pill, CornerRadius::same(8), p.tag_bg);
        painter.galley(
            pill.center() - galley.size() / 2.0,
            galley,
            p.text_secondary,
        );
        right = pill.left() - 6.0;
    }
    if entry.attachments > 0 {
        painter.text(
            pos2(right - 5.0, cy),
            Align2::CENTER_CENTER,
            icons::PAPERCLIP,
            icons::font(10.0),
            p.text_muted,
        );
        right -= 16.0;
    }
    let left = rect.left() + indent + 18.0;
    let color = if selected {
        p.text_primary
    } else {
        p.text_secondary
    };
    let font = FontId::proportional(13.0);
    let name = elide(ui, &entry.name, font.clone(), (right - left).max(0.0));
    painter.text(pos2(left, cy), Align2::LEFT_CENTER, name, font, color);
}

/// The inline rename input (`.rename-input`): Enter or leaving it commits,
/// Escape cancels.
fn rename_field(
    ui: &mut Ui,
    state: &mut TreeState,
    rect: Rect,
    indent: f32,
    out: &mut Vec<TreeAction>,
) {
    let Some((id, name)) = state.editing.as_mut() else {
        return;
    };
    let field = Rect::from_min_max(
        pos2(rect.left() + indent + 14.0, rect.top() + 2.0),
        pos2(rect.right() - 6.0, rect.bottom() - 2.0),
    );
    let edit_id = ui.scope_id().with(("rename", id.clone()));
    let resp = ui.put(
        field,
        TextEdit::singleline(name)
            .id(edit_id)
            .font(FontId::proportional(13.0))
            .margin(vec2(4.0, 2.0)),
    );
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, "Rename"));
    if state.focus_edit {
        resp.request_focus();
        state.focus_edit = false;
        return;
    }
    let escape = ui.input(|i| i.key_pressed(Key::Escape));
    if escape {
        state.editing = None;
        return;
    }
    if resp.lost_focus() {
        let (id, name) = state.editing.take().unwrap_or_default();
        let name = name.trim().to_string();
        if !name.is_empty() {
            out.push(TreeAction::Rename { id, name });
        }
    }
}

fn context_menu(ui: &mut Ui, entry: &Entry, out: &mut Vec<TreeAction>, state: &mut TreeState) {
    let item = |ui: &mut Ui, glyph: &str, label: &str, shortcut: Option<&str>| -> bool {
        let text = match shortcut {
            Some(s) => format!("{glyph}  {label}    {s}"),
            None => format!("{glyph}  {label}"),
        };
        let clicked = ui.button(text).clicked();
        if clicked {
            ui.close();
        }
        clicked
    };
    let id = entry.id.clone();
    let mut act = |a: ContextAction| out.push(TreeAction::Context(a, id.clone()));
    if item(ui, icons::FOLDER_OPEN, "Open in File System", None) {
        act(ContextAction::OpenInFileSystem);
    }
    ui.separator();
    if item(ui, icons::EYE, "Preview", Some("Ctrl+P")) {
        act(ContextAction::Preview);
    }
    if item(ui, icons::FILE_CODE, "Save to HTML", None) {
        act(ContextAction::Export(ExportKind::Html));
    }
    if item(ui, icons::FILE_PDF, "Save to PDF", None) {
        act(ContextAction::Export(ExportKind::Pdf));
    }
    if item(ui, icons::FILE_LINES, "Save to Markdown", None) {
        act(ContextAction::Export(ExportKind::Markdown));
    }
    ui.separator();
    if entry.deck {
        if item(ui, icons::SLIDERS, "Presentation Settings...", None) {
            act(ContextAction::PresentationSettings);
        }
        if item(ui, icons::FILE_LINES, "Convert to Note", None) {
            act(ContextAction::ConvertToNote);
        }
    } else if item(
        ui,
        icons::PERSON_CHALKBOARD,
        "Convert to Presentation...",
        None,
    ) {
        act(ContextAction::ConvertToPresentation);
    }
    ui.separator();
    if item(ui, icons::CLONE, "Duplicate...", Some("Ctrl+Shift+D")) {
        act(ContextAction::Duplicate);
    }
    if item(ui, icons::ARROWS_UP_DOWN_LEFT_RIGHT, "Move...", None) {
        act(ContextAction::Move);
    }
    if item(ui, icons::PEN, "Rename", None) {
        state.start_rename(&entry.id, &entry.name);
    }
    if item(ui, icons::TAGS, "Clear Tags", None) {
        act(ContextAction::ClearTags);
    }
    if item(ui, icons::TRASH, "Delete", Some("Ctrl+D")) {
        act(ContextAction::Delete);
    }
}

/// `text` cut to `width` with an ellipsis.
pub fn elide(ui: &Ui, text: &str, font: FontId, width: f32) -> String {
    let fits = |s: &str| {
        ui.painter()
            .layout_no_wrap(s.to_string(), font.clone(), egui::Color32::WHITE)
            .size()
            .x
            <= width
    };
    if fits(text) {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let (mut lo, mut hi) = (0, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let candidate: String = chars[..mid].iter().collect::<String>() + "…";
        if fits(&candidate) {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    chars[..lo].iter().collect::<String>() + "…"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drop_zones() {
        let r = Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 20.0));
        assert_eq!(drop_pos(r, 2.0, SortMode::User), DropPos::Before);
        assert_eq!(drop_pos(r, 10.0, SortMode::User), DropPos::Child);
        assert_eq!(drop_pos(r, 18.0, SortMode::User), DropPos::After);
        assert_eq!(drop_pos(r, 2.0, SortMode::NameAsc), DropPos::Child);
    }

    #[test]
    fn collapsed_branches_hide_their_children() {
        let index = serde_json::json!({ "files": [
            { "id": "a", "name": "A", "parentId": null, "order": 0 },
            { "id": "b", "name": "B", "parentId": "a", "order": 0 },
            { "id": "c", "name": "C", "parentId": "b", "order": 0 },
            { "id": "d", "name": "D", "parentId": null, "order": 1 }
        ]});
        let list = model::entries(&index);
        let collapsed: HashSet<String> = ["a".to_string()].into();
        let rows: Vec<&str> = visible_rows(&list, SortMode::User, &HashSet::new(), &collapsed)
            .iter()
            .map(|(e, _)| e.id.as_str())
            .collect();
        assert_eq!(rows, ["a", "d"]);
    }
}
