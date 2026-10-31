//! The sidebar (`Sidebar.svelte`): a `marina_egui::panes` stack of the
//! FILES, OUTLINE, SLIDES, TAGS, SEARCH and BACKLINKS panes, in the user's
//! order and heights (saved in the project's layout).
//!
//! | Pane      | Drawn by                                                          |
//! |-----------|-------------------------------------------------------------------|
//! | FILES     | `views::tree`, with the sort select and the tag filter            |
//! | OUTLINE   | [`outline`] here                                                  |
//! | SLIDES    | `noteliner_slides::SlidesPane` (decks only)                       |
//! | TAGS      | `views::panes::tags_pane`                                         |
//! | SEARCH    | `views::panes::search_pane` (over `Backend::search`)              |
//! | BACKLINKS | `views::panes::backlinks_pane` (over `Backend::backlinks`)        |

use std::collections::HashSet;

use egui::{pos2, vec2, Align2, FontId, Rect, RichText, Sense, Ui};
use marina_egui::icons;
use marina_egui::panes::{self, Pane, PaneLayout};
use marina_egui::theme::{self, Palette};

use noteliner_slides::{Deck, ImageSource, SlidesPane};

use crate::config::Layout;
use crate::model::{Entry, SortMode};
use crate::views::panes::{self as pane_views, PaneAction, PaneStates, TagProps};
use crate::views::tree::{self, TreeAction, TreeState};

/// What the user did in the sidebar.
#[derive(Clone, Debug, PartialEq)]
pub enum SidebarAction {
    Tree(TreeAction),
    Sort(SortMode),
    /// An outline heading: move the editor to this line.
    GoToLine(usize),
    ClosePane(String),
    /// The pane order or heights changed: save the layout.
    LayoutChanged,
    /// The SEARCH, BACKLINKS or TAGS panes or the tag filter.
    Pane(PaneAction),
    /// The SLIDES pane: jump, edit, or open the presentation settings.
    Slides(noteliner_slides::PaneAction),
}

/// What the sidebar draws from.
pub struct SidebarProps<'a> {
    /// `noteliner.json` (tag palette and colours).
    pub index: &'a serde_json::Value,
    pub entries: &'a [Entry],
    pub sort: SortMode,
    pub hidden_tags: &'a HashSet<String>,
    pub selected: Option<&'a str>,
    /// The open note's text (for the outline) and caret line.
    pub text: &'a str,
    pub cursor_line: usize,
    pub is_deck: bool,
    /// The open deck, the slide holding the caret, and where its images come from.
    pub deck: Option<&'a Deck>,
    pub active_slide: Option<usize>,
    pub images: &'a dyn ImageSource,
}

const TITLES: [(&str, &str); 6] = [
    ("files", "FILES"),
    ("outline", "OUTLINE"),
    ("slides", "SLIDES"),
    ("tags", "TAGS"),
    ("search", "SEARCH"),
    ("backlinks", "BACKLINKS"),
];

/// Draws the visible panes in `layout`'s order.
pub fn show(
    ui: &mut Ui,
    layout: &mut Layout,
    tree: &mut TreeState,
    states: &mut PaneStates,
    slides: &mut SlidesPane,
    props: &SidebarProps<'_>,
) -> Vec<SidebarAction> {
    let mut out = Vec::new();
    let mut pane_out = Vec::new();
    let mut header_out = Vec::new();
    let all_tags = crate::model::all_tags(props.index);
    // Captured before the tree runs: it forgets the drag on release, the
    // frame a TAGS chip needs it.
    let dragging = tree.dragging().map(str::to_string);
    let tag_props = TagProps {
        index: props.index,
        entries: props.entries,
        all_tags: &all_tags,
        selected: props.selected,
        dragging: dragging.as_deref(),
    };
    let visible: Vec<Pane<'_>> = TITLES
        .iter()
        .filter(|(id, _)| {
            // The SLIDES pane only exists for a deck (`slidesPaneVisible`).
            layout.pane_visible(id) && (*id != "slides" || props.is_deck)
        })
        .map(|(id, title)| Pane::new(id, title).min_height(60.0))
        .collect();
    if visible.is_empty() {
        return out;
    }
    let mut pane_layout = PaneLayout {
        order: layout.pane_order.clone(),
        heights: crate::config::PANE_KEYS
            .iter()
            .map(|k| (k.to_string(), layout.pane_height(k)))
            .collect(),
    };
    let sort = props.sort;
    let mut sort_pick = None;
    let result = panes::show(
        ui,
        ui.scope_id().with("noteliner-sidebar"),
        &mut pane_layout,
        &visible,
        |ui, id| match id {
            "files" => {
                for a in tree::show(
                    ui,
                    tree,
                    props.entries,
                    sort,
                    props.hidden_tags,
                    props.selected,
                ) {
                    out.push(SidebarAction::Tree(a));
                }
            }
            "outline" => {
                if let Some(line) = outline(ui, props.text, props.cursor_line) {
                    out.push(SidebarAction::GoToLine(line));
                }
            }
            "slides" => {
                let actions =
                    slides.show(ui, props.text, props.deck, props.active_slide, props.images);
                out.extend(actions.into_iter().map(SidebarAction::Slides));
            }
            "tags" => pane_views::tags_pane(ui, &mut states.tags, &tag_props, &mut pane_out),
            "search" => pane_views::search_pane(ui, &mut states.search, &mut pane_out),
            "backlinks" => pane_views::backlinks_pane(
                ui,
                &states.backlinks,
                props.selected.is_some(),
                &mut pane_out,
            ),
            _ => {}
        },
        |ui, id| match id {
            "files" => {
                pane_views::filter_button(
                    ui,
                    &mut states.filter_open,
                    props.hidden_tags,
                    &tag_props,
                    &mut header_out,
                );
                if let Some(mode) = sort_select(ui, sort) {
                    sort_pick = Some(mode);
                }
            }
            "tags" => pane_views::tags_header(
                ui,
                !all_tags.is_empty(),
                props.selected.is_some(),
                &mut header_out,
            ),
            _ => {}
        },
    );
    for a in header_out {
        match a {
            // The input lives in the TAGS pane's own state.
            PaneAction::StartAddTag => states.tags.start_add(props.selected),
            a => pane_out.push(a),
        }
    }
    out.extend(pane_out.into_iter().map(SidebarAction::Pane));
    if let Some(mode) = sort_pick {
        out.push(SidebarAction::Sort(mode));
    }
    if let Some(closed) = result.closed {
        out.push(SidebarAction::ClosePane(closed));
    }
    if result.layout_changed || pane_layout.order != layout.pane_order {
        layout.pane_order = crate::config::normalize_pane_order(&pane_layout.order);
        for (k, h) in &pane_layout.heights {
            if (layout.pane_height(k) - h).abs() > 0.5 {
                layout.set_pane_height(k, *h);
            }
        }
        out.push(SidebarAction::LayoutChanged);
    }
    out
}

