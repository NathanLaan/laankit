//! The Files pane's tree: the folders on disk, with the notes and diagrams
//! in them. A folder is a branch that opens and closes; a note or diagram is
//! a leaf. In every folder its folders come first, then its files in the
//! chosen sort.
//!
//! - Click selects. A note opens in its tab; a folder also opens or closes.
//! - Double-click, or F2, renames in place.
//! - Dragging a note or a folder moves it to another folder: drop it on the
//!   folder, or on the empty part of the tree for the top of the project.
//!   There is no drag to reorder.
//! - Right-click opens a menu for the note, the folder, or (on the empty
//!   part) the project.

use std::collections::HashSet;

use egui::{pos2, vec2, Align2, CornerRadius, FontId, Key, Rect, Sense, Stroke, TextEdit, Ui};
use laankit_egui::icons;
use laankit_egui::theme::{self, Palette};

use crate::backend::ExportKind;
use crate::model::{self, Entry, Node, SortMode};

/// `.file-item`: 4 px padding around a 13 px line.
pub const ROW_HEIGHT: f32 = 26.0;
/// `padding-left: 12px + depth × 16px`, plus room for the branch chevron.
const INDENT: f32 = 16.0;
const LEFT_PAD: f32 = 20.0;

/// A note's right-click menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextAction {
    OpenInFileSystem,
    Preview,
    Export(ExportKind),
    /// The Export dialog for a diagram (PNG or SVG).
    ExportDiagram,
    ConvertToPresentation,
    Present,
    PresentWithSpeakerView,
    ConvertToNote,
    PresentationSettings,
    Duplicate,
    Move,
    ClearTags,
    Delete,
}

/// A folder's right-click menu. The New actions are also on the menu of the
/// empty part of the tree, for the top of the project.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FolderAction {
    NewFile,
    NewDiagram,
    NewFolder,
    Move,
    OpenInFileSystem,
    Delete,
}

/// What is being dragged, or renamed: a note or diagram by id, or a folder
/// by path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    File(String),
    Folder(String),
}

/// What the user did in the tree.
#[derive(Clone, Debug, PartialEq)]
pub enum TreeAction {
    Select(String),
    /// A folder was clicked: it is the selection, and where new files go.
    SelectFolder(String),
    /// Open or close a folder.
    ToggleFolder(String),
    /// Inline rename committed with a new name.
    Rename {
        id: String,
        name: String,
    },
    RenameFolder {
        path: String,
        name: String,
    },
    /// Dropped on a folder (`into` is its path), or on the empty part of
    /// the tree (`into` is empty: the top of the project).
    Drop {
        dragged: Item,
        into: String,
    },
    Context(ContextAction, String),
    /// A folder's menu, with its path; empty for the project folder.
    FolderContext(FolderAction, String),
    /// Arrow keys: the previous or next note in display order.
    Step(i32),
}

/// Tree state that outlives a frame.
#[derive(Default)]
pub struct TreeState {
    /// The row being renamed, and the name typed so far.
    editing: Option<(Item, String)>,
    focus_edit: bool,
    dragging: Option<Item>,
    /// The tree was clicked last, so arrow keys move the selection.
    active: bool,
}

impl TreeState {
    /// The note being dragged, if one is (the TAGS pane and the editor take
    /// drops too).
    pub fn dragging(&self) -> Option<&str> {
        match &self.dragging {
            Some(Item::File(id)) => Some(id),
            _ => None,
        }
    }

    /// Starts renaming a note in place.
    pub fn start_rename(&mut self, id: &str, name: &str) {
        self.editing = Some((Item::File(id.to_string()), name.to_string()));
        self.focus_edit = true;
    }

    /// Starts renaming a folder in place.
    pub fn start_rename_folder(&mut self, path: &str) {
        self.editing = Some((
            Item::Folder(path.to_string()),
            model::folder_name(path).to_string(),
        ));
        self.focus_edit = true;
    }
}

/// What the tree draws from.
pub struct TreeProps<'a> {
    pub entries: &'a [Entry],
    /// Every folder, by path.
    pub folders: &'a [String],
    /// The folders that are open.
    pub open: &'a HashSet<String>,
    pub sort: SortMode,
    pub hidden: &'a HashSet<String>,
    /// The open note.
    pub selected: Option<&'a str>,
    /// The selected folder, which takes the highlight from the open note.
    pub selected_folder: Option<&'a str>,
}

/// Whether `dragged` may be dropped into the folder `into`.
fn can_drop(props: &TreeProps<'_>, dragged: &Item, into: &str) -> bool {
    match dragged {
        Item::File(id) => props
            .entries
            .iter()
            .any(|e| e.id == *id && e.folder != into),
        Item::Folder(path) => model::folder_can_move(path, into),
    }
}

