//! A list of a diagram's objects, in three groups: shapes, text, and lines.
//! A click on a row selects the object on the canvas, and the canvas's
//! selection marks the rows.
//!
//! The rows are built from the diagram each frame ([`objects`]), so the list
//! cannot disagree with it. [`ObjectsState`] keeps only which groups are
//! folded and where a Shift+click starts from.

use blockliner_core::model::{
    Connector, ConnectorKind, Diagram, Point, Rect as ModelRect, ShapeKind,
};
use egui::{pos2, vec2, Align2, FontId, Rect, Sense, Ui};
use laankit_egui::icons;
use laankit_egui::theme::{self, Palette};

use crate::canvas::Tool;
use crate::glyphs;

const ROW_HEIGHT: f32 = 24.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Shapes,
    Text,
    Lines,
}

impl Group {
    pub const ALL: [Group; 3] = [Group::Shapes, Group::Text, Group::Lines];

    pub fn name(self) -> &'static str {
        match self {
            Group::Shapes => "Shapes",
            Group::Text => "Text",
            Group::Lines => "Lines",
        }
    }
}

/// What a row selects: a shape by its id, or a line by its place in
/// `Diagram::connectors`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Shape(String),
    Line(usize),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ObjectRow {
    pub group: Group,
    pub target: Target,
    /// The first line of a shape's label, or "Start to End" for a line.
    pub name: String,
    /// The name is the object's own text, not its kind standing in for it.
    pub named: bool,
    /// A line's label.
    pub detail: Option<String>,
    /// The tool whose icon stands for it.
    pub icon: Tool,
    /// A shape declared in a statement kept as written: it can't be edited.
    pub external: bool,
    /// Where it is, to bring it into view.
    pub bounds: ModelRect,
}

/// The first line of `label` with something on it.
fn first_line(label: &str) -> Option<&str> {
    label.lines().map(str::trim).find(|l| !l.is_empty())
}

/// What a line's end is on: the shape's label, or its kind.
fn end_name(d: &Diagram, shape: Option<&str>) -> Option<String> {
    let s = d.shape(shape?)?;
    Some(first_line(&s.label).map_or_else(|| s.kind.name().to_string(), str::to_string))
}

fn line_name(d: &Diagram, c: &Connector) -> (String, bool) {
    match (end_name(d, c.from.shape()), end_name(d, c.to.shape())) {
        (Some(a), Some(b)) => (format!("{a} to {b}"), true),
        (Some(a), None) => (format!("From {a}"), true),
        (None, Some(b)) => (format!("To {b}"), true),
        (None, None) => (
            match c.kind {
                ConnectorKind::Line => "Line",
                ConnectorKind::Arrow => "Arrow",
            }
            .to_string(),
            false,
        ),
    }
}

fn line_bounds(d: &Diagram, c: &Connector) -> ModelRect {
    let path = d.connector_path(c).unwrap_or_default();
    let Some(first) = path.first() else {
        return ModelRect::new(0.0, 0.0, 0.0, 0.0);
    };
    let (mut lo, mut hi) = (*first, *first);
    for Point { x, y } in &path {
        lo = Point::new(lo.x.min(*x), lo.y.min(*y));
        hi = Point::new(hi.x.max(*x), hi.y.max(*y));
    }
    ModelRect::new(lo.x, lo.y, hi.x - lo.x, hi.y - lo.y)
}

