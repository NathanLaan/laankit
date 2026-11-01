//! The Attachments panel (`AttachmentPanel.svelte`): the selected note's
//! attachments, images as thumbnails and other files as a type icon.
//! Double-click opens one in the system app, `×` removes it (after a
//! confirmation), `+` adds files (30 MB limit), and a right-click inserts
//! a Markdown reference at the caret (what the Svelte editor does for a
//! pasted or dropped file).

use std::path::{Path, PathBuf};

use egui::{pos2, vec2, Align2, FontId, Frame, Margin, Rect, RichText, Sense, Ui};
use laankit_egui::icons;
use laankit_egui::theme::{self, Palette};
use serde_json::Value;

/// `MAX_SIZE`: larger files are refused (`file:addAttachment` refuses them too).
pub const MAX_SIZE: u64 = 30 * 1024 * 1024;
const IMAGE_EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "gif", "webp", "svg"];
const ITEM_HEIGHT: f32 = 132.0;

/// One attachment of the selected note (`file.attachments[]`).
#[derive(Clone, Debug, PartialEq)]
pub struct Attachment {
    pub id: String,
    pub original_name: String,
    /// The stored name in `_attachments/`.
    pub filename: String,
}

impl Attachment {
    pub fn from_value(v: &Value) -> Option<Self> {
        Some(Self {
            id: v["id"].as_str()?.to_string(),
            original_name: v["originalName"].as_str().unwrap_or_default().to_string(),
            filename: v["filename"].as_str()?.to_string(),
        })
    }

    pub fn is_image(&self) -> bool {
        IMAGE_EXTENSIONS.contains(&extension(&self.original_name).as_str())
    }

    /// `insertMarkdownReference`: `![name](./_attachments/file)` for an
    /// image, `[name](./_attachments/file)` otherwise.
    pub fn markdown(&self) -> String {
        let path = format!("./_attachments/{}", self.filename);
        if self.is_image() {
            format!("![{}]({path})", self.original_name)
        } else {
            format!("[{}]({path})", self.original_name)
        }
    }
}

/// What the panel asks the app to do.
#[derive(Clone, Debug, PartialEq)]
pub enum AttachmentAction {
    Close,
    /// `+`: pick files to attach to the selected note.
    Add,
    /// Double-click: open the stored file with the system app.
    Open(PathBuf),
    /// `×`: remove after confirming (`Remove "name"?`).
    Remove(Attachment),
    /// Put this Markdown at the editor's caret.
    Insert(String),
}

/// The attachments of the note in `index` with this id.
pub fn of_note(index: &Value, id: &str) -> Vec<Attachment> {
    index["files"]
        .as_array()
        .and_then(|files| files.iter().find(|f| f["id"].as_str() == Some(id)))
        .and_then(|f| f["attachments"].as_array())
        .map(|a| a.iter().filter_map(Attachment::from_value).collect())
        .unwrap_or_default()
}

pub fn show(
    ui: &mut Ui,
    selected: bool,
    attachments: &[Attachment],
    dir: &Path,
) -> Vec<AttachmentAction> {
    let p = theme::palette(ui.ctx());
    let mut out = Vec::new();
    egui::Panel::top(ui.scope_id().with("attachments-header"))
        .frame(
            Frame::NONE
                .fill(p.bg_surface)
                .inner_margin(Margin::symmetric(12, 6))
                .stroke(egui::Stroke::new(1.0, p.border)),
        )
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("ATTACHMENTS").size(11.0).color(p.text_muted));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icon_button(ui, icons::XMARK, "Close attachments") {
                        out.push(AttachmentAction::Close);
                    }
                    if selected && icon_button(ui, icons::PLUS, "Add attachment") {
                        out.push(AttachmentAction::Add);
                    }
                });
            });
        });
    egui::ScrollArea::vertical()
        .id_salt("noteliner-attachments")
        .auto_shrink(false)
        .show(ui, |ui| {
            Frame::NONE.inner_margin(Margin::same(8)).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;
                if !selected {
                    empty(ui, &p, "No file selected");
                    return;
                }
                if attachments.is_empty() {
                    empty(ui, &p, "No attachments");
                    return;
                }
                for a in attachments {
                    item(ui, &p, a, dir, &mut out);
                }
            });
        });
    out
}

fn icon_button(ui: &mut Ui, glyph: &str, label: &str) -> bool {
    let resp = ui
        .add(egui::Button::new(icons::icon_sized(glyph, 12.0)).frame(false))
        .on_hover_text(label);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    resp.clicked()
}

fn empty(ui: &mut Ui, p: &Palette, text: &str) {
    ui.add_space(8.0);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(text).size(12.5).color(p.text_muted));
    });
}

