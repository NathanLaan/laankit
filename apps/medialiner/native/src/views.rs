//! The main window's parts: the toolbar, the Folders and Files panes, the
//! grid, the preview, the Info pane, and the status bar.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Local};
use egui::{
    pos2, vec2, Align, Align2, Color32, CornerRadius, FontId, Frame, Layout, Margin, Rect,
    RichText, ScrollArea, Sense, Stroke, StrokeKind, TextureHandle, Ui, Vec2,
};
use laankit_egui::dialog::{self, ButtonKind};
use laankit_egui::theme::{self, Palette};
use laankit_egui::toolbar::{self, ToolbarButton};
use laankit_egui::tree::{self, elide, TreeRow};
use laankit_egui::{icons, panes};
use medialiner_core::info::{self, human_size, FileInfo};
use medialiner_core::scan::FileEntry;
use medialiner_core::MediaKind;

use crate::commands::Act;
use crate::textures::{Textures, Thumb};

pub const STATUS_BAR_HEIGHT: f32 = 24.0;
pub const PANE_HEADER_HEIGHT: f32 = 36.0;
/// (min, default, max) widths.
pub const FOLDERS_WIDTH: (f32, f32, f32) = (160.0, 240.0, 520.0);
pub const FILES_WIDTH: (f32, f32, f32) = (160.0, 240.0, 520.0);
pub const INFO_WIDTH: (f32, f32, f32) = (220.0, 290.0, 520.0);
const FILE_ROW_HEIGHT: f32 = 24.0;
const GRID_GAP: f32 = 12.0;
const NAME_HEIGHT: f32 = 20.0;
/// The preview's zoom limits.
pub const ZOOM_MIN: f32 = 0.05;
pub const ZOOM_MAX: f32 = 16.0;

/// What the views ask the app for.
#[derive(Clone, Debug, PartialEq)]
pub enum ViewAction {
    Act(Act),
    /// The tree took the arrow keys.
    FocusTree,
    /// The files or grid took the arrow keys.
    FocusContent,
    SelectFolder(PathBuf),
    ToggleFolder(PathBuf),
    MergeFolder(PathBuf),
    /// Dragged onto another folder: merge it there.
    DropFolder {
        source: PathBuf,
        dest: PathBuf,
    },
    OpenFolderInFileManager(PathBuf),
    RefreshFolder(PathBuf),
    RemoveFolder(PathBuf),
    /// Select a photo (or another file, in the Files pane).
    Select(PathBuf),
    OpenPreview(PathBuf),
    Photo(PathBuf, PhotoCommand),
    SetThumbSize(f32),
    ToggleSubfolders,
    CloseInfo,
    ShowLocation(bool),
    Copy(String),
    OpenUrl(String),
}

/// The right-click menu's commands on a photo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhotoCommand {
    OpenInFolder,
    ShowInfo,
    OpenWithDefault,
    CopyPath,
}

fn menu_item(ui: &mut Ui, label: &str) -> bool {
    let clicked = ui.button(label).clicked();
    if clicked {
        ui.close();
    }
    clicked
}

/// The right-click menu on a photo (or another file).
pub fn photo_menu(ui: &mut Ui, path: &Path, photo: bool, out: &mut Vec<ViewAction>) {
    let mut cmd = |c| out.push(ViewAction::Photo(path.to_owned(), c));
    if menu_item(ui, "Open in Folder") {
        cmd(PhotoCommand::OpenInFolder);
    }
    if photo && menu_item(ui, "Show Info") {
        cmd(PhotoCommand::ShowInfo);
    }
    ui.separator();
    if menu_item(ui, "Open With Default App") {
        cmd(PhotoCommand::OpenWithDefault);
    }
    if menu_item(ui, "Copy Path") {
        cmd(PhotoCommand::CopyPath);
    }
}

// --- Toolbar and status bar ------------------------------------------------

pub struct ToolbarProps {
    pub folders_visible: bool,
    pub files_visible: bool,
    pub info_visible: bool,
    pub settings_open: bool,
    pub about_open: bool,
}

