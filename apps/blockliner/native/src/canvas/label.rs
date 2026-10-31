//! Editing a label in place: a text field over the shape, or at a
//! connector's midpoint for its edge label. Enter commits, Shift+Enter adds
//! a line, Esc cancels, and clicking elsewhere commits.
//!
//! A new shape's id comes from its first label (`"Orders DB"` →
//! `orders_db`), so the Mermaid file reads well. After that the id stays put
//! when the label changes, since connectors and kept statements refer to it.
//! An empty edge label removes it.

use blockliner_core::connect::path_midpoint;
use blockliner_core::{ids, Diagram, Rect as ModelRect};
use egui::{vec2, Align2, Event, Frame, Key, Margin, Stroke, TextEdit, Ui};
use marina_egui::theme;

use super::{paint, pos, Canvas};

/// What a label edit changes.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Target {
    Shape(String),
    /// A connector, by index.
    Line(usize),
}

pub(super) struct LabelEdit {
    pub target: Target,
    pub text: String,
    /// The shape was just created, so it takes its id from this label.
    pub fresh: bool,
    pub focused: bool,
}

/// An edge label's field is at least this many grid cells wide.
const LINE_FIELD_CELLS: f32 = 6.0;

impl Canvas {
    pub(super) fn edit_label(&mut self, id: String, text: String, fresh: bool) {
        self.label = Some(LabelEdit {
            target: Target::Shape(id),
            text,
            fresh,
            focused: false,
        });
    }

    /// Edits connector `i`'s edge label, selecting it alone. Only a Mermaid
    /// edge (both ends glued) has somewhere to keep one.
    pub(super) fn edit_line_label(&mut self, d: &Diagram, i: usize) -> bool {
        let Some(c) = d.connectors.get(i).filter(|c| c.is_edge()) else {
            return false;
        };
        self.selection.clear();
        self.lines = vec![i];
        self.label = Some(LabelEdit {
            target: Target::Line(i),
            text: c.label.clone().unwrap_or_default(),
            fresh: false,
            focused: false,
        });
        true
    }

    /// Whether the single selected item's label can be edited: a shape
    /// BlockLiner declares, or a Mermaid edge.
    pub fn can_edit_label(&self, d: &Diagram) -> bool {
        match (self.selection.as_slice(), self.lines.as_slice()) {
            ([id], []) => d.shape(id).is_some_and(|s| !s.external),
            ([], [i]) => d.connectors.get(*i).is_some_and(|c| c.is_edge()),
            _ => false,
        }
    }

    /// Starts editing the single selected item's label (Enter, Edit Label).
    pub fn edit_selected_label(&mut self, d: &Diagram) -> bool {
        if !self.can_edit_label(d) {
            return false;
        }
        match (self.selection.first(), self.lines.first()) {
            (Some(id), _) => {
                let text = d.shape(id).map(|s| s.label.clone()).unwrap_or_default();
                self.edit_label(id.clone(), text, false);
                true
            }
            (None, Some(&i)) => self.edit_line_label(d, i),
            (None, None) => false,
        }
    }

    #[cfg(test)]
    pub fn is_editing_label(&self) -> bool {
        self.label.is_some()
    }

    /// The shape whose label is being edited, if any.
    pub(super) fn editing_shape(&self) -> Option<&str> {
        match &self.label.as_ref()?.target {
            Target::Shape(id) => Some(id),
            Target::Line(_) => None,
        }
    }

    /// The connector whose label is being edited, if any.
    pub(super) fn editing_line(&self) -> Option<usize> {
        match self.label.as_ref()?.target {
            Target::Line(i) => Some(i),
            Target::Shape(_) => None,
        }
    }

    /// Puts the label being edited, if any, into the diagram.
    pub fn commit_label(&mut self, d: &mut Diagram) {
        let Some(edit) = self.label.take() else {
            return;
        };
        let id = match edit.target {
            Target::Line(i) => {
                if let Some(c) = d.connectors.get_mut(i) {
                    c.label = Some(edit.text).filter(|t| !t.trim().is_empty());
                }
                return;
            }
            Target::Shape(id) => id,
        };
        let Some(shape) = d.shape_mut(&id) else {
            return;
        };
        shape.label = edit.text.clone();
        if edit.fresh && !edit.text.trim().is_empty() {
            let new = ids::unique(&ids::from_label(&edit.text), |other| {
                other != id && d.shape(other).is_some()
            });
            if d.rename_shape(&id, &new) {
                for s in &mut self.selection {
                    if *s == id {
                        *s = new.clone();
                    }
                }
            }
        }
    }

