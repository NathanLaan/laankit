//! The open presentation (`projectState.deck`): the parsed deck, cached per
//! edit, and the `noteliner-slides` widgets that show it: the SLIDES pane,
//! the deck preview and the presenter. See `crates/noteliner-slides/README.md`
//! for the contract.

use std::collections::HashMap;
use std::path::PathBuf;

use noteliner_slides::{
    estimate_duration, parse_deck, slide_at_line, Deck, DeckPreview, ImageSource, Presenter,
    SlidesPane,
};
use serde_json::Value;

use crate::views::preview::attachment_name;

/// `StatusBar.svelte`'s speaking rate.
pub const WPM: f64 = 130.0;

/// The deck of the open note, and the widgets drawing it.
#[derive(Default)]
pub struct DeckState {
    /// What the deck was parsed from: note id, editor version, presentation JSON.
    key: Option<(String, u64, String)>,
    deck: Option<Deck>,
    pub pane: SlidesPane,
    pub preview: DeckPreview,
    pub presenter: Option<Presenter>,
}

impl DeckState {
    /// Re-parses when the note, its text or its `presentation:` block changed.
    /// `presentation` is `None` for an ordinary note.
    pub fn update(
        &mut self,
        id: Option<&str>,
        version: u64,
        text: &str,
        presentation: Option<&Value>,
    ) {
        let (Some(id), Some(presentation)) = (id, presentation.filter(|p| !p.is_null())) else {
            self.key = None;
            self.deck = None;
            self.presenter = None;
            return;
        };
        let key = (id.to_string(), version, presentation.to_string());
        if self.key.as_ref() != Some(&key) {
            self.deck = Some(parse_deck(text, Some(presentation)));
            self.key = Some(key);
        }
    }

    pub fn deck(&self) -> Option<&Deck> {
        self.deck.as_ref()
    }

    /// The deck and its widgets, borrowed together.
    pub fn parts(&mut self) -> (Option<&Deck>, &mut SlidesPane, &mut DeckPreview) {
        (self.deck.as_ref(), &mut self.pane, &mut self.preview)
    }

    /// The slide holding the caret (1-based).
    pub fn active(&self, caret_line: usize) -> Option<usize> {
        slide_at_line(self.deck.as_ref()?, caret_line).map(|s| s.index)
    }

    /// The status bar's deck segments: current slide, total, minutes.
    pub fn status(&self, caret_line: usize) -> Option<(usize, usize, usize)> {
        let deck = self.deck.as_ref().filter(|d| !d.slides.is_empty())?;
        let (_, minutes) = estimate_duration(deck, WPM);
        Some((
            self.active(caret_line).unwrap_or(1),
            deck.slides.len(),
            (minutes.round() as usize).max(1),
        ))
    }
}

/// `DeckPreview.svelte`'s header/footer placeholders.
pub fn vars(note_name: &str) -> HashMap<String, String> {
    let now = chrono::Local::now();
    let date = now.format("%Y-%m-%d").to_string();
    let time = now.format("%H:%M").to_string();
    HashMap::from([
        ("name".into(), note_name.to_string()),
        ("title".into(), note_name.to_string()),
        ("datetime".into(), format!("{date} {time}")),
        ("date".into(), date),
        ("time".into(), time),
    ])
}

/// Slide images: `_attachments/…` from the project folder (as the preview
/// resolves them), remote and `data:` URLs as they are.
pub struct SlideImages {
    pub ctx: egui::Context,
    pub dir: Option<PathBuf>,
}

impl ImageSource for SlideImages {
    fn uri(&self, src: &str) -> Option<String> {
        if src.starts_with("data:") {
            return marina_richtext::resolve::data_uri(&self.ctx, src);
        }
        if src.starts_with("http://") || src.starts_with("https://") {
            return Some(src.to_string());
        }
        let name = attachment_name(src)?;
        let dir = self.dir.as_ref()?;
        let uri = format!("bytes://attachments/{name}");
        if self.ctx.try_load_bytes(&uri).is_err() {
            self.ctx
                .include_bytes(uri.clone(), std::fs::read(dir.join(&name)).ok()?);
        }
        Some(uri)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_only_decks_and_caches_per_version() {
        let mut d = DeckState::default();
        let text = "# One\n\nHello\n\n---\n\n# Two\n\nWorld\n";
        d.update(Some("a"), 1, text, None);
        assert!(d.deck().is_none(), "not a presentation");
        let p = json!({ "theme": "dark" });
        d.update(Some("a"), 1, text, Some(&p));
        assert_eq!(d.deck().unwrap().slides.len(), 2);
        assert_eq!(d.active(7), Some(2));
        let (current, total, minutes) = d.status(1).unwrap();
        assert_eq!((current, total, minutes), (1, 2, 1));
        // A new version re-parses.
        d.update(Some("a"), 2, "# Only\n", Some(&p));
        assert_eq!(d.deck().unwrap().slides.len(), 1);
    }

    #[test]
    fn vars_fill_the_svelte_placeholders() {
        let v = vars("Talk");
        for k in ["name", "title", "date", "time", "datetime"] {
            assert!(v.contains_key(k), "{k}");
        }
        assert_eq!(v["title"], "Talk");
    }
}
