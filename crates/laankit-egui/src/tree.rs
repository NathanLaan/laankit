//! A tree of rows: folders, outlines, anything nested.
//!
//! The app owns the tree and which branches are open. Each frame it passes
//! the visible rows, already flattened in order with their depths, and gets
//! back what was clicked, toggled, or dropped. Rows can be dragged onto
//! other rows of the same tree, and each row has a right-click menu the app
//! fills in.
//!
//! ```ignore
//! let rows = [TreeRow::new(0, "Photos").branch(true).selected(true)];
//! let out = tree::show(ui, "folders", &rows, |ui, i| {
//!     if ui.button("Refresh").clicked() { … }
//! });
//! if let Some(i) = out.toggled { … }
//! ```

use egui::{pos2, vec2, Align2, FontId, Id, Rect, Sense, Stroke, Ui};

use crate::icons;
use crate::theme::{self, Palette};

/// A row's height.
pub const ROW_HEIGHT: f32 = 26.0;
/// How far each level is indented.
pub const INDENT: f32 = 14.0;
/// The space left of a top-level row's chevron.
const LEFT_PAD: f32 = 20.0;

/// A visible row.
#[derive(Clone, Debug)]
pub struct TreeRow<'a> {
    pub depth: usize,
    pub label: &'a str,
    /// Muted text after the label (a folder's parent path, a count).
    pub hint: Option<&'a str>,
    pub icon: &'a str,
    /// `None` for a leaf, `Some(open)` for a branch with a chevron.
    pub branch: Option<bool>,
    pub selected: bool,
    /// Greyed out (a folder that isn't there).
    pub muted: bool,
    /// Can be dragged onto another row.
    pub draggable: bool,
    /// Accepts a dragged row.
    pub accepts_drop: bool,
    /// Scroll it into view this frame.
    pub reveal: bool,
}

impl<'a> TreeRow<'a> {
    pub fn new(depth: usize, label: &'a str) -> Self {
        Self {
            depth,
            label,
            hint: None,
            icon: icons::FOLDER,
            branch: None,
            selected: false,
            muted: false,
            draggable: false,
            accepts_drop: false,
            reveal: false,
        }
    }

    pub fn hint(mut self, hint: Option<&'a str>) -> Self {
        self.hint = hint;
        self
    }

    pub fn icon(mut self, icon: &'a str) -> Self {
        self.icon = icon;
        self
    }

    /// A branch, open or closed.
    pub fn branch(mut self, open: bool) -> Self {
        self.branch = Some(open);
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn muted(mut self, muted: bool) -> Self {
        self.muted = muted;
        self
    }

    /// Can be dragged, and accepts drops.
    pub fn drag_and_drop(mut self, on: bool) -> Self {
        self.draggable = on;
        self.accepts_drop = on;
        self
    }

    pub fn reveal(mut self, reveal: bool) -> Self {
        self.reveal = reveal;
        self
    }
}

/// What happened this frame, by row index.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TreeOutput {
    pub clicked: Option<usize>,
    pub double_clicked: Option<usize>,
    /// Its chevron was clicked.
    pub toggled: Option<usize>,
    /// `(dragged, onto)`.
    pub dropped: Option<(usize, usize)>,
    /// Its right-click menu opened this frame.
    pub context: Option<usize>,
}

/// The drag payload: which tree, and which row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Dragged {
    tree: Id,
    row: usize,
}

/// Draws `rows`, one under another, at the full width. `context_menu` fills
/// the right-click menu of row `i`. Put it in a scroll area for long trees.
pub fn show(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    rows: &[TreeRow<'_>],
    mut context_menu: impl FnMut(&mut Ui, usize),
) -> TreeOutput {
    let p = theme::palette(ui.ctx());
    let tree = ui.make_persistent_id(id_salt);
    let mut out = TreeOutput::default();
    let spacing = ui.spacing().item_spacing.y;
    ui.spacing_mut().item_spacing.y = 0.0;
    for (i, row) in rows.iter().enumerate() {
        let sense = if row.draggable {
            Sense::click_and_drag()
        } else {
            Sense::click()
        };
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), sense);
        resp.widget_info(|| {
            egui::WidgetInfo::selected(egui::Role::TreeItem, !row.muted, row.selected, row.label)
        });
        let indent = LEFT_PAD + row.depth as f32 * INDENT;
        let mut toggled = false;
        if row.branch.is_some() {
            let chevron = Rect::from_min_size(
                pos2(rect.left() + indent - 16.0, rect.top()),
                vec2(16.0, ROW_HEIGHT),
            );
            let c = ui.interact(chevron, tree.with(("chevron", i)), Sense::click());
            c.widget_info(|| {
                let open = row.branch == Some(true);
                egui::WidgetInfo::labeled(
                    egui::Role::Button,
                    true,
                    format!("{} {}", if open { "Collapse" } else { "Expand" }, row.label),
                )
            });
            if c.clicked() {
                toggled = true;
                out.toggled = Some(i);
            }
        }
        if row.draggable && resp.drag_started() {
            resp.dnd_set_drag_payload(Dragged { tree, row: i });
        }
        let mut drop_hover = false;
        if row.accepts_drop {
            let ours = |d: &Dragged| d.tree == tree && d.row != i;
            if resp
                .dnd_hover_payload::<Dragged>()
                .is_some_and(|d| ours(&d))
            {
                drop_hover = true;
            }
            if let Some(d) = resp.dnd_release_payload::<Dragged>() {
                if ours(&d) {
                    out.dropped = Some((d.row, i));
                }
            }
        }
        if !toggled {
            if resp.double_clicked() {
                out.double_clicked = Some(i);
            } else if resp.clicked() {
                out.clicked = Some(i);
            }
        }
        paint_row(ui, &p, rect, row, indent, resp.hovered(), drop_hover);
        if row.reveal {
            resp.scroll_to_me(None);
        }
        if resp.secondary_clicked() {
            out.context = Some(i);
        }
        resp.context_menu(|ui| context_menu(ui, i));
    }
    ui.spacing_mut().item_spacing.y = spacing;
    out
}