/// Draws the tree.
pub fn show(ui: &mut Ui, state: &mut TreeState, props: &TreeProps<'_>) -> Vec<TreeAction> {
    let p = theme::palette(ui.ctx());
    let mut out = Vec::new();
    let rows = model::rows(
        props.entries,
        props.folders,
        props.sort,
        props.hidden,
        Some(props.open),
    );
    let pointer = ui.ctx().pointer_interact_pos();
    let released = ui.input(|i| i.pointer.any_released());
    // The folder a drag would land in, and its row; `None` for a row that
    // takes no drop.
    let mut drop_target: Option<(String, Rect)> = None;
    let mut over_row = false;

    let area = egui::ScrollArea::vertical()
        .id_salt("noteliner-file-tree")
        .auto_shrink(false)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            if rows.is_empty() {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.add_space(LEFT_PAD);
                    ui.label(
                        egui::RichText::new(if props.entries.is_empty() {
                            "No files yet"
                        } else {
                            "No files match the tag filter"
                        })
                        .size(13.0)
                        .color(p.text_muted),
                    );
                });
            }
            for (node, depth) in &rows {
                let (rect, resp) = ui.allocate_exact_size(
                    vec2(ui.available_width(), ROW_HEIGHT),
                    Sense::click_and_drag(),
                );
                let indent = LEFT_PAD + *depth as f32 * INDENT;
                if pointer.is_some_and(|pos| rect.contains(pos)) {
                    over_row = true;
                }
                match node {
                    Node::Folder(path) => {
                        let path = *path;
                        let name = model::folder_name(path);
                        let is_open = props.open.contains(path);
                        let is_selected = props.selected_folder == Some(path);
                        resp.widget_info(|| {
                            egui::WidgetInfo::selected(
                                egui::Role::TreeItem,
                                true,
                                is_selected,
                                format!("Folder {name}"),
                            )
                        });
                        paint_folder(
                            ui,
                            &p,
                            rect,
                            name,
                            indent,
                            is_selected,
                            resp.hovered(),
                            is_open,
                        );
                        let editing = matches!(
                            &state.editing,
                            Some((Item::Folder(f), _)) if f == path
                        );
                        if editing {
                            rename_field(ui, state, rect, indent, &mut out);
                        } else {
                            if resp.double_clicked() {
                                state.start_rename_folder(path);
                            } else if resp.clicked() {
                                state.active = true;
                                out.push(TreeAction::SelectFolder(path.to_string()));
                                out.push(TreeAction::ToggleFolder(path.to_string()));
                            }
                            if resp.drag_started() {
                                state.dragging = Some(Item::Folder(path.to_string()));
                            }
                        }
                        resp.context_menu(|ui| {
                            if !is_selected {
                                out.push(TreeAction::SelectFolder(path.to_string()));
                            }
                            folder_menu(ui, Some(path), &mut out, state);
                        });
                        if let (Some(dragged), Some(pos)) = (&state.dragging, pointer) {
                            if rect.contains(pos) && can_drop(props, dragged, path) {
                                drop_target = Some((path.to_string(), rect));
                            }
                        }
                    }
                    Node::File(entry) => {
                        let is_selected = props.selected_folder.is_none()
                            && props.selected == Some(entry.id.as_str());
                        resp.widget_info(|| {
                            egui::WidgetInfo::selected(
                                egui::Role::TreeItem,
                                true,
                                is_selected,
                                &entry.name,
                            )
                        });
                        let editing = matches!(
                            &state.editing,
                            Some((Item::File(id), _)) if *id == entry.id
                        );
                        paint_row(
                            ui,
                            &p,
                            rect,
                            entry,
                            indent,
                            is_selected,
                            resp.hovered(),
                            None,
                        );
                        if editing {
                            rename_field(ui, state, rect, indent, &mut out);
                        } else {
                            if resp.double_clicked() {
                                state.start_rename(&entry.id, &entry.name);
                            } else if resp.clicked() {
                                state.active = true;
                                out.push(TreeAction::Select(entry.id.clone()));
                            }
                            if resp.drag_started() {
                                state.dragging = Some(Item::File(entry.id.clone()));
                            }
                        }
                        resp.context_menu(|ui| {
                            if !is_selected {
                                out.push(TreeAction::Select(entry.id.clone()));
                            }
                            context_menu(ui, entry, &mut out, state);
                        });
                    }
                }
            }
            // The empty part of the tree: the top of the project.
            let rest = vec2(
                ui.available_width(),
                (ui.clip_rect().bottom() - ui.cursor().top()).max(ROW_HEIGHT),
            );
            let (_, bg) = ui.allocate_exact_size(rest, Sense::click());
            bg.widget_info(|| {
                egui::WidgetInfo::labeled(egui::Role::Pane, true, "Empty part of the file tree")
            });
            bg.context_menu(|ui| folder_menu(ui, None, &mut out, state));
        });

    // The drop indicator, then the drop itself on release.
    if let Some(dragged) = state.dragging.clone() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        let tree_rect = area.inner_rect;
        // Off every row, inside the tree: the top of the project.
        if drop_target.is_none()
            && !over_row
            && pointer.is_some_and(|pos| tree_rect.contains(pos))
            && can_drop(props, &dragged, "")
        {
            drop_target = Some((String::new(), tree_rect));
        }
        if let Some((_, rect)) = &drop_target {
            ui.painter().rect_stroke(
                rect.shrink(1.0),
                CornerRadius::same(3),
                Stroke::new(1.0, p.accent),
                egui::StrokeKind::Inside,
            );
        }
        if released {
            if let Some((into, _)) = drop_target {
                out.push(TreeAction::Drop { dragged, into });
            }
            state.dragging = None;
        }
    }

    // Arrow keys move the selection once the tree was clicked and nothing
    // else has the keyboard; F2 renames what is selected.
    if state.active && state.editing.is_none() && !ui.ctx().egui_wants_keyboard_input() {
        let (up, down, rename) = ui.input(|i| {
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
        if rename {
            if let Some(path) = props.selected_folder {
                state.start_rename_folder(path);
            } else if let Some(e) = props
                .selected
                .and_then(|id| props.entries.iter().find(|e| e.id == id))
            {
                state.start_rename(&e.id, &e.name);
            }
        }
    }
    if ui.input(|i| i.pointer.any_pressed()) && !ui.ui_contains_pointer() {
        state.active = false;
    }
    out
}

/// A folder's row: its chevron, a folder icon, and its name.
#[allow(clippy::too_many_arguments)]
fn paint_folder(
    ui: &Ui,
    p: &Palette,
    rect: Rect,
    name: &str,
    indent: f32,
    selected: bool,
    hovered: bool,
    open: bool,
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
    painter.text(
        pos2(rect.left() + indent + 6.0, cy),
        Align2::CENTER_CENTER,
        if open {
            icons::FOLDER_OPEN
        } else {
            icons::FOLDER
        },
        icons::font(12.0),
        if selected { p.accent } else { p.text_muted },
    );
    let left = rect.left() + indent + 18.0;
    let font = FontId::proportional(13.0);
    let text = elide(
        ui,
        name,
        font.clone(),
        (rect.right() - 10.0 - left).max(0.0),
    );
    painter.text(
        pos2(left, cy),
        Align2::LEFT_CENTER,
        text,
        font,
        if selected {
            p.text_primary
        } else {
            p.text_secondary
        },
    );
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
    let icon = file_icon(entry.is_diagram(), entry.deck);
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
    let Some((item, name)) = state.editing.as_mut() else {
        return;
    };
    let field = Rect::from_min_max(
        pos2(rect.left() + indent + 14.0, rect.top() + 2.0),
        pos2(rect.right() - 6.0, rect.bottom() - 2.0),
    );
    let edit_id = ui.scope_id().with(("rename", format!("{item:?}")));
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
        let Some((item, name)) = state.editing.take() else {
            return;
        };
        let name = name.trim().to_string();
        if name.is_empty() {
            return;
        }
        match item {
            Item::File(id) => out.push(TreeAction::Rename { id, name }),
            // A folder's name is its name on disk: unchanged is no rename.
            Item::Folder(path) if model::folder_name(&path) != name => {
                out.push(TreeAction::RenameFolder { path, name })
            }
            Item::Folder(_) => {}
        }
    }
}