pub fn toolbar(ui: &mut Ui, props: &ToolbarProps) -> Vec<Act> {
    let mut out = Vec::new();
    let mut bottom = Vec::new();
    toolbar::show(
        ui,
        |ui| {
            let add =
                ToolbarButton::new(icons::FOLDER_PLUS, "Add Folder (Ctrl+O)").label("Add Folder");
            if toolbar::button(ui, add).clicked() {
                out.push(Act::AddFolder);
            }
            toolbar::divider(ui);
            let folders = ToolbarButton::new(icons::FOLDER_TREE, "Folders Pane (Ctrl+B)")
                .label("Folders")
                .active(props.folders_visible);
            if toolbar::button(ui, folders).clicked() {
                out.push(Act::ToggleFolders);
            }
            let files = ToolbarButton::new(icons::LIST_UL, "Files Pane (Ctrl+Shift+F)")
                .label("Files")
                .active(props.files_visible);
            if toolbar::button(ui, files).clicked() {
                out.push(Act::ToggleFiles);
            }
            let info = ToolbarButton::new(icons::CIRCLE_INFO, "Info Pane (Ctrl+I)")
                .label("Info")
                .active(props.info_visible);
            if toolbar::button(ui, info).clicked() {
                out.push(Act::ShowInfo);
            }
        },
        |ui| {
            let settings = ToolbarButton::new(icons::GEAR, "Settings (Ctrl+,)")
                .label("Settings")
                .active(props.settings_open);
            if toolbar::button(ui, settings).clicked() {
                bottom.push(Act::Settings);
            }
            let about = ToolbarButton::new(icons::CIRCLE_QUESTION, "About")
                .label("About")
                .active(props.about_open);
            if toolbar::button(ui, about).clicked() {
                bottom.push(Act::About);
            }
        },
    );
    out.extend(bottom);
    out
}

/// The status bar's text, left to right.
pub fn status_bar(ui: &mut Ui, parts: &[String], busy: Option<&str>) {
    let p = theme::palette(ui.ctx());
    let rect = ui.max_rect();
    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, p.bg_surface);
    painter.hline(rect.x_range(), rect.top(), Stroke::new(1.0, p.border));
    let font = FontId::proportional(12.0);
    let y = rect.center().y;
    let mut x = rect.left() + 12.0;
    for part in parts {
        let r = painter.text(
            pos2(x, y),
            Align2::LEFT_CENTER,
            part,
            font.clone(),
            p.text_secondary,
        );
        x = r.right() + 16.0;
    }
    if let Some(busy) = busy {
        painter.text(
            pos2(rect.right() - 12.0, y),
            Align2::RIGHT_CENTER,
            busy,
            font,
            p.accent,
        );
    }
}

// --- Pane header ---------------------------------------------------------

/// A pane's header: its title, and buttons at the right.
fn pane_header(ui: &mut Ui, title: &str, buttons: impl FnOnce(&mut Ui)) {
    let p = theme::palette(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(
        vec2(ui.available_width(), PANE_HEADER_HEIGHT),
        Sense::hover(),
    );
    let painter = ui.painter();
    painter.hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        Stroke::new(1.0, p.border),
    );
    painter.text(
        pos2(rect.left() + 14.0, rect.center().y),
        Align2::LEFT_CENTER,
        title.to_uppercase(),
        FontId::proportional(11.0),
        p.text_muted,
    );
    let inner = Rect::from_min_max(
        pos2(rect.left() + 80.0, rect.top() + 6.0),
        pos2(rect.right() - 8.0, rect.bottom() - 6.0),
    );
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::right_to_left(Align::Center)),
        buttons,
    );
}

/// A 24 px icon button for a pane header.
fn header_button(ui: &mut Ui, id: &str, glyph: &str, label: &str) -> bool {
    let p = theme::palette(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::hover());
    panes::header_button(ui, &p, ui.scope_id().with(id), rect, glyph, label).clicked()
}

fn empty_note(ui: &mut Ui, text: &str) {
    ui.add_space(16.0);
    ui.vertical_centered(|ui| {
        ui.set_max_width(ui.available_width() - 24.0);
        dialog::muted_text(ui, text);
    });
}

// --- Folders ---------------------------------------------------------------

/// A visible row of the Folders tree.
pub struct FolderRow {
    pub path: PathBuf,
    pub depth: usize,
    pub name: String,
    pub hint: Option<String>,
    /// `None`: no subfolders. `Some(open)` otherwise.
    pub branch: Option<bool>,
    pub root: bool,
    /// Not there (an unplugged drive).
    pub missing: bool,
}