/// `.attachment-item`: a thumbnail or type icon, the name, and `×`.
fn item(ui: &mut Ui, p: &Palette, a: &Attachment, dir: &Path, out: &mut Vec<AttachmentAction>) {
    let (rect, resp) =
        ui.allocate_exact_size(vec2(ui.available_width(), ITEM_HEIGHT), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ListItem, true, &a.original_name));
    let resp = resp.on_hover_text(format!(
        "{}\nDouble-click to open; right-click to insert into the note",
        a.original_name
    ));
    ui.painter().rect(
        rect,
        theme::RADIUS,
        if resp.hovered() {
            p.bg_item_hover
        } else {
            p.bg_base
        },
        egui::Stroke::new(1.0, p.border),
        egui::StrokeKind::Inside,
    );
    let thumb = Rect::from_min_max(
        rect.min + vec2(8.0, 8.0),
        pos2(rect.right() - 8.0, rect.bottom() - 30.0),
    );
    let path = dir.join(&a.filename);
    // Loaded like the Preview's images (`bytes://attachments/…`).
    let uri = a.is_image().then(|| {
        use laankit_richtext::Resolver;
        crate::views::preview::AttachmentResolver {
            dir: Some(dir.to_path_buf()),
        }
        .image(ui.ctx(), &format!("_attachments/{}", a.filename))
    });
    if let Some(Some(uri)) = uri {
        egui::Image::new(uri)
            .fit_to_exact_size(thumb.size())
            .maintain_aspect_ratio(true)
            .corner_radius(theme::RADIUS)
            .paint_at(ui, thumb);
    } else {
        ui.painter().text(
            thumb.center() - vec2(0.0, 8.0),
            Align2::CENTER_CENTER,
            file_icon(&a.original_name),
            icons::font(32.0),
            p.text_muted,
        );
        ui.painter().text(
            thumb.center() + vec2(0.0, 22.0),
            Align2::CENTER_CENTER,
            file_type(&a.original_name),
            FontId::proportional(10.5),
            p.text_faint,
        );
    }
    let name_font = FontId::proportional(12.0);
    let name =
        crate::views::tree::elide(ui, &a.original_name, name_font.clone(), rect.width() - 40.0);
    ui.painter().text(
        pos2(rect.left() + 10.0, rect.bottom() - 15.0),
        Align2::LEFT_CENTER,
        name,
        name_font,
        p.text_secondary,
    );
    let del = Rect::from_center_size(
        pos2(rect.right() - 16.0, rect.bottom() - 15.0),
        vec2(20.0, 20.0),
    );
    let del_resp = ui.interact(del, ui.scope_id().with(("att-del", &a.id)), Sense::click());
    del_resp.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::Role::Button,
            true,
            format!("Remove {}", a.original_name),
        )
    });
    let del_resp = del_resp.on_hover_text("Remove attachment");
    ui.painter().text(
        del.center(),
        Align2::CENTER_CENTER,
        icons::XMARK,
        icons::font(11.0),
        if del_resp.hovered() {
            p.text_primary
        } else {
            p.text_muted
        },
    );
    if del_resp.clicked() {
        out.push(AttachmentAction::Remove(a.clone()));
    } else if resp.double_clicked() {
        out.push(AttachmentAction::Open(path));
    }
    resp.context_menu(|ui| {
        if ui.button("Insert into Note").clicked() {
            out.push(AttachmentAction::Insert(a.markdown()));
            ui.close();
        }
        if ui.button("Copy Markdown").clicked() {
            ui.ctx().copy_text(a.markdown());
            ui.close();
        }
        if ui.button("Open").clicked() {
            out.push(AttachmentAction::Open(dir.join(&a.filename)));
            ui.close();
        }
    });
}

fn extension(name: &str) -> String {
    name.rsplit_once('.')
        .map(|(_, e)| e.to_lowercase())
        .unwrap_or_default()
}

/// `getFileIcon`.
fn file_icon(name: &str) -> &'static str {
    match extension(name).as_str() {
        "pdf" => icons::FILE_PDF,
        "doc" | "docx" => icons::FILE_WORD,
        "xls" | "xlsx" => icons::FILE_EXCEL,
        "csv" => icons::FILE_CSV,
        "zip" | "rar" | "gz" => icons::FILE_ZIPPER,
        "mp3" | "wav" => icons::FILE_AUDIO,
        "mp4" | "mov" => icons::FILE_VIDEO,
        "txt" => icons::FILE_LINES,
        "json" | "js" | "html" => icons::FILE_CODE,
        _ => icons::FILE,
    }
}

/// `getFileType`: the extension in capitals, or `FILE`.
fn file_type(name: &str) -> String {
    match extension(name) {
        e if e.is_empty() => "FILE".into(),
        e => e.to_uppercase(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn markdown_references() {
        let img = Attachment {
            id: "1".into(),
            original_name: "Photo.JPG".into(),
            filename: "att-1.jpg".into(),
        };
        assert!(img.is_image());
        assert_eq!(img.markdown(), "![Photo.JPG](./_attachments/att-1.jpg)");
        let doc = Attachment {
            original_name: "report.pdf".into(),
            filename: "att-2.pdf".into(),
            ..img
        };
        assert_eq!(doc.markdown(), "[report.pdf](./_attachments/att-2.pdf)");
        assert_eq!(file_type("x"), "FILE");
        assert_eq!(file_type("a.tar.gz"), "GZ");
    }

    #[test]
    fn attachments_of_a_note() {
        let index = json!({ "files": [
            { "id": "a", "attachments": [{ "id": "1", "originalName": "x.png", "filename": "att-1.png" }] },
            { "id": "b" }
        ]});
        assert_eq!(of_note(&index, "a").len(), 1);
        assert!(of_note(&index, "b").is_empty());
        assert!(of_note(&index, "z").is_empty());
    }
}
