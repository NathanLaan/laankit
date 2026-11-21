//! Help: a floating window (not a modal dialog) with an index grouped by section, a search box
//! (non-matching pages are greyed out), and the page's Markdown rendered by
//! `laankit-richtext`.
//!
//! The pages are `assets/help-pages.json`; edit that file to change them.

use std::collections::HashMap;
use std::sync::LazyLock;

use egui::{pos2, vec2, Align2, FontId, RichText, Sense};
use laankit_egui::dialog;
use laankit_egui::theme;
use laankit_richtext::{Document, RichStyle, RichView, WebResolver};
use serde::Deserialize;

/// One help page.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct HelpPage {
    pub id: String,
    pub title: String,
    pub section: String,
    pub body: String,
}

/// The pages, in the file's order.
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

/// Draws Help as a window that floats over the app: it can be moved and
/// resized, and the app behind it stays in use. Returns `true` when it
/// should close.
pub fn show(ctx: &egui::Context, state: &mut HelpState) -> bool {
    let p = theme::palette(ctx);
    let screen = ctx.content_rect();
    let size = vec2(
        (screen.width() - 120.0).clamp(360.0, 860.0),
        (screen.height() - 120.0).clamp(280.0, 640.0),
    );
    let mut open = true;
    egui::Window::new("NoteLiner Help")
        .id(egui::Id::unique("noteliner-help"))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_size(size)
        .min_size(vec2(360.0, 240.0))
        .default_pos(screen.right_top() + vec2(-size.x - 24.0, 48.0))
        .show(ctx, |ui| {
            // The index and the page share whatever size the window has.
            ui.set_min_size(ui.available_size());
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
                                    RichText::new(laankit_egui::panes::heading(ui.ctx(), section))
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
                                ui.painter().rect_filled(
                                    rect,
                                    laankit_egui::theme::shape(ui.ctx()).control(),
                                    p.bg_selected,
                                );
                            } else if resp.hovered() {
                                ui.painter().rect_filled(
                                    rect,
                                    laankit_egui::theme::shape(ui.ctx()).control(),
                                    p.bg_item_hover,
                                );
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
                    let laankit_richtext::Action::Open(url) = a;
                    ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                }
            });
        });
    !open
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_load_and_search() {
        assert!(PAGES.len() > 20, "help-pages.json is missing pages");
        assert_eq!(PAGES[0].id, "overview");
        // The page on how a presentation is written, by what is on it.
        let formatting = PAGES.iter().find(|pg| pg.id == "presentation-formatting");
        let body = &formatting.expect("the page").body;
        for needle in ["<!-- notes", "layout=", "bg=", "{{page}}", "slideLevel"] {
            assert!(body.contains(needle), "{needle} is not on the page");
        }
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