pub fn folders_pane(
    ui: &mut Ui,
    rows: &[FolderRow],
    selected: Option<&Path>,
    reveal: bool,
) -> Vec<ViewAction> {
    let mut out = Vec::new();
    pane_header(ui, "Folders", |ui| {
        if header_button(ui, "add-folder", icons::PLUS, "Add Folder") {
            out.push(ViewAction::Act(Act::AddFolder));
        }
    });
    if rows.is_empty() {
        empty_note(ui, "Add a folder of photos to start.");
        ui.add_space(8.0);
        ui.vertical_centered(|ui| {
            if dialog::button(ui, "Add Folder…", ButtonKind::Primary, true).clicked() {
                out.push(ViewAction::Act(Act::AddFolder));
            }
        });
        return out;
    }
    let tree_rows: Vec<TreeRow<'_>> = rows
        .iter()
        .map(|r| {
            let is_sel = selected == Some(r.path.as_path());
            let mut row = TreeRow::new(r.depth, &r.name)
                .hint(r.hint.as_deref())
                .icon(if r.branch == Some(true) {
                    icons::FOLDER_OPEN
                } else {
                    icons::FOLDER
                })
                .selected(is_sel)
                .muted(r.missing)
                .drag_and_drop(!r.missing)
                .reveal(reveal && is_sel);
            if let Some(open) = r.branch {
                row = row.branch(open);
            }
            row
        })
        .collect();
    let mut menu = Vec::new();
    ScrollArea::vertical()
        .id_salt("medialiner-folders")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(4.0);
            let result = tree::show(ui, "medialiner-folder-tree", &tree_rows, |ui, i| {
                let r = &rows[i];
                if r.missing {
                    ui.label(RichText::new("Not found").weak());
                } else {
                    if menu_item(ui, "Merge Into…") {
                        menu.push(ViewAction::MergeFolder(r.path.clone()));
                    }
                    if menu_item(ui, "Open in Folder") {
                        menu.push(ViewAction::OpenFolderInFileManager(r.path.clone()));
                    }
                    if menu_item(ui, "Refresh") {
                        menu.push(ViewAction::RefreshFolder(r.path.clone()));
                    }
                }
                if r.root {
                    ui.separator();
                    if menu_item(ui, "Remove from List") {
                        menu.push(ViewAction::RemoveFolder(r.path.clone()));
                    }
                }
            });
            if let Some(i) = result.toggled {
                out.push(ViewAction::ToggleFolder(rows[i].path.clone()));
            }
            if let Some(i) = result.clicked.or(result.context) {
                out.push(ViewAction::FocusTree);
                if selected != Some(rows[i].path.as_path()) {
                    out.push(ViewAction::SelectFolder(rows[i].path.clone()));
                }
            }
            if let Some(i) = result.double_clicked {
                if rows[i].branch.is_some() {
                    out.push(ViewAction::ToggleFolder(rows[i].path.clone()));
                }
            }
            if let Some((from, onto)) = result.dropped {
                out.push(ViewAction::DropFolder {
                    source: rows[from].path.clone(),
                    dest: rows[onto].path.clone(),
                });
            }
            ui.add_space(8.0);
        });
    out.extend(menu);
    out
}

// --- Files -----------------------------------------------------------------

pub struct FilesProps<'a> {
    pub folder: Option<&'a Path>,
    pub files: &'a [FileEntry],
    pub loading: bool,
    pub error: Option<&'a str>,
    pub selected: Option<&'a Path>,
    pub include_subfolders: bool,
    pub reveal: bool,
}

pub fn files_pane(ui: &mut Ui, props: &FilesProps<'_>) -> Vec<ViewAction> {
    let p = theme::palette(ui.ctx());
    let mut out = Vec::new();
    pane_header(ui, "Files", |ui| {
        if props.folder.is_some() {
            let label = if props.include_subfolders {
                "Include Subfolders (On)"
            } else {
                "Include Subfolders (Off)"
            };
            let (rect, _) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::hover());
            if props.include_subfolders {
                ui.painter().rect_filled(rect, theme::RADIUS, p.bg_selected);
            }
            if panes::header_button(
                ui,
                &p,
                ui.scope_id().with("subfolders"),
                rect,
                icons::FOLDER_TREE,
                label,
            )
            .clicked()
            {
                out.push(ViewAction::ToggleSubfolders);
            }
        }
    });
    let Some(folder) = props.folder else {
        empty_note(ui, "Select a folder to see its files.");
        return out;
    };
    if let Some(e) = props.error {
        empty_note(ui, &format!("Couldn't read this folder: {e}"));
        return out;
    }
    if props.files.is_empty() {
        empty_note(
            ui,
            if props.loading {
                "Reading…"
            } else {
                "No files in this folder."
            },
        );
        return out;
    }
    let reveal_row = props
        .reveal
        .then(|| {
            props
                .files
                .iter()
                .position(|f| Some(f.path.as_path()) == props.selected)
        })
        .flatten();
    let mut area = ScrollArea::vertical()
        .id_salt(("medialiner-files", folder))
        .auto_shrink([false, false]);
    if let Some(i) = reveal_row {
        let visible_h = ui.available_height();
        let state = egui::scroll_area::State::load(
            ui.ctx(),
            ui.make_persistent_id(("medialiner-files", folder)),
        );
        let top = i as f32 * FILE_ROW_HEIGHT;
        let offset = state.map_or(0.0, |s| s.clamped_offset().y);
        if top < offset || top + FILE_ROW_HEIGHT > offset + visible_h {
            area = area.vertical_scroll_offset((top - visible_h / 2.0).max(0.0));
        }
    }
    let mut menu = Vec::new();
    area.show_rows(ui, FILE_ROW_HEIGHT, props.files.len(), |ui, range| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for i in range {
            let f = &props.files[i];
            let selected = props.selected == Some(f.path.as_path());
            let (rect, resp) =
                ui.allocate_exact_size(vec2(ui.available_width(), FILE_ROW_HEIGHT), Sense::click());
            resp.widget_info(|| {
                egui::WidgetInfo::selected(egui::Role::ListItem, true, selected, &f.name)
            });
            let hint = if props.include_subfolders {
                f.path
                    .parent()
                    .and_then(|d| d.strip_prefix(folder).ok())
                    .filter(|d| !d.as_os_str().is_empty())
                    .map(|d| d.display().to_string())
            } else {
                None
            };
            paint_file_row(ui, &p, rect, f, hint.as_deref(), selected, resp.hovered());
            if resp.clicked() {
                out.push(ViewAction::FocusContent);
                out.push(ViewAction::Select(f.path.clone()));
            }
            if resp.double_clicked() && f.kind.is_photo() {
                out.push(ViewAction::OpenPreview(f.path.clone()));
            }
            resp.context_menu(|ui| photo_menu(ui, &f.path, f.kind.is_photo(), &mut menu));
        }
    });
    out.extend(menu);
    out
}