/// A menu row: an icon, a label, and a shortcut.
fn menu_item(ui: &mut Ui, glyph: &str, label: &str, shortcut: Option<&str>) -> bool {
    let text = match shortcut {
        Some(s) => format!("{glyph}  {label}    {s}"),
        None => format!("{glyph}  {label}"),
    };
    let clicked = ui.button(text).clicked();
    if clicked {
        ui.close();
    }
    clicked
}

/// A folder's menu, or with `None` the menu of the empty part of the tree,
/// whose New actions make things at the top of the project.
fn folder_menu(ui: &mut Ui, path: Option<&str>, out: &mut Vec<TreeAction>, state: &mut TreeState) {
    let target = path.unwrap_or_default().to_string();
    let mut act = |a: FolderAction| out.push(TreeAction::FolderContext(a, target.clone()));
    if menu_item(ui, icons::FILE_CIRCLE_PLUS, "New File...", Some("Ctrl+N")) {
        act(FolderAction::NewFile);
    }
    if menu_item(ui, icons::OBJECT_GROUP, "New Diagram...", None) {
        act(FolderAction::NewDiagram);
    }
    if menu_item(ui, icons::FOLDER_PLUS, "New Folder...", None) {
        act(FolderAction::NewFolder);
    }
    let Some(path) = path else {
        ui.separator();
        if menu_item(ui, icons::FOLDER_OPEN, "Open in File System", None) {
            act(FolderAction::OpenInFileSystem);
        }
        return;
    };
    ui.separator();
    if menu_item(ui, icons::PEN, "Rename", Some("F2")) {
        state.start_rename_folder(path);
    }
    if menu_item(ui, icons::ARROWS_UP_DOWN_LEFT_RIGHT, "Move...", None) {
        act(FolderAction::Move);
    }
    if menu_item(ui, icons::FOLDER_OPEN, "Open in File System", None) {
        act(FolderAction::OpenInFileSystem);
    }
    ui.separator();
    if menu_item(ui, icons::TRASH, "Delete", Some("Ctrl+D")) {
        act(FolderAction::Delete);
    }
}