/// The FILES header's sort select (`pane-header-select`): a compact
/// drop-down in the pane header.
fn sort_select(ui: &mut Ui, current: SortMode) -> Option<SortMode> {
    let p = theme::palette(ui.ctx());
    let mut picked = None;
    let label = RichText::new(format!("{}  {}", current.label(), icons::CHEVRON_DOWN))
        .size(11.0)
        .color(p.text_muted);
    let resp =
        egui::containers::menu::MenuButton::from_button(egui::Button::new(label).frame(false))
            .ui(ui, |ui| {
                for mode in SortMode::ALL {
                    if ui
                        .selectable_label(mode == current, RichText::new(mode.label()).size(12.0))
                        .clicked()
                    {
                        picked = Some(mode);
                        ui.close();
                    }
                }
            })
            .0;
    resp.on_hover_text("Sort files")
        .widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Sort files"));
    picked.filter(|m| *m != current)
}

/// `OutlinePane.svelte`: `#`–`####` headings; the one the caret is under is
/// highlighted. Returns the line of a clicked heading.
fn outline(ui: &mut Ui, text: &str, cursor_line: usize) -> Option<usize> {
    let p = theme::palette(ui.ctx());
    let headings = headings(text);
    let mut clicked = None;
    if headings.is_empty() {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            ui.label(RichText::new("No headings").size(13.0).color(p.text_muted));
        });
        return None;
    }
    let active = headings
        .iter()
        .filter(|h| h.line <= cursor_line)
        .map(|h| h.line)
        .next_back();
    egui::ScrollArea::vertical()
        .id_salt("noteliner-outline")
        .auto_shrink(false)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for h in &headings {
                let (rect, resp) =
                    ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Link, true, &h.text));
                outline_row(ui, &p, rect, h, active == Some(h.line), resp.hovered());
                if resp.clicked() {
                    clicked = Some(h.line);
                }
            }
        });
    clicked
}

fn outline_row(ui: &Ui, p: &Palette, rect: Rect, h: &Heading, active: bool, hovered: bool) {
    if active {
        ui.painter().rect_filled(rect, 0.0, p.bg_selected);
    } else if hovered {
        ui.painter().rect_filled(rect, 0.0, p.bg_item_hover);
    }
    let left = rect.left() + 8.0 + (h.level - 1) as f32 * 14.0;
    let font = FontId::proportional(if h.level == 1 { 13.0 } else { 12.5 });
    let text = tree::elide(ui, &h.text, font.clone(), rect.right() - left - 6.0);
    ui.painter().text(
        pos2(left, rect.center().y),
        Align2::LEFT_CENTER,
        text,
        font,
        if active {
            p.accent
        } else if h.level == 1 {
            p.text_primary
        } else {
            p.text_secondary
        },
    );
}

/// An outline heading.
#[derive(Clone, Debug, PartialEq)]
pub struct Heading {
    pub level: usize,
    pub text: String,
    /// 1-based.
    pub line: usize,
}

/// `extractHeadings`: `/^(#{1,4})\s+(.+)/` per line.
pub fn headings(text: &str) -> Vec<Heading> {
    text.split('\n')
        .enumerate()
        .filter_map(|(i, line)| {
            let hashes = line.chars().take_while(|c| *c == '#').count();
            if !(1..=4).contains(&hashes) {
                return None;
            }
            let rest = &line[hashes..];
            if !rest.starts_with(char::is_whitespace) || rest.trim().is_empty() {
                return None;
            }
            Some(Heading {
                level: hashes,
                text: rest.trim().to_string(),
                line: i + 1,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_like_the_svelte_regex() {
        let h = headings("# One\ntext\n## Two  \n#####Five\n#NoSpace\n####   Four\n");
        let got: Vec<(usize, &str, usize)> = h
            .iter()
            .map(|h| (h.level, h.text.as_str(), h.line))
            .collect();
        assert_eq!(got, [(1, "One", 1), (2, "Two", 3), (4, "Four", 6)]);
    }
}