fn file_icon(kind: MediaKind) -> &'static str {
    match kind {
        MediaKind::Photo => icons::IMAGE,
        MediaKind::Video => icons::FILE_VIDEO,
        MediaKind::Other => icons::FILE,
    }
}

fn paint_file_row(
    ui: &Ui,
    p: &Palette,
    rect: Rect,
    f: &FileEntry,
    hint: Option<&str>,
    selected: bool,
    hovered: bool,
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
    let photo = f.kind.is_photo();
    let (icon_colour, text_colour) = match (photo, selected) {
        (_, true) => (p.accent, p.text_primary),
        (true, false) => (p.text_muted, p.text_secondary),
        (false, false) => (p.text_faint, p.text_muted),
    };
    painter.text(
        pos2(rect.left() + 18.0, cy),
        Align2::CENTER_CENTER,
        file_icon(f.kind),
        icons::font(11.0),
        icon_colour,
    );
    let left = rect.left() + 32.0;
    let right = rect.right() - 8.0;
    let font = FontId::proportional(13.0);
    let name = elide(ui, &f.name, font.clone(), (right - left).max(0.0));
    let r = painter.text(pos2(left, cy), Align2::LEFT_CENTER, name, font, text_colour);
    if let Some(hint) = hint {
        let at = r.right() + 6.0;
        if at < right {
            let font = FontId::proportional(11.0);
            let hint = elide(ui, hint, font.clone(), right - at);
            painter.text(pos2(at, cy), Align2::LEFT_CENTER, hint, font, p.text_muted);
        }
    }
}

// --- Centre header ---------------------------------------------------------

pub struct HeaderProps<'a> {
    pub title: &'a str,
    pub subtitle: Option<String>,
    pub preview_open: bool,
    pub thumb_size: f32,
    pub zoom_label: Option<String>,
}

/// The bar above the grid or preview: Back, the title, and the thumbnail
/// size or zoom.
pub fn centre_header(ui: &mut Ui, props: &HeaderProps<'_>) -> Vec<ViewAction> {
    let p = theme::palette(ui.ctx());
    let mut out = Vec::new();
    let (rect, _) = ui.allocate_exact_size(
        vec2(ui.available_width(), PANE_HEADER_HEIGHT),
        Sense::hover(),
    );
    ui.painter().rect_filled(rect, 0.0, p.bg_surface);
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        Stroke::new(1.0, p.border),
    );
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(vec2(10.0, 4.0)))
            .layout(Layout::left_to_right(Align::Center)),
        |ui| {
            if props.preview_open {
                if dialog::icon_button(
                    ui,
                    Some(icons::ARROW_LEFT),
                    "Back",
                    ButtonKind::Secondary,
                    true,
                )
                .on_hover_text("Back to the grid (Esc)")
                .clicked()
                {
                    out.push(ViewAction::Act(Act::Back));
                }
                ui.add_space(6.0);
            }
            ui.label(
                RichText::new(props.title)
                    .size(13.0)
                    .strong()
                    .color(p.text_primary),
            );
            if let Some(sub) = &props.subtitle {
                ui.label(RichText::new(sub).size(12.0).color(p.text_muted));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if props.preview_open {
                    if ui
                        .small_button("100%")
                        .on_hover_text("Actual size (1)")
                        .clicked()
                    {
                        out.push(ViewAction::Act(Act::ZoomActual));
                    }
                    if ui
                        .small_button("Fit")
                        .on_hover_text("Fit to window (0)")
                        .clicked()
                    {
                        out.push(ViewAction::Act(Act::ZoomFit));
                    }
                    if let Some(z) = &props.zoom_label {
                        ui.label(RichText::new(z).size(12.0).color(p.text_muted));
                    }
                } else {
                    let mut size = props.thumb_size;
                    let slider = egui::Slider::new(
                        &mut size,
                        crate::config::THUMB_MIN..=crate::config::THUMB_MAX,
                    )
                    .show_value(false);
                    let resp = ui
                        .add_sized(vec2(120.0, 18.0), slider)
                        .on_hover_text("Thumbnail size (Ctrl+= and Ctrl+-)");
                    resp.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::Role::Slider, true, "Thumbnail Size")
                    });
                    if resp.changed() {
                        out.push(ViewAction::SetThumbSize(size));
                    }
                    ui.label(icons::icon_sized(icons::IMAGES, 12.0).color(p.text_muted));
                }
            });
        },
    );
    out
}