fn context_menu(ui: &mut Ui, entry: &Entry, out: &mut Vec<TreeAction>, state: &mut TreeState) {
    let item = menu_item;
    let id = entry.id.clone();
    let mut act = |a: ContextAction| out.push(TreeAction::Context(a, id.clone()));
    if item(ui, icons::FOLDER_OPEN, "Open in File System", None) {
        act(ContextAction::OpenInFileSystem);
    }
    ui.separator();
    // A diagram has no preview, note exports or presentation.
    if entry.is_diagram() {
        if item(ui, icons::FILE_EXPORT, "Export...", None) {
            act(ContextAction::ExportDiagram);
        }
        if item(ui, icons::FILE_CODE, "Save to Mermaid", None) {
            act(ContextAction::Export(ExportKind::Mermaid));
        }
    } else {
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
    }
    ui.separator();
    if entry.is_diagram() {
        // Nothing to convert.
    } else if entry.deck {
        if item(ui, icons::CIRCLE_PLAY, "Present", Some("F5")) {
            act(ContextAction::Present);
        }
        if item(
            ui,
            icons::DISPLAY,
            "Present with Speaker View",
            Some("Alt+F5"),
        ) {
            act(ContextAction::PresentWithSpeakerView);
        }
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

/// A file's icon, in the tree and on its tab: a diagram, a presentation or
/// a note.
pub fn file_icon(diagram: bool, deck: bool) -> &'static str {
    if diagram {
        icons::OBJECT_GROUP
    } else if deck {
        icons::PERSON_CHALKBOARD
    } else {
        icons::FILE_LINES
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn props<'a>(
        entries: &'a [Entry],
        folders: &'a [String],
        open: &'a HashSet<String>,
        hidden: &'a HashSet<String>,
    ) -> TreeProps<'a> {
        TreeProps {
            entries,
            folders,
            open,
            sort: SortMode::NameAsc,
            hidden,
            selected: None,
            selected_folder: None,
        }
    }

    #[test]
    fn drops_only_move_into_another_folder() {
        let index = serde_json::json!({ "files": [
            { "id": "a", "name": "A", "filename": "a.md" },
            { "id": "b", "name": "B", "filename": "Work/b.md" }
        ]});
        let entries = model::entries(&index);
        let folders: Vec<String> = ["Work", "Work/Sub", "Other"].map(String::from).to_vec();
        let (open, hidden) = (HashSet::new(), HashSet::new());
        let p = props(&entries, &folders, &open, &hidden);
        let file = |id: &str| Item::File(id.to_string());
        let folder = |path: &str| Item::Folder(path.to_string());
        // A note moves to any folder but the one it is in.
        assert!(can_drop(&p, &file("a"), "Work"));
        assert!(!can_drop(&p, &file("a"), ""));
        assert!(can_drop(&p, &file("b"), ""));
        assert!(!can_drop(&p, &file("b"), "Work"));
        assert!(!can_drop(&p, &file("nope"), "Work"));
        // A folder can't move into itself, or under itself.
        assert!(!can_drop(&p, &folder("Work"), "Work"));
        assert!(!can_drop(&p, &folder("Work"), "Work/Sub"));
        assert!(can_drop(&p, &folder("Work"), "Other"));
        assert!(!can_drop(&p, &folder("Work"), ""));
        assert!(can_drop(&p, &folder("Work/Sub"), ""));
    }

    #[test]
    fn only_a_dragged_note_is_offered_to_other_panes() {
        let mut s = TreeState {
            dragging: Some(Item::Folder("Work".into())),
            ..Default::default()
        };
        assert_eq!(s.dragging(), None);
        s.dragging = Some(Item::File("a".into()));
        assert_eq!(s.dragging(), Some("a"));
        s.start_rename_folder("Work/Meetings");
        assert_eq!(
            s.editing,
            Some((Item::Folder("Work/Meetings".into()), "Meetings".into()))
        );
    }
}
