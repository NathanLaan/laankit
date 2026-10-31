//! The HTML NoteLiner's preview renders (`noteliner_core::export::markdown_to_html`, comrak with
//! its options) through `from_html`: task lists, aligned tables, `<mark>` and `<kbd>`. Plus the
//! article path staying as strict as before, and checkboxes painting by state.

use egui::epaint::Shape;
use egui_kittest::Harness;
use marina_egui::theme::{ThemeId, ThemeState};
use marina_richtext::{
    from_article_html, from_html, Align, Block, Document, Options, RichStyle, RichView, Span,
    WebResolver,
};

const NOTE: &str = "\
- [ ] todo
- [x] done
  1. [ ] nested

| L | C | R | N |
|:--|:-:|--:|---|
| a | b | c | d |

<mark>hi</mark> and <kbd>Ctrl</kbd>+<kbd>C</kbd>
";

fn preview(md: &str) -> Document {
    from_html(
        &noteliner_core::export::markdown_to_html(md),
        &Options::default(),
    )
}

/// The first text block's spans of each list item.
fn item_spans(items: &[Vec<Block>]) -> Vec<Vec<Span>> {
    items
        .iter()
        .map(|item| match item.first() {
            Some(Block::Text { spans, .. }) => spans.clone(),
            other => panic!("item starts with {other:?}"),
        })
        .collect()
}

#[test]
fn task_items_keep_their_checkbox_state() {
    let doc = preview(NOTE);
    let Block::List { items, .. } = &doc.blocks[0] else {
        panic!("{:?}", doc.blocks[0]);
    };
    let spans = item_spans(items);
    assert_eq!(spans[0][0].text, "[ ]");
    assert_eq!(spans[0][0].style.checkbox, Some(false));
    assert_eq!(spans[0][1].text, " todo");
    assert_eq!(spans[0][1].style.checkbox, None);
    assert_eq!(spans[1][0].text, "[x]");
    assert_eq!(spans[1][0].style.checkbox, Some(true));
    // The nested ordered list's task, after "done".
    let Some(Block::List { ordered, items, .. }) = items[1].get(1) else {
        panic!("{:?}", items[1]);
    };
    assert!(*ordered);
    assert_eq!(item_spans(items)[0][0].style.checkbox, Some(false));
    // Copying gives Markdown task syntax.
    assert!(doc.blocks[0].part_text().starts_with("[ ] todo\n[x] done"));
}

#[test]
fn checkboxes_only_start_list_items() {
    let doc = from_html(
        "<ul><li>text <input type=\"checkbox\" checked> more</li></ul>\
         <p><input type=\"checkbox\"> loose</p><ul><li><input type=\"text\"> field</li></ul>",
        &Options::default(),
    );
    let any_box = format!("{doc:?}").contains("checkbox: Some");
    assert!(!any_box, "{doc:#?}");
}

#[test]
fn articles_stay_strict() {
    // The sanitizer's allow-list (DOMPurify's, as in ThreadLiner) has no input, mark or kbd:
    // their text stays, unstyled.
    let doc = from_article_html(
        "<ul><li><input type=\"checkbox\" checked> x</li></ul><p><mark>m</mark> <kbd>k</kbd></p>",
        None,
    );
    let text = format!("{doc:?}");
    assert!(!text.contains("checkbox: Some"), "{doc:#?}");
    assert!(!text.contains("mark: true") && !text.contains("kbd: true"));
    assert_eq!(doc.plain_text(), "x m k");
}

#[test]
fn table_cells_keep_their_alignment() {
    let doc = preview(NOTE);
    let Some(Block::Table { rows, header_rows }) =
        doc.blocks.iter().find(|b| matches!(b, Block::Table { .. }))
    else {
        panic!("no table in {doc:#?}");
    };
    assert_eq!(*header_rows, 1);
    let aligns = |row: &Vec<Vec<Span>>| -> Vec<Option<Align>> {
        row.iter().map(|c| c[0].style.cell_align).collect()
    };
    let expected = vec![
        Some(Align::Left),
        Some(Align::Center),
        Some(Align::Right),
        None,
    ];
    assert_eq!(aligns(&rows[0]), expected);
    assert_eq!(aligns(&rows[1]), expected);
}

#[test]
fn body_cells_follow_their_header_and_css_wins() {
    let doc = from_html(
        "<table><tr><th align=\"right\">A</th><th>B</th></tr>\
         <tr><td>1</td><td align=\"left\" style=\"text-align: center\">2</td></tr></table>",
        &Options::default(),
    );
    let Block::Table { rows, .. } = &doc.blocks[0] else {
        panic!("{doc:#?}");
    };
    assert_eq!(rows[1][0][0].style.cell_align, Some(Align::Right));
    assert_eq!(rows[1][1][0].style.cell_align, Some(Align::Center));
}

#[test]
fn mark_and_kbd() {
    let doc = preview(NOTE);
    let Some(Block::Text { spans, .. }) = doc.blocks.last() else {
        panic!("{doc:#?}");
    };
    let style = |t: &str| spans.iter().find(|s| s.text == t).expect(t).style;
    assert!(style("hi").mark);
    assert!(style("Ctrl").kbd && style("Ctrl").code);
    assert!(style("C").kbd);
    assert!(!style(" and ").mark);
}

/// The shapes a frame painted, flattened.
fn shapes(h: &Harness<'_, (Document, RichView, bool)>) -> Vec<Shape> {
    fn flatten(shape: &Shape, out: &mut Vec<Shape>) {
        match shape {
            Shape::Vec(v) => v.iter().for_each(|s| flatten(s, out)),
            s => out.push(s.clone()),
        }
    }
    let mut out = Vec::new();
    for clipped in &h.output().shapes {
        flatten(&clipped.shape, &mut out);
    }
    out
}

fn render(md: &str, theme: ThemeId) -> (Vec<Shape>, RichStyle) {
    let mut h = Harness::builder()
        .with_size(egui::vec2(600.0, 300.0))
        .build_ui_state(
            move |ui, (doc, view, ready): &mut (Document, RichView, bool)| {
                if !*ready {
                    marina_egui::fonts::install(ui.ctx());
                    ThemeState {
                        theme,
                        ..Default::default()
                    }
                    .apply(ui.ctx());
                    *ready = true;
                    return;
                }
                let style = RichStyle::from_ctx(ui.ctx());
                view.show(ui, doc, &style, &mut WebResolver);
            },
            (preview(md), RichView::default(), false),
        );
    h.run();
    let style = RichStyle::from_ctx(&h.ctx);
    (shapes(&h), style)
}

#[test]
fn checked_and_unchecked_boxes_paint_differently() {
    for theme in [ThemeId::Light, ThemeId::Midnight] {
        let filled = |shapes: &[Shape], fill: egui::Color32| {
            shapes
                .iter()
                .filter(|s| matches!(s, Shape::Rect(r) if r.fill == fill))
                .count()
        };
        let (checked, style) = render("- [x] done\n", theme);
        let (unchecked, _) = render("- [ ] todo\n", theme);
        assert_eq!(filled(&checked, style.check_fill), 1, "{theme:?}");
        assert_eq!(filled(&unchecked, style.check_fill), 0, "{theme:?}");
        // The unchecked box is an outline in the border colour.
        let outlined = unchecked.iter().any(
            |s| matches!(s, Shape::Rect(r) if r.stroke.color == style.check_border && r.fill == egui::Color32::TRANSPARENT),
        );
        assert!(outlined, "{theme:?}");
    }
}