// --- Grid ------------------------------------------------------------------

pub struct GridProps<'a> {
    pub folder: &'a Path,
    pub photos: &'a [&'a FileEntry],
    pub keys: &'a [String],
    pub selected: Option<&'a Path>,
    pub thumb_size: f32,
    pub reveal: bool,
}

pub struct GridOutput {
    pub actions: Vec<ViewAction>,
    /// The photos on screen, and a row either side, by index.
    pub visible: std::ops::Range<usize>,
    pub columns: usize,
}

pub fn grid(ui: &mut Ui, props: &GridProps<'_>, textures: &mut Textures) -> GridOutput {
    let p = theme::palette(ui.ctx());
    let mut actions = Vec::new();
    let n = props.photos.len();
    let cell = props.thumb_size;
    let width = ui.available_width() - 2.0 * GRID_GAP;
    let columns = (((width + GRID_GAP) / (cell + GRID_GAP)).floor() as usize).max(1);
    let rows = n.div_ceil(columns);
    let row_h = cell + NAME_HEIGHT + GRID_GAP;
    let id = ("medialiner-grid", props.folder);
    let mut area = ScrollArea::vertical()
        .id_salt(id)
        .auto_shrink([false, false]);
    if props.reveal {
        if let Some(i) = props
            .photos
            .iter()
            .position(|f| Some(f.path.as_path()) == props.selected)
        {
            let visible_h = ui.available_height();
            let state = egui::scroll_area::State::load(ui.ctx(), ui.make_persistent_id(id));
            let top = GRID_GAP + (i / columns) as f32 * row_h;
            let offset = state.map_or(0.0, |s| s.clamped_offset().y);
            if top < offset || top + row_h > offset + visible_h {
                area = area.vertical_scroll_offset((top - (visible_h - row_h) / 2.0).max(0.0));
            }
        }
    }
    let mut visible = 0..0;
    let mut menu = Vec::new();
    area.show_rows(ui, row_h, rows, |ui, range| {
        visible = (range.start.saturating_sub(1) * columns)..((range.end + 1) * columns).min(n);
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        if range.start == 0 {
            ui.add_space(GRID_GAP);
        }
        for r in range {
            let (row_rect, _) =
                ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::hover());
            for c in 0..columns {
                let i = r * columns + c;
                let Some(f) = props.photos.get(i) else { break };
                let at = pos2(
                    row_rect.left() + GRID_GAP + c as f32 * (cell + GRID_GAP),
                    row_rect.top(),
                );
                let rect = Rect::from_min_size(at, vec2(cell, cell + NAME_HEIGHT));
                let resp = ui.interact(
                    rect,
                    ui.make_persistent_id(("cell", &f.path)),
                    Sense::click(),
                );
                let selected = props.selected == Some(f.path.as_path());
                resp.widget_info(|| {
                    egui::WidgetInfo::selected(egui::Role::ListItem, true, selected, &f.name)
                });
                let thumb = textures.thumb(&f.path, &props.keys[i]);
                paint_cell(ui, &p, rect, cell, f, thumb, selected, resp.hovered());
                if resp.clicked() {
                    actions.push(ViewAction::FocusContent);
                    actions.push(ViewAction::Select(f.path.clone()));
                }
                if resp.double_clicked() {
                    actions.push(ViewAction::OpenPreview(f.path.clone()));
                }
                if resp.secondary_clicked() {
                    actions.push(ViewAction::FocusContent);
                    actions.push(ViewAction::Select(f.path.clone()));
                }
                resp.context_menu(|ui| photo_menu(ui, &f.path, true, &mut menu));
            }
        }
    });
    actions.extend(menu);
    GridOutput {
        actions,
        visible,
        columns,
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_cell(
    ui: &Ui,
    p: &Palette,
    rect: Rect,
    cell: f32,
    f: &FileEntry,
    thumb: Option<&Thumb>,
    selected: bool,
    hovered: bool,
) {
    let painter = ui.painter();
    let square = Rect::from_min_size(rect.min, vec2(cell, cell));
    let radius = CornerRadius::same(6);
    painter.rect_filled(square, radius, p.bg_surface);
    match thumb {
        Some(Thumb::Ready(tex)) => {
            painter.image(tex.id(), square, square_uv(tex.size_vec2()), Color32::WHITE);
        }
        Some(Thumb::Failed) => {
            painter.text(
                square.center(),
                Align2::CENTER_CENTER,
                icons::FILE_CIRCLE_QUESTION,
                icons::font(cell * 0.2),
                p.text_faint,
            );
        }
        None => {
            painter.text(
                square.center(),
                Align2::CENTER_CENTER,
                icons::IMAGE,
                icons::font(cell * 0.18),
                p.border,
            );
        }
    }
    if selected {
        painter.rect_stroke(
            square.expand(2.0),
            radius,
            Stroke::new(3.0, p.accent),
            StrokeKind::Outside,
        );
    } else if hovered {
        painter.rect_stroke(
            square,
            radius,
            Stroke::new(1.0, p.border_hover),
            StrokeKind::Inside,
        );
    }
    let font = FontId::proportional(11.5);
    let name = elide(ui, &f.name, font.clone(), cell - 4.0);
    painter.text(
        pos2(square.center().x, square.bottom() + NAME_HEIGHT / 2.0 + 1.0),
        Align2::CENTER_CENTER,
        name,
        font,
        if selected {
            p.text_primary
        } else {
            p.text_secondary
        },
    );
}

/// The UV rectangle that crops an image of `size` to its centre square.
pub fn square_uv(size: Vec2) -> Rect {
    if size.x <= 0.0 || size.y <= 0.0 {
        return Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    }
    if size.x > size.y {
        let w = size.y / size.x;
        Rect::from_min_max(pos2((1.0 - w) / 2.0, 0.0), pos2((1.0 + w) / 2.0, 1.0))
    } else {
        let h = size.x / size.y;
        Rect::from_min_max(pos2(0.0, (1.0 - h) / 2.0), pos2(1.0, (1.0 + h) / 2.0))
    }
}

// --- Preview ---------------------------------------------------------------

/// How the preview is zoomed and panned.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreviewView {
    /// Fitted to the area; `zoom` and `pan` are ignored.
    pub fit: bool,
    /// Screen points per image pixel.
    pub zoom: f32,
    /// The image centre's offset from the area's centre.
    pub pan: Vec2,
}