fn paint_row(
    ui: &Ui,
    p: &Palette,
    rect: Rect,
    row: &TreeRow<'_>,
    indent: f32,
    hovered: bool,
    drop_hover: bool,
) {
    let painter = ui.painter();
    if drop_hover {
        painter.rect_filled(rect, 0.0, p.bg_drag_over);
        painter.rect_stroke(
            rect.shrink(1.0),
            theme::RADIUS,
            Stroke::new(1.0, p.accent),
            egui::StrokeKind::Inside,
        );
    } else if row.selected {
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
    if let Some(open) = row.branch {
        painter.text(
            pos2(rect.left() + indent - 8.0, cy),
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
    let icon_colour = match (row.muted, row.selected) {
        (true, _) => p.text_faint,
        (_, true) => p.accent,
        _ => p.text_muted,
    };
    painter.text(
        pos2(rect.left() + indent + 6.0, cy),
        Align2::CENTER_CENTER,
        row.icon,
        icons::font(12.0),
        icon_colour,
    );
    let left = rect.left() + indent + 18.0;
    let right = rect.right() - 8.0;
    let colour = match (row.muted, row.selected) {
        (true, _) => p.text_faint,
        (_, true) => p.text_primary,
        _ => p.text_secondary,
    };
    let font = FontId::proportional(13.0);
    let name = elide(ui, row.label, font.clone(), (right - left).max(0.0));
    let label = painter.text(pos2(left, cy), Align2::LEFT_CENTER, name, font, colour);
    if let Some(hint) = row.hint {
        let at = label.right() + 6.0;
        if at < right {
            let font = FontId::proportional(11.0);
            let hint = elide(ui, hint, font.clone(), right - at);
            painter.text(pos2(at, cy), Align2::LEFT_CENTER, hint, font, p.text_muted);
        }
    }
}

/// `text`, cut short with an ellipsis to fit `width` in `font`.
pub fn elide(ui: &Ui, text: &str, font: FontId, width: f32) -> String {
    let fits = |s: &str| {
        ui.fonts_mut(|f| f.layout_no_wrap(s.to_owned(), font.clone(), egui::Color32::WHITE))
            .size()
            .x
            <= width
    };
    if fits(text) {
        return text.to_owned();
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
    use egui_kittest::kittest::Queryable;
    use egui_kittest::Harness;

    use super::*;

    #[derive(Default)]
    struct State {
        open: bool,
        selected: Option<usize>,
        menu: Vec<usize>,
    }

    fn harness() -> Harness<'static, State> {
        Harness::builder()
            .with_size(vec2(300.0, 200.0))
            .with_context_setup(crate::fonts::install)
            .build_ui_state(
                |ui, s: &mut State| {
                    let mut rows = vec![TreeRow::new(0, "Photos")
                        .branch(s.open)
                        .selected(s.selected == Some(0))];
                    if s.open {
                        rows.push(
                            TreeRow::new(1, "Trip")
                                .hint(Some("2025"))
                                .selected(s.selected == Some(1)),
                        );
                    }
                    let out = show(ui, "test-tree", &rows, |ui, i| {
                        if ui.button("Menu Item").clicked() {
                            s.menu.push(i);
                        }
                    });
                    if out.toggled.is_some() {
                        s.open = !s.open;
                    }
                    if let Some(i) = out.clicked {
                        s.selected = Some(i);
                    }
                },
                State::default(),
            )
    }

    #[test]
    fn expands_selects_and_offers_a_menu() {
        let mut h = harness();
        h.run();
        h.get_by_label("Expand Photos").click();
        h.run();
        assert!(h.state().open);
        h.get_by_label("Trip").click();
        h.run();
        assert_eq!(h.state().selected, Some(1));
        h.get_by_label("Trip").click_secondary();
        h.run();
        h.get_by_label("Menu Item").click();
        h.run();
        assert_eq!(h.state().menu, [1]);
        h.get_by_label("Collapse Photos").click();
        h.run();
        assert!(!h.state().open);
    }
}