/// Every object of `d`: the shapes and then the text objects, each from the
/// front to the back, and then the lines in the order they were made.
pub fn objects(d: &Diagram) -> Vec<ObjectRow> {
    let shape_rows = |text: bool| {
        d.shapes
            .iter()
            .rev()
            .filter(move |s| (s.kind == ShapeKind::Text) == text)
            .map(move |s| {
                let name = first_line(&s.label);
                ObjectRow {
                    group: if text { Group::Text } else { Group::Shapes },
                    target: Target::Shape(s.id.clone()),
                    name: name.unwrap_or(s.kind.name()).to_string(),
                    named: name.is_some(),
                    detail: None,
                    icon: Tool::for_shape(&s.kind).unwrap_or(Tool::Rect),
                    external: s.external,
                    bounds: s.rect,
                }
            })
    };
    let lines = d.connectors.iter().enumerate().map(|(i, c)| {
        let (name, named) = line_name(d, c);
        ObjectRow {
            group: Group::Lines,
            target: Target::Line(i),
            name,
            named,
            detail: c.label.as_deref().and_then(first_line).map(str::to_string),
            icon: match (c.kind, c.route.is_elbow()) {
                (ConnectorKind::Line, false) => Tool::Line,
                (ConnectorKind::Line, true) => Tool::ElbowLine,
                (ConnectorKind::Arrow, false) => Tool::Arrow,
                (ConnectorKind::Arrow, true) => Tool::ElbowArrow,
            },
            external: false,
            bounds: line_bounds(d, c),
        }
    });
    shape_rows(false)
        .chain(shape_rows(true))
        .chain(lines)
        .collect()
}

/// The canvas's selection, as the list sets it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Selection {
    pub shapes: Vec<String>,
    pub lines: Vec<usize>,
    /// The object clicked, to bring into view when it is off screen.
    pub reveal: Option<ModelRect>,
}

#[derive(Default)]
pub struct ObjectsState {
    folded: [bool; 3],
    /// The last row clicked: where a Shift+click selects from.
    anchor: Option<Target>,
    /// The selection as of last frame, to tell when the canvas changed it.
    seen: (Vec<String>, Vec<usize>),
}

enum Item<'a> {
    Header(Group, usize),
    Row(&'a ObjectRow),
}

fn is_selected(target: &Target, shapes: &[String], lines: &[usize]) -> bool {
    match target {
        Target::Shape(id) => shapes.contains(id),
        Target::Line(i) => lines.contains(i),
    }
}

/// The selection after a click on `row`. `toggle` is Ctrl, and `range` is
/// Shift: from `anchor` to `row`, among the rows of `row`'s group.
fn clicked(
    rows: &[ObjectRow],
    row: &ObjectRow,
    anchor: Option<&Target>,
    shapes: &[String],
    lines: &[usize],
    toggle: bool,
    range: bool,
) -> Selection {
    let mut sel = Selection {
        reveal: Some(row.bounds),
        ..Selection::default()
    };
    let add = |sel: &mut Selection, t: &Target| match t {
        Target::Shape(id) if !sel.shapes.contains(id) => sel.shapes.push(id.clone()),
        Target::Line(i) if !sel.lines.contains(i) => sel.lines.push(*i),
        _ => {}
    };
    let group: Vec<&ObjectRow> = rows.iter().filter(|r| r.group == row.group).collect();
    let place = |t: &Target| group.iter().position(|r| r.target == *t);
    if let (true, Some(a), Some(b)) = (range, anchor.and_then(place), place(&row.target)) {
        for r in &group[a.min(b)..=a.max(b)] {
            add(&mut sel, &r.target);
        }
    } else if toggle {
        sel.shapes = shapes.to_vec();
        sel.lines = lines.to_vec();
        if is_selected(&row.target, shapes, lines) {
            match &row.target {
                Target::Shape(id) => sel.shapes.retain(|s| s != id),
                Target::Line(i) => sel.lines.retain(|l| l != i),
            }
            sel.reveal = None;
        } else {
            add(&mut sel, &row.target);
        }
    } else {
        add(&mut sel, &row.target);
    }
    sel
}