impl Default for PreviewView {
    fn default() -> Self {
        Self {
            fit: true,
            zoom: 1.0,
            pan: Vec2::ZERO,
        }
    }
}

impl PreviewView {
    /// The zoom that fits `image` in `area`, never above 1.
    pub fn fit_zoom(image: Vec2, area: Vec2) -> f32 {
        if image.x <= 0.0 || image.y <= 0.0 {
            return 1.0;
        }
        (area.x / image.x).min(area.y / image.y).min(1.0)
    }

    /// The zoom in effect.
    pub fn effective(&self, image: Vec2, area: Vec2) -> f32 {
        if self.fit {
            Self::fit_zoom(image, area)
        } else {
            self.zoom
        }
    }

    /// Zooms by `factor` around `anchor` (relative to the area's centre).
    pub fn zoom_by(&mut self, factor: f32, anchor: Vec2, image: Vec2, area: Vec2) {
        let old = self.effective(image, area);
        let new = (old * factor).clamp(ZOOM_MIN, ZOOM_MAX);
        let pan = if self.fit { Vec2::ZERO } else { self.pan };
        self.pan = anchor - (anchor - pan) * (new / old);
        self.zoom = new;
        self.fit = false;
    }
}

pub struct PreviewProps<'a> {
    pub path: &'a Path,
    pub texture: Option<&'a Result<TextureHandle, String>>,
    /// The thumbnail, scaled up while the full image decodes.
    pub thumb: Option<&'a TextureHandle>,
}