    /// Each labelled connector's label patch in model units, topmost first,
    /// for hit-testing (clicking a label picks its connector).
    pub(super) fn edge_labels(ui: &Ui, d: &Diagram) -> Vec<(usize, ModelRect)> {
        let font = paint::edge_label_font(1.0);
        ui.ctx().fonts_mut(|f| {
            d.connectors
                .iter()
                .enumerate()
                .rev()
                .filter_map(|(i, c)| {
                    let text = c.label.as_deref().filter(|t| !t.is_empty())?;
                    let at = path_midpoint(&d.connector_path(c)?)?;
                    let size = f
                        .layout_no_wrap(text.to_owned(), font.clone(), egui::Color32::WHITE)
                        .size();
                    let (w, h) = (
                        size.x + 2.0 * paint::EDGE_LABEL_PAD,
                        size.y + 2.0 * paint::EDGE_LABEL_PAD,
                    );
                    Some((i, ModelRect::new(at.x - w / 2.0, at.y - h / 2.0, w, h)))
                })
                .collect()
        })
    }

    /// The text field, while a label is being edited.
    pub(super) fn label_editor(&mut self, ui: &mut Ui, d: &mut Diagram) {
        let (view, origin) = (self.view, self.rect.min);
        let Some(edit) = &self.label else {
            return;
        };
        let p = theme::palette(ui.ctx());
        // Where it goes, its font and its colour: inside the shape's label
        // area (so inside a diamond's outline), or centred on the
        // connector's midpoint.
        let (area, font, ink, line) = match &edit.target {
            Target::Shape(id) => {
                let Some(shape) = d.shape(id) else {
                    self.label = None;
                    return;
                };
                let screen = paint::screen_rect(view, origin, &shape.label_rect());
                // The colour the label will be drawn in, so it reads on the
                // fill.
                let ink = shape
                    .style
                    .label_rgba()
                    .map(|[r, g, b, a]| egui::Color32::from_rgba_unmultiplied(r, g, b, a))
                    .unwrap_or(p.text_primary);
                (screen, paint::label_font(view.zoom), ink, false)
            }
            Target::Line(i) => {
                let at = d
                    .connectors
                    .get(*i)
                    .and_then(|c| d.connector_path(c))
                    .and_then(|path| path_midpoint(&path));
                let Some(at) = at else {
                    self.label = None;
                    return;
                };
                let font = paint::edge_label_font(view.zoom);
                let width = ui
                    .ctx()
                    .fonts_mut(|f| {
                        edit.text
                            .split('\n')
                            .map(|l| {
                                f.layout_no_wrap(l.to_owned(), font.clone(), p.text_secondary)
                                    .size()
                                    .x
                            })
                            .fold(0.0, f32::max)
                    })
                    .max(LINE_FIELD_CELLS * d.grid * view.zoom);
                // Room for one more character, so typing doesn't wrap.
                let width = width + font.size;
                let centre = view.to_screen(origin, pos(at.x, at.y));
                let screen = egui::Rect::from_center_size(centre, vec2(width, 0.0));
                (screen, font, p.text_primary, true)
            }
        };

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

        let row = ui.ctx().fonts_mut(|f| f.row_height(&font));
        let Some(edit) = &mut self.label else {
            return;
        };
        let rows = edit.text.split('\n').count().max(1);
        let size = if line {
            vec2(area.width(), rows as f32 * row)
        } else {
            let pad = paint::LABEL_PAD * view.zoom;
            vec2(
                (area.width() - 2.0 * pad).max(row),
                (rows as f32 * row).min(area.height()).max(row),
            )
        };
        let field_rect = egui::Rect::from_center_size(area.center(), size);
        if line {
            // The patch an edge label sits on, outlined while it's edited.
            let patch = field_rect.expand(paint::EDGE_LABEL_PAD * view.zoom);
            let painter = ui.painter_at(self.rect);
            painter.rect_filled(patch, 2.0, p.bg_base);
            painter.rect_stroke(
                patch,
                2.0,
                Stroke::new(1.0, p.accent),
                egui::StrokeKind::Outside,
            );
        }
        let field = TextEdit::multiline(&mut edit.text)
            .id_salt(("blockliner-label", format!("{:?}", edit.target)))
            .font(font)
            .text_color(ink)
            .align(Align2::CENTER_TOP)
            .frame(Frame::NONE)
            .margin(Margin::ZERO)
            .desired_rows(rows)
            .desired_width(size.x);
        let resp = ui.put(field_rect, field);
        if !edit.focused {
            resp.request_focus();
            edit.focused = true;
        } else if resp.lost_focus() {
            self.commit_label(d);
        }
    }
}
