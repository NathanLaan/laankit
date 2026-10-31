//! The Preview pane (`Preview.svelte`): the note's Markdown rendered with
//! `noteliner_core::export`'s comrak path (the same GFM options as the HTML
//! export) and shown natively by `marina-richtext`.
//!
//! - `[[wikilinks]]` become `wikilink:` links; clicking one opens the note,
//!   or offers to create it when there's no such note (`onCreateFromWikilink`).
//! - `_attachments/…` images are read from the project's `_attachments`
//!   folder ([`AttachmentResolver`]), as the `attachment://` protocol serves
//!   them to the webview builds.
//! - A deck renders as slides (`DeckPreview.svelte`), through
//!   `noteliner_slides::DeckPreview`; clicking a slide jumps the editor to it.
//! - `$…$` and `$$…$$` math is typeset (Typst, via `crate::math`); the
//!   webview builds showed it as text.

use std::path::PathBuf;
use std::sync::LazyLock;

use std::collections::HashMap;

use egui::{Frame, Margin, RichText, Ui};
use marina_egui::{icons, theme};
use marina_richtext::{
    from_note_html, Action, Document, LinkAction, Options, Resolver, RichStyle, RichView,
};
use noteliner_slides::{Deck, DeckPreview, ImageSource};
use regex::Regex;

/// Rebuild the document at most this often while typing.
const REBUILD_SECS: f64 = 0.15;
/// The scheme wikilinks are rewritten to before rendering.
pub const WIKILINK_SCHEME: &str = "wikilink:";

/// What the preview asks the app to do.
#[derive(Clone, Debug, PartialEq)]
pub enum PreviewAction {
    Close,
    /// A `[[wikilink]]` was clicked: open the note with this name (or create it).
    Wikilink(String),
    /// Any other link, for the browser.
    OpenUrl(String),
    /// A slide was clicked in the deck preview: put the editor on this line.
    JumpToLine(usize),
}

/// What the preview draws for a presentation.
pub struct DeckProps<'a> {
    pub deck: Option<&'a Deck>,
    pub active: Option<usize>,
    pub vars: &'a HashMap<String, String>,
    pub images: &'a dyn ImageSource,
    pub preview: &'a mut DeckPreview,
}

#[derive(Default)]
pub struct Preview {
    /// The text the document was built from, and when.
    built_from: Option<(String, String)>,
    built_at: f64,
    doc: Option<Document>,
    view: RichView,
}

impl Preview {
    /// Draws the pane for note `id` with Markdown `text`.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        id: Option<&str>,
        text: &str,
        deck: Option<DeckProps<'_>>,
        attachments: Option<PathBuf>,
    ) -> Vec<PreviewAction> {
        let is_deck = deck.is_some();
        let p = theme::palette(ui.ctx());
        let mut out = Vec::new();
        egui::Panel::top(ui.scope_id().with("preview-toolbar"))
            .frame(
                Frame::NONE
                    .fill(p.bg_surface)
                    .inner_margin(Margin::symmetric(12, 6))
                    .stroke(egui::Stroke::new(1.0, p.border)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(if is_deck { "SLIDES" } else { "PREVIEW" })
                            .size(11.0)
                            .color(p.text_muted),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let close = ui
                            .add(
                                egui::Button::new(icons::icon_sized(icons::XMARK, 12.0))
                                    .frame(false),
                            )
                            .on_hover_text("Close Preview");
                        close.widget_info(|| {
                            egui::WidgetInfo::labeled(egui::Role::Button, true, "Close Preview")
                        });
                        if close.clicked() {
                            out.push(PreviewAction::Close);
                        }
                    });
                });
            });
        egui::CentralPanel::default()
            .frame(Frame::NONE.fill(p.bg_surface).inner_margin(if is_deck {
                Margin::ZERO
            } else {
                Margin::symmetric(16, 8)
            }))
            .show(ui, |ui| {
                if let Some(d) = deck {
                    if let Some(line) = d.preview.show(ui, d.deck, d.active, d.vars, d.images) {
                        out.push(PreviewAction::JumpToLine(line));
                    }
                    return;
                }
                let Some(id) = id else {
                    ui.label(RichText::new("Nothing to preview").color(p.text_muted));
                    return;
                };
                if text.trim().is_empty() {
                    ui.label(RichText::new("Nothing to preview").color(p.text_muted));
                    return;
                }
                self.rebuild(ui, id, text);
                let Some(doc) = &self.doc else { return };
                let mut resolver = AttachmentResolver { dir: attachments };
                let style = RichStyle::from_ctx(ui.ctx());
                let shown = self.view.show(ui, doc, &style, &mut resolver);
                for a in shown.actions {
                    let Action::Open(target) = a;
                    match target.strip_prefix(WIKILINK_SCHEME) {
                        Some(name) => out.push(PreviewAction::Wikilink(percent_decode(name))),
                        None => out.push(PreviewAction::OpenUrl(target)),
                    }
                }
            });
        out
    }

    /// Rebuilds the document when the text changed, throttled while typing.
    fn rebuild(&mut self, ui: &Ui, id: &str, text: &str) {
        let now = ui.input(|i| i.time);
        let same_note = self.built_from.as_ref().is_some_and(|(i, _)| i == id);
        let stale = self
            .built_from
            .as_ref()
            .is_none_or(|(i, t)| i != id || t != text);
        if !stale {
            return;
        }
        if same_note && now - self.built_at < REBUILD_SECS {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs_f64(REBUILD_SECS));
            return;
        }
        self.doc = Some(document(text));
        if !same_note {
            self.view.reset();
        }
        self.built_from = Some((id.to_string(), text.to_string()));
        self.built_at = now;
    }
}