pub fn preview(ui: &mut Ui, props: &PreviewProps<'_>, view: &mut PreviewView) -> Vec<ViewAction> {
    let p = theme::palette(ui.ctx());
    let mut out = Vec::new();
    let area = ui.available_rect_before_wrap();
    let resp = ui.allocate_rect(area, Sense::click_and_drag());
    resp.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::Role::Image,
            true,
            props
                .path
                .file_name()
                .map(|n| n.to_string_lossy())
                .unwrap_or_default(),
        )
    });
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, p.bg_base);
    let tex = match props.texture {
        Some(Ok(t)) => Some(t),
        Some(Err(e)) => {
            painter.text(
                area.center(),
                Align2::CENTER_CENTER,
                format!("Couldn't open this photo: {e}"),
                FontId::proportional(13.0),
                p.text_muted,
            );
            None
        }
        None => None,
    };
    let (shown, size) = match (tex, props.thumb) {
        (Some(t), _) => (Some(t), t.size_vec2()),
        (None, Some(t)) if props.texture.is_none() => {
            // The thumbnail, at the full image's aspect, until it's decoded.
            (Some(t), t.size_vec2())
        }
        _ => (None, Vec2::ZERO),
    };
    if let Some(t) = shown {
        let full = tex.is_some();
        // While the thumbnail stands in, fit it.
        let zoom = if full {
            view.effective(size, area.size())
        } else {
            (area.width() / size.x).min(area.height() / size.y)
        };
        let pan = if view.fit || !full {
            Vec2::ZERO
        } else {
            view.pan
        };
        let rect = Rect::from_center_size(area.center() + pan, size * zoom);
        painter.image(
            t.id(),
            rect,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
        if full {
            if resp.dragged() && !view.fit {
                view.pan += resp.drag_delta();
            }
            if resp.hovered() {
                // Ctrl+scroll and pinches (egui turns Ctrl+scroll into zoom).
                let factor = ui.input(|i| i.zoom_delta());
                if factor != 1.0 {
                    let anchor = ui
                        .input(|i| i.pointer.hover_pos())
                        .map_or(Vec2::ZERO, |pos| pos - area.center());
                    view.zoom_by(factor, anchor, size, area.size());
                }
            }
            if resp.double_clicked() {
                out.push(ViewAction::Act(if view.fit {
                    Act::ZoomActual
                } else {
                    Act::ZoomFit
                }));
            }
        }
    } else if props.texture.is_none() {
        painter.text(
            area.center(),
            Align2::CENTER_CENTER,
            "Opening…",
            FontId::proportional(13.0),
            p.text_muted,
        );
    }
    if resp.clicked() {
        out.push(ViewAction::FocusContent);
    }
    let mut menu = Vec::new();
    resp.context_menu(|ui| photo_menu(ui, props.path, true, &mut menu));
    out.extend(menu);
    out
}

// --- Info pane -------------------------------------------------------------

pub struct InfoProps<'a> {
    pub info: Option<&'a FileInfo>,
    pub loading: bool,
    pub show_location: bool,
}

fn when(t: Option<std::time::SystemTime>) -> Option<String> {
    t.map(|t| {
        DateTime::<Local>::from(t)
            .format("%Y-%m-%d %H:%M")
            .to_string()
    })
}

pub fn info_pane(ui: &mut Ui, props: &InfoProps<'_>) -> Vec<ViewAction> {
    let p = theme::palette(ui.ctx());
    let mut out = Vec::new();
    pane_header(ui, "Info", |ui| {
        if header_button(ui, "close-info", icons::XMARK, "Close Info") {
            out.push(ViewAction::CloseInfo);
        }
    });
    let Some(info) = props.info else {
        empty_note(
            ui,
            if props.loading {
                "Reading…"
            } else {
                "No photo selected."
            },
        );
        return out;
    };
    ScrollArea::vertical()
        .id_salt("medialiner-info")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            Frame::NONE.inner_margin(Margin::symmetric(14, 10)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(&info.name).size(14.0).strong().color(p.text_primary));
                ui.add_space(8.0);
                section(ui, &p, "File");
                field(ui, &p, "Folder", Some(info.folder.display().to_string()));
                if ui.small_button("Open in Folder").clicked() {
                    out.push(ViewAction::Photo(info.path.clone(), PhotoCommand::OpenInFolder));
                }
                ui.add_space(4.0);
                field(ui, &p, "Size", Some(human_size(info.size)));
                field(ui, &p, "Format", Some(info.format.clone()));
                field(ui, &p, "Dimensions", info.dimensions.map(|(w, h)| format!("{w} × {h}")));
                field(ui, &p, "Created", when(info.created));
                field(ui, &p, "Modified", when(info.modified));
                if let Some(photo) = &info.photo {
                    ui.add_space(8.0);
                    section(ui, &p, "Photo");
                    field(ui, &p, "Taken", photo.taken.clone());
                    field(ui, &p, "Camera", photo.camera.clone());
                    field(ui, &p, "Lens", photo.lens.clone());
                    field(ui, &p, "Focal Length", photo.focal_length.clone());
                    field(ui, &p, "Aperture", photo.aperture.clone());
                    field(ui, &p, "Shutter Speed", photo.exposure.clone());
                    field(ui, &p, "ISO", photo.iso.clone());
                    field(ui, &p, "Flash", photo.flash.clone());
                    field(ui, &p, "Orientation", photo.orientation.clone());
                    if let Some(loc) = photo.location {
                        ui.add_space(8.0);
                        section(ui, &p, "Location");
                        if props.show_location {
                            let text = format!("{:.6}, {:.6}", loc.0, loc.1);
                            field(ui, &p, "Latitude, Longitude", Some(text.clone()));
                            ui.horizontal(|ui| {
                                if ui.small_button("Copy").clicked() {
                                    out.push(ViewAction::Copy(text));
                                }
                                if ui.small_button("Open in Maps").clicked() {
                                    out.push(ViewAction::OpenUrl(info::map_url(loc)));
                                }
                                if ui.small_button("Hide").clicked() {
                                    out.push(ViewAction::ShowLocation(false));
                                }
                            });
                        } else {
                            dialog::muted_text(ui, "This photo has a location. It's hidden until you choose to show it.");
                            if ui.small_button("Show Location").clicked() {
                                out.push(ViewAction::ShowLocation(true));
                            }
                        }
                    }
                }
            });
        });
    out
}