/// Draws the list in the rest of `ui`. `shapes` and `lines` are the canvas's
/// selection. Returns the selection a click asks for.
pub fn show(
    ui: &mut Ui,
    state: &mut ObjectsState,
    rows: &[ObjectRow],
    shapes: &[String],
    lines: &[usize],
) -> Option<Selection> {
    let p = theme::palette(ui.ctx());
    if rows.is_empty() {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            ui.label(
                egui::RichText::new("No objects")
                    .size(13.0)
                    .color(p.text_muted),
            );
        });
        return None;
    }

    // The canvas changed the selection: show the first selected row.
    let changed = state.seen.0 != shapes || state.seen.1 != lines;
    if changed {
        state.seen = (shapes.to_vec(), lines.to_vec());
    }
    let first = changed
        .then(|| rows.iter().find(|r| is_selected(&r.target, shapes, lines)))
        .flatten();
    if let Some(row) = first {
        state.folded[row.group as usize] = false;
    }

    let mut items = Vec::with_capacity(rows.len() + 3);
    for group in Group::ALL {
        let count = rows.iter().filter(|r| r.group == group).count();
        if count == 0 {
            continue;
        }
        items.push(Item::Header(group, count));
        if !state.folded[group as usize] {
            items.extend(rows.iter().filter(|r| r.group == group).map(Item::Row));
        }
    }

    let (toggle, range) = ui.input(|i| (i.modifiers.command, i.modifiers.shift));
    let mut picked = None;
    let mut fold = None;
    egui::ScrollArea::vertical()
        .id_salt("blockliner-objects")
        .auto_shrink(false)
        .show_viewport(ui, |ui, viewport| {
            let width = ui.available_width();
            let (all, _) = ui.allocate_exact_size(
                vec2(width, items.len() as f32 * ROW_HEIGHT),
                Sense::hover(),
            );
            let rect_of = |i: usize| {
                Rect::from_min_size(
                    pos2(all.left(), all.top() + i as f32 * ROW_HEIGHT),
                    vec2(width, ROW_HEIGHT),
                )
            };
            if let Some(row) = first {
                let at = items
                    .iter()
                    .position(|it| matches!(it, Item::Row(r) if std::ptr::eq(*r, row)));
                if let Some(at) = at {
                    // `None`: scrolls only as far as it takes to show it.
                    ui.scroll_to_rect(rect_of(at), None);
                }
            }
            // Only the rows in view are drawn.
            let from = (viewport.top() / ROW_HEIGHT).floor().max(0.0) as usize;
            let to = ((viewport.bottom() / ROW_HEIGHT).ceil() as usize).min(items.len());
            for (i, item) in items.iter().enumerate().take(to).skip(from) {
                let rect = rect_of(i);
                match item {
                    Item::Header(group, count) => {
                        let folded = state.folded[*group as usize];
                        let id = ui.scope_id().with(("objects-group", *group as usize));
                        let resp = ui.interact(rect, id, Sense::click());
                        let label = format!("{} ({count})", group.name());
                        resp.widget_info(|| {
                            egui::WidgetInfo::labeled(egui::Role::Button, true, &label)
                        });
                        header(ui, &p, rect, &label, folded, resp.hovered());
                        if resp.clicked() {
                            fold = Some(*group);
                        }
                    }
                    Item::Row(row) => {
                        let id = ui.scope_id().with(("objects-row", &row.target_key()));
                        let resp = ui.interact(rect, id, Sense::click());
                        let selected = is_selected(&row.target, shapes, lines);
                        resp.widget_info(|| {
                            egui::WidgetInfo::selected(
                                egui::Role::ListItem,
                                true,
                                selected,
                                row.label(),
                            )
                        });
                        let cut = paint_row(ui, &p, rect, row, selected, resp.hovered());
                        let resp = if row.external {
                            resp.on_hover_text(format!(
                                "{}\nDeclared in a statement kept as written, so it can't be edited here.",
                                row.label()
                            ))
                        } else if cut {
                            resp.on_hover_text(row.label())
                        } else {
                            resp
                        };
                        if resp.clicked() {
                            picked = Some(*row);
                        }
                    }
                }
            }
        });

    if let Some(group) = fold {
        state.folded[group as usize] ^= true;
    }
    let row = picked?;
    let sel = clicked(
        rows,
        row,
        state.anchor.as_ref(),
        shapes,
        lines,
        toggle,
        range,
    );
    if !range || state.anchor.is_none() {
        state.anchor = Some(row.target.clone());
    }
    // This change is the list's own: nothing to scroll to.
    state.seen = (sel.shapes.clone(), sel.lines.clone());
    Some(sel)
}

impl ObjectRow {
    /// The row as a screen reader hears it, and as a test finds it.
    pub fn label(&self) -> String {
        match &self.detail {
            Some(detail) => format!("{}: {detail}", self.name),
            None => self.name.clone(),
        }
    }

    fn target_key(&self) -> (u8, String) {
        match &self.target {
            Target::Shape(id) => (0, id.clone()),
            Target::Line(i) => (1, i.to_string()),
        }
    }
}