/// The note's Markdown as a rich-text document.
pub fn document(markdown: &str) -> Document {
    let html = noteliner_core::export::preview_html(&wikilinks_to_html(markdown));
    from_note_html(&html, &Options::default())
}

/// `[[Name]]` / `[[Name|alias]]` → `<a href="wikilink:Name">alias</a>`,
/// outside fenced code blocks and inline code (the `marked` extension only
/// sees inline text).
pub fn wikilinks_to_html(markdown: &str) -> String {
    static WIKILINK: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\[\[([^\[\]|]+)(?:\|([^\[\]]+))?\]\]").expect("regex"));
    let mut out = String::with_capacity(markdown.len());
    let mut fence: Option<&str> = None;
    for line in markdown.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if let Some(f) = fence {
            if trimmed.starts_with(f) {
                fence = None;
            }
            out.push_str(line);
            continue;
        }
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fence = Some(&trimmed[..3]);
            out.push_str(line);
            continue;
        }
        // Even segments are prose; odd ones are inside `code`.
        for (i, part) in line.split('`').enumerate() {
            if i > 0 {
                out.push('`');
            }
            if i % 2 == 1 {
                out.push_str(part);
                continue;
            }
            let replaced = WIKILINK.replace_all(part, |c: &regex::Captures<'_>| {
                let target = c[1].trim();
                let label = c.get(2).map_or(target, |a| a.as_str().trim());
                format!(
                    "<a href=\"{WIKILINK_SCHEME}{}\">{}</a>",
                    percent_encode(target),
                    escape_html(label)
                )
            });
            out.push_str(&replaced);
        }
    }
    out
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn percent_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Images from the project's `_attachments` folder; wikilinks and web links
/// are reported to the app.
pub struct AttachmentResolver {
    pub dir: Option<PathBuf>,
}

/// The attachment file name an image `src` points at, if it's one.
pub fn attachment_name(src: &str) -> Option<String> {
    let rest = src
        .strip_prefix("./_attachments/")
        .or_else(|| src.strip_prefix("/_attachments/"))
        .or_else(|| src.strip_prefix("_attachments/"))
        .or_else(|| src.strip_prefix("attachment://localhost/"))
        .or_else(|| src.strip_prefix("attachment:///"))?;
    let name = percent_decode(rest);
    (!name.is_empty() && !name.contains('/') && !name.contains('\\') && !name.contains(".."))
        .then_some(name)
}

impl Resolver for AttachmentResolver {
    fn image(&mut self, ctx: &egui::Context, src: &str) -> Option<String> {
        if src.starts_with("data:") {
            return marina_richtext::resolve::data_uri(ctx, src);
        }
        if src.starts_with("http://") || src.starts_with("https://") {
            return Some(src.to_string());
        }
        let name = attachment_name(src)?;
        let dir = self.dir.as_ref()?;
        let uri = format!("bytes://attachments/{name}");
        if ctx.try_load_bytes(&uri).is_err() {
            ctx.include_bytes(uri.clone(), std::fs::read(dir.join(&name)).ok()?);
        }
        Some(uri)
    }

    fn link(&mut self, target: &str) -> LinkAction {
        match target.strip_prefix('#') {
            Some(anchor) => LinkAction::Anchor(anchor.to_string()),
            None => LinkAction::Open(target.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wikilinks_outside_code() {
        let md = "See [[Other Note]] and [[a/b|the alias]].\n`[[not]]`\n```\n[[code]]\n```\n";
        let out = wikilinks_to_html(md);
        assert!(out.contains("<a href=\"wikilink:Other%20Note\">Other Note</a>"));
        assert!(out.contains("<a href=\"wikilink:a%2Fb\">the alias</a>"));
        assert!(out.contains("`[[not]]`"));
        assert!(out.contains("\n[[code]]\n"));
        assert_eq!(percent_decode("a%2Fb%20c"), "a/b c");
    }

    #[test]
    fn attachment_sources() {
        assert_eq!(
            attachment_name("./_attachments/att-1.png").as_deref(),
            Some("att-1.png")
        );
        assert_eq!(
            attachment_name("attachment://localhost/att%20x.png").as_deref(),
            Some("att x.png")
        );
        assert_eq!(attachment_name("_attachments/../secret"), None);
        assert_eq!(attachment_name("https://x/y.png"), None);
    }

    #[test]
    fn document_has_the_wikilink() {
        let doc = document("# Title\n\nGo to [[Beta]].\n");
        let text = format!("{doc:?}");
        assert!(text.contains("wikilink:Beta"), "{text}");
    }
}