fn section(ui: &mut Ui, p: &Palette, title: &str) {
    ui.label(
        RichText::new(title.to_uppercase())
            .size(11.0)
            .color(p.text_muted),
    );
    ui.add_space(2.0);
}

fn field(ui: &mut Ui, p: &Palette, label: &str, value: Option<String>) {
    let Some(value) = value else { return };
    ui.label(RichText::new(label).size(11.5).color(p.text_tertiary));
    ui.add(
        egui::Label::new(RichText::new(value).size(13.0).color(p.text_primary))
            .selectable(true)
            .wrap(),
    );
    ui.add_space(4.0);
}

// --- Empty states ----------------------------------------------------------

/// The centre with no folder selected.
pub fn welcome(ui: &mut Ui, has_folders: bool) -> bool {
    let p = theme::palette(ui.ctx());
    let mut add = false;
    ui.vertical_centered(|ui| {
        ui.add_space((ui.available_height() / 2.0 - 80.0).max(24.0));
        ui.label(icons::icon_sized(icons::IMAGES, 40.0).color(p.text_faint));
        ui.add_space(12.0);
        if has_folders {
            ui.label(RichText::new("Select a folder").size(16.0).color(p.text_primary));
            ui.add_space(4.0);
            dialog::muted_text(ui, "Its photos show here.");
        } else {
            ui.label(RichText::new("No folders yet").size(16.0).color(p.text_primary));
            ui.add_space(4.0);
            dialog::muted_text(ui, "Add a folder of photos. MediaLiner shows them where they are; nothing is copied or imported.");
            ui.add_space(12.0);
            add = dialog::button(ui, "Add Folder…", ButtonKind::Primary, true).clicked();
        }
    });
    add
}

/// The centre for a folder with no photos.
pub fn no_photos(ui: &mut Ui, loading: bool, error: Option<&str>, others: usize) {
    let p = theme::palette(ui.ctx());
    ui.vertical_centered(|ui| {
        ui.add_space((ui.available_height() / 2.0 - 40.0).max(24.0));
        let text = match (error, loading) {
            (Some(e), _) => format!("Couldn't read this folder: {e}"),
            (None, true) => "Reading…".to_string(),
            (None, false) if others > 0 => {
                format!(
                    "No photos here. {} other {} in the Files pane.",
                    others,
                    if others == 1 { "file is" } else { "files are" }
                )
            }
            (None, false) => "No photos in this folder.".to_string(),
        };
        ui.label(RichText::new(text).size(13.0).color(p.text_muted));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crops_to_the_centre_square() {
        let uv = square_uv(vec2(200.0, 100.0));
        assert_eq!(
            (uv.min.x, uv.max.x, uv.min.y, uv.max.y),
            (0.25, 0.75, 0.0, 1.0)
        );
        let uv = square_uv(vec2(100.0, 400.0));
        assert_eq!((uv.min.y, uv.max.y), (0.375, 0.625));
    }

    #[test]
    fn zooms_around_the_pointer() {
        let image = vec2(4000.0, 2000.0);
        let area = vec2(1000.0, 800.0);
        let mut v = PreviewView::default();
        assert_eq!(v.effective(image, area), 0.25);
        // Zooming about a point keeps that point still.
        let anchor = vec2(100.0, 0.0);
        let before = (anchor - v.pan) / v.effective(image, area);
        v.zoom_by(2.0, anchor, image, area);
        assert!(!v.fit);
        assert_eq!(v.zoom, 0.5);
        let after = (anchor - v.pan) / v.zoom;
        assert!((before - after).length() < 1e-3);
        // A small image fits at 100%, not larger.
        assert_eq!(PreviewView::fit_zoom(vec2(100.0, 100.0), area), 1.0);
    }
}
