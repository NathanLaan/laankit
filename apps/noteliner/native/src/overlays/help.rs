//! Help (`HelpApp.svelte`). The Svelte builds open it in a second window;
//! here it's a full-size dialog with the same pages: an index grouped by
//! section with a search box (non-matching pages are greyed out), and the
//! page's Markdown rendered by `marina-richtext`.
//!
//! The pages are `assets/help-pages.json`, generated from the Svelte build's
//! `lib/helpContent.js` by `scripts/gen-help.mjs`, so both builds stay in
//! step (a test checks the file is current when Node is available).

use std::collections::HashMap;
use std::sync::LazyLock;

use egui::{pos2, vec2, Align2, FontId, RichText, Sense};
use marina_egui::dialog::{self, Dialog, Size};
use marina_egui::theme;
use marina_richtext::{Document, RichStyle, RichView, WebResolver};
use serde::Deserialize;

/// One help page.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct HelpPage {
    pub id: String,
    pub title: String,
    pub section: String,
    pub body: String,
}

/// The pages, in the Svelte order.
pub static PAGES: LazyLock<Vec<HelpPage>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../assets/help-pages.json")).unwrap_or_default()
});

#[derive(Default)]
pub struct HelpState {
    pub query: String,
    pub selected: Option<String>,
    docs: HashMap<String, Document>,
    view: RichView,
}

/// The pages grouped by section, sections in order of first appearance
/// (`grouped` in `HelpApp.svelte`).
pub fn grouped() -> Vec<&'static HelpPage> {
    let mut sections: Vec<&str> = Vec::new();
    for p in PAGES.iter() {
        if !sections.contains(&p.section.as_str()) {
            sections.push(&p.section);
        }
    }
    sections
        .iter()
        .flat_map(|s| PAGES.iter().filter(move |p| p.section == *s))
        .collect()
}

/// `pageMatches`: title, body or section contains the query.
pub fn page_matches(page: &HelpPage, query: &str) -> bool {
    let q = query.trim().to_lowercase();
    q.is_empty()
        || page.title.to_lowercase().contains(&q)
        || page.body.to_lowercase().contains(&q)
        || page.section.to_lowercase().contains(&q)
}

/// Draws Help; returns `true` when it should close.
pub fn show(ctx: &egui::Context, state: &mut HelpState, top_inset: f32) -> bool {
    let p = theme::palette(ctx);
    let resp = Dialog::full("help", "NoteLiner Help")
        .size(Size::Full { top_inset })
        .show(ctx, |ui| {
            let full = ui.available_rect_before_wrap();
            let left_w = (full.width() * 0.3).clamp(200.0, 340.0);
            let index_rect = egui::Rect::from_min_size(full.min, vec2(left_w, full.height()));
            let page_rect =
                egui::Rect::from_min_max(pos2(full.left() + left_w + 16.0, full.top()), full.max);
            ui.painter().vline(
                full.left() + left_w + 8.0,
                full.y_range(),
                egui::Stroke::new(1.0, p.border),
            );

            ui.scope_builder(egui::UiBuilder::new().max_rect(index_rect), |ui| {
                let r = dialog::text_input(ui, &mut state.query, "Search help…", true);
                r.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::Role::SearchInput, true, "Search help")
                });
                let q = state.query.trim();
                if !q.is_empty() {
                    let n = PAGES.iter().filter(|pg| page_matches(pg, q)).count();
                    dialog::muted_text(
                        ui,
                        &format!("{n} matching page{}", if n == 1 { "" } else { "s" }),
                    );
                }
                ui.add_space(6.0);
                egui::ScrollArea::vertical()
                    .id_salt("help-index")
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        let mut section = "";
                        for page in grouped() {
                            if page.section != section {
                                section = &page.section;
                                ui.add_space(8.0);
                                ui.label(
                                    RichText::new(section.to_uppercase())
                                        .size(11.0)
                                        .color(p.text_muted)
                                        .extra_letter_spacing(0.5),
                                );
                            }
                            let matches = page_matches(page, &state.query);
                            let selected =
                                state.selected.as_deref().unwrap_or(PAGES[0].id.as_str())
                                    == page.id;
                            let (rect, resp) = ui.allocate_exact_size(
                                vec2(ui.available_width(), 26.0),
                                Sense::click(),
                            );
                            resp.widget_info(|| {
                                egui::WidgetInfo::selected(
                                    egui::Role::Button,
                                    true,
                                    selected,
                                    &page.title,
                                )
                            });
                            if selected {
                                ui.painter().rect_filled(rect, theme::RADIUS, p.bg_selected);
                            } else if resp.hovered() {
                                ui.painter()
                                    .rect_filled(rect, theme::RADIUS, p.bg_item_hover);
                            }
                            ui.painter().text(
                                pos2(rect.left() + 8.0, rect.center().y),
                                Align2::LEFT_CENTER,
                                &page.title,
                                FontId::proportional(13.0),
                                if !matches {
                                    p.text_faint
                                } else if selected {
                                    p.accent
                                } else {
                                    p.text_primary
                                },
                            );
                            if resp.clicked() {
                                state.selected = Some(page.id.clone());
                                state.view.reset();
                            }
                        }
                    });
            });

            ui.scope_builder(egui::UiBuilder::new().max_rect(page_rect), |ui| {
                let Some(page) = PAGES
                    .iter()
                    .find(|pg| Some(&pg.id) == state.selected.as_ref())
                    .or_else(|| PAGES.first())
                else {
                    return;
                };
                let doc = state
                    .docs
                    .entry(page.id.clone())
                    .or_insert_with(|| crate::views::preview::document(&page.body));
                let style = RichStyle::from_ctx(ui.ctx());
                let shown = state.view.show(ui, doc, &style, &mut WebResolver);
                for a in shown.actions {
                    let marina_richtext::Action::Open(url) = a;
                    ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                }
            });
        });
    resp.close
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_load_and_search() {
        assert!(PAGES.len() > 20, "help-pages.json is missing pages");
        assert_eq!(PAGES[0].id, "overview");
        assert!(PAGES.iter().any(|pg| page_matches(pg, "wikilinks")));
        assert!(!page_matches(&PAGES[0], "zzzz-no-such-text"));
        // Each section heads one group.
        let g = grouped();
        assert_eq!(g.len(), PAGES.len());
        let mut seen: Vec<&str> = Vec::new();
        for w in g.windows(2) {
            if w[0].section != w[1].section {
                assert!(
                    !seen.contains(&w[1].section.as_str()),
                    "{} split",
                    w[1].section
                );
                seen.push(&w[0].section);
            }
        }
    }
}
