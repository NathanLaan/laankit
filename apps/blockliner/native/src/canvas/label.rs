//! Editing a shape's label in place: a text field over the shape. Enter
//! commits, Shift+Enter adds a line, Esc cancels, and clicking elsewhere
//! commits.
//!
//! A new shape's id comes from its first label (`"Orders DB"` →
//! `orders_db`), so the Mermaid file reads well. After that the id stays put
//! when the label changes, since connectors and kept statements refer to it.

use blockliner_core::{ids, Diagram};
use egui::{vec2, Align2, Event, Frame, Key, Margin, TextEdit, Ui};
use marina_egui::theme;

use super::{paint, Canvas};

pub(super) struct LabelEdit {
    pub id: String,
    pub text: String,
    /// The shape was just created, so it takes its id from this label.
    pub fresh: bool,
    pub focused: bool,
}

impl Canvas {
    pub(super) fn edit_label(&mut self, id: String, text: String, fresh: bool) {
        self.label = Some(LabelEdit {
            id,
            text,
            fresh,
            focused: false,
        });
    }

    #[cfg(test)]
    pub fn is_editing_label(&self) -> bool {
        self.label.is_some()
    }

    /// Puts the label being edited, if any, into the diagram.
    pub fn commit_label(&mut self, d: &mut Diagram) {
        let Some(edit) = self.label.take() else {
            return;
        };
        let Some(shape) = d.shape_mut(&edit.id) else {
            return;
        };
        shape.label = edit.text.clone();
        if edit.fresh && !edit.text.trim().is_empty() {
            let new = ids::unique(&ids::from_label(&edit.text), |id| {
                id != edit.id && d.shape(id).is_some()
            });
            if d.rename_shape(&edit.id, &new) {
                for s in &mut self.selection {
                    if *s == edit.id {
                        *s = new.clone();
                    }
                }
            }
        }
    }

    /// The text field, while a label is being edited.
    pub(super) fn label_editor(&mut self, ui: &mut Ui, d: &mut Diagram) {
        let (view, origin) = (self.view, self.rect.min);
        let Some(edit) = &self.label else {
            return;
        };
        let Some(shape) = d.shape(&edit.id) else {
            self.label = None;
            return;
        };
        let screen = paint::screen_rect(view, origin, &shape.rect);

        // Enter and Esc are ours, before the text field sees them.
        let (enter, esc) = ui.input_mut(|i| {
            let (mut enter, mut esc) = (false, false);
            i.events.retain(|e| match e {
                Event::Key {
                    key: Key::Enter,
                    pressed: true,
                    modifiers,
                    ..
                } if !modifiers.shift => {
                    enter = true;
                    false
                }
                Event::Key {
                    key: Key::Escape,
                    pressed: true,
                    ..
                } => {
                    esc = true;
                    false
                }
                _ => true,
            });
            (enter, esc)
        });
        if esc {
            self.label = None;
            return;
        }
        if enter {
            self.commit_label(d);
            return;
        }

        let p = theme::palette(ui.ctx());
        let font = paint::label_font(view.zoom);
        let row = ui.ctx().fonts_mut(|f| f.row_height(&font));
        let Some(edit) = &mut self.label else {
            return;
        };
        let rows = edit.text.split('\n').count().max(1);
        let pad = paint::LABEL_PAD * view.zoom;
        let size = vec2(
            (screen.width() - 2.0 * pad).max(row),
            (rows as f32 * row).min(screen.height()).max(row),
        );
        let area = egui::Rect::from_center_size(screen.center(), size);
        // The colour the label will be drawn in, so it reads on the fill.
        let ink = shape
            .style
            .label_rgba()
            .map(|[r, g, b, a]| egui::Color32::from_rgba_unmultiplied(r, g, b, a))
            .unwrap_or(p.text_primary);
        let field = TextEdit::multiline(&mut edit.text)
            .id_salt(("blockliner-label", edit.id.as_str()))
            .font(font)
            .text_color(ink)
            .align(Align2::CENTER_TOP)
            .frame(Frame::NONE)
            .margin(Margin::ZERO)
            .desired_rows(rows)
            .desired_width(size.x);
        let resp = ui.put(area, field);
        if !edit.focused {
            resp.request_focus();
            edit.focused = true;
        } else if resp.lost_focus() {
            self.commit_label(d);
        }
    }
}