fn header(ui: &Ui, p: &Palette, rect: Rect, label: &str, folded: bool, hovered: bool) {
    let shape = theme::shape(ui.ctx());
    if hovered {
        ui.painter()
            .rect_filled(shape.item_rect(rect), shape.item(), p.bg_item_hover);
    }
    ui.painter().text(
        pos2(rect.left() + 14.0, rect.center().y),
        Align2::CENTER_CENTER,
        if folded {
            icons::CHEVRON_RIGHT
        } else {
            icons::CHEVRON_DOWN
        },
        icons::font(9.0),
        p.text_muted,
    );
    ui.painter().text(
        pos2(rect.left() + 26.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(12.0),
        p.text_secondary,
    );
}

/// `text` cut to `width` with an ellipsis, and whether it was cut.
fn elide(ui: &Ui, text: &str, font: &FontId, width: f32) -> (String, bool) {
    let fits = |s: &str| {
        ui.painter()
            .layout_no_wrap(s.to_string(), font.clone(), egui::Color32::WHITE)
            .size()
            .x
            <= width
    };
    if fits(text) {
        return (text.to_string(), false);
    }
    let chars: Vec<char> = text.chars().collect();
    let (mut lo, mut hi) = (0, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let cut: String = chars[..mid].iter().collect::<String>() + "…";
        if fits(&cut) {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    (chars[..lo].iter().collect::<String>() + "…", true)
}

/// Draws a row. Returns whether its name was cut short.
fn paint_row(
    ui: &Ui,
    p: &Palette,
    rect: Rect,
    row: &ObjectRow,
    selected: bool,
    hovered: bool,
) -> bool {
    let shape = theme::shape(ui.ctx());
    if selected {
        ui.painter()
            .rect_filled(shape.item_rect(rect), shape.item(), p.bg_selected);
    } else if hovered {
        ui.painter()
            .rect_filled(shape.item_rect(rect), shape.item(), p.bg_item_hover);
    }
    let muted = row.external || !row.named;
    let ink = if selected {
        p.accent
    } else if muted {
        p.text_muted
    } else {
        p.text_primary
    };
    glyphs::paint_tool_icon(
        &ui.painter().with_clip_rect(rect.intersect(ui.clip_rect())),
        row.icon,
        pos2(rect.left() + 36.0, rect.center().y),
        if selected { p.accent } else { p.text_muted },
    );

    let font = FontId::proportional(13.0);
    let left = rect.left() + 54.0;
    let mut right = rect.right() - 8.0;
    if let Some(detail) = &row.detail {
        let small_font = FontId::proportional(12.0);
        let (text, _) = elide(ui, detail, &small_font, (rect.width() * 0.35).max(30.0));
        let drawn = ui.painter().text(
            pos2(right, rect.center().y),
            Align2::RIGHT_CENTER,
            text,
            small_font,
            p.text_muted,
        );
        right = drawn.left() - 8.0;
    }
    let (text, cut) = elide(ui, &row.name, &font, (right - left).max(10.0));
    ui.painter().text(
        pos2(left, rect.center().y),
        Align2::LEFT_CENTER,
        text,
        font,
        ink,
    );
    cut
}

#[cfg(test)]
mod tests {
    use super::*;
    use blockliner_core::model::{End, Route, Shape};

    fn shape(id: &str, kind: ShapeKind, label: &str, x: f32) -> Shape {
        Shape::new(id, kind, label, ModelRect::new(x, 0.0, 100.0, 50.0))
    }

    fn sample() -> Diagram {
        let mut d = Diagram::new();
        d.shapes
            .push(shape("a", ShapeKind::Rect, "Start\nhere", 0.0));
        d.shapes.push(shape("t", ShapeKind::Text, "Draft", 200.0));
        d.shapes.push(shape("b", ShapeKind::Diamond, "", 400.0));
        d.shapes
            .push(shape("n", ShapeKind::Note, "Remember", 600.0));
        let mut edge = Connector::new(
            ConnectorKind::Arrow,
            End::glued("a", None),
            End::glued("b", None),
        );
        edge.label = Some("yes".into());
        d.connectors.push(edge);
        d.connectors.push(Connector::new(
            ConnectorKind::Line,
            End::Free(Point::new(0.0, 300.0)),
            End::Free(Point::new(50.0, 340.0)),
        ));
        d.connectors.push(
            Connector::new(
                ConnectorKind::Arrow,
                End::Free(Point::new(0.0, 400.0)),
                End::glued("a", None),
            )
            .with_route(Route::ELBOW),
        );
        d
    }

    fn names(rows: &[ObjectRow], group: Group) -> Vec<&str> {
        rows.iter()
            .filter(|r| r.group == group)
            .map(|r| r.name.as_str())
            .collect()
    }

    #[test]
    fn three_groups_with_shapes_from_the_front() {
        let rows = objects(&sample());
        // A note is a shape; the last shape made is in front, so first.
        assert_eq!(
            names(&rows, Group::Shapes),
            ["Remember", "Diamond", "Start"]
        );
        assert_eq!(names(&rows, Group::Text), ["Draft"]);
        assert_eq!(
            names(&rows, Group::Lines),
            ["Start to Diamond", "Line", "To Start"]
        );
    }

    #[test]
    fn a_shape_with_no_label_goes_by_its_kind() {
        let rows = objects(&sample());
        let diamond = rows.iter().find(|r| r.name == "Diamond").unwrap();
        assert!(!diamond.named);
        assert_eq!(diamond.icon, Tool::Diamond);
        assert!(rows.iter().find(|r| r.name == "Start").unwrap().named);
    }

    #[test]
    fn a_line_has_its_label_its_icon_and_its_bounds() {
        let rows = objects(&sample());
        let lines: Vec<_> = rows.iter().filter(|r| r.group == Group::Lines).collect();
        assert_eq!(lines[0].label(), "Start to Diamond: yes");
        assert_eq!(lines[0].icon, Tool::Arrow);
        assert_eq!(lines[1].icon, Tool::Line);
        assert!(!lines[1].named);
        assert_eq!(lines[1].bounds, ModelRect::new(0.0, 300.0, 50.0, 40.0));
        assert_eq!(lines[2].icon, Tool::ElbowArrow);
        assert_eq!(lines[2].target, Target::Line(2));
    }

    #[test]
    fn a_shape_that_cannot_be_edited_says_so() {
        let mut d = sample();
        d.shapes[0].external = true;
        let rows = objects(&d);
        assert!(rows.iter().find(|r| r.name == "Start").unwrap().external);
    }

    #[test]
    fn a_click_selects_one_and_ctrl_adds_or_removes() {
        let rows = objects(&sample());
        let start = rows.iter().find(|r| r.name == "Start").unwrap();
        let line = rows.iter().find(|r| r.name == "Line").unwrap();

        let sel = clicked(&rows, start, None, &["n".into()], &[0], false, false);
        assert_eq!(sel.shapes, ["a"]);
        assert!(sel.lines.is_empty());
        assert_eq!(sel.reveal, Some(start.bounds));

        let sel = clicked(&rows, line, None, &["a".into()], &[], true, false);
        assert_eq!(
            (sel.shapes.as_slice(), sel.lines.as_slice()),
            (&["a".to_string()][..], &[1][..])
        );

        let sel = clicked(&rows, start, None, &["a".into()], &[1], true, false);
        assert!(sel.shapes.is_empty());
        assert_eq!(sel.lines, [1]);
        assert_eq!(sel.reveal, None);
    }

    #[test]
    fn shift_selects_a_run_within_the_group() {
        let rows = objects(&sample());
        let top = rows.iter().find(|r| r.name == "Remember").unwrap();
        let start = rows.iter().find(|r| r.name == "Start").unwrap();
        let sel = clicked(&rows, start, Some(&top.target), &[], &[], false, true);
        assert_eq!(sel.shapes, ["n", "b", "a"]);

        // An anchor in another group: the click selects the row alone.
        let line = rows.iter().find(|r| r.name == "Line").unwrap();
        let sel = clicked(&rows, start, Some(&line.target), &[], &[], false, true);
        assert_eq!(sel.shapes, ["a"]);
    }
}
