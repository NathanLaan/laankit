//! The EPUB reader: a thin adapter over `pageliner_epub::EpubView`, which
//! draws its own toolbar, sidebar (Contents, Annotations) and text.
//!
//! State: EPUB positions and highlights were **reset** for the native reader
//! (decision of 2026-10-17). `EpubView` keeps everything under the book
//! state's `native` key and never writes the epub.js `cfi`, `bookmarks` or
//! `highlights` keys, so the Electron and Tauri builds keep their own.
//! Reading preferences (`epubFontSize`, `epubTheme`, `epubFace`) go to
//! `ui-preferences.json`, as the Svelte reader's do.

use std::path::PathBuf;
use std::sync::Arc;

use egui::Ui;
use marina_egui::not_ported::NotPorted;
use marina_egui::panes::PaneLayout;
use marina_egui::theme;
use pageliner_epub::{Book as EpubBook, EpubEvent, EpubView, NativeState, ReaderPrefs};
use serde_json::{Map, Value};

use super::{back_bar, bar_frame, ReaderAction};
use crate::model::Book;

pub enum EpubReader {
    Open(Box<EpubView>),
    /// The file couldn't be read as an EPUB: the reason, for the error view.
    Failed {
        title: String,
        error: String,
    },
}

impl EpubReader {
    /// Opens `book` from its stored file (or `bytes`), at the position saved
    /// under `state`'s `native` key, with the reading prefs from `ui_prefs`.
    pub fn new(
        book: &Book,
        path: PathBuf,
        bytes: Arc<Vec<u8>>,
        state: Map<String, Value>,
        ui_prefs: &Map<String, Value>,
    ) -> Self {
        let opened = EpubBook::open(&path).or_else(|_| EpubBook::from_bytes(bytes.to_vec()));
        match opened {
            Ok(epub) => Self::Open(Box::new(EpubView::new(
                epub,
                NativeState::from_book_state(&state),
                ReaderPrefs::from_ui_prefs(ui_prefs),
            ))),
            Err(e) => {
                log::warn!("Couldn't open {}: {e:#}", path.display());
                Self::Failed {
                    title: book.title.clone(),
                    error: e.to_string(),
                }
            }
        }
    }

    /// `_keys` is false while a dialog or the palette owns the keyboard; the
    /// view already ignores its paging keys whenever a text field has focus,
    /// which covers both.
    pub fn show(&mut self, ui: &mut Ui, panes: &mut PaneLayout, _keys: bool) -> Vec<ReaderAction> {
        let view = match self {
            Self::Open(view) => view,
            Self::Failed { title, error } => return failed(ui, title, error),
        };
        // The sidebar layout is shared with the PDF reader.
        view.set_panes(panes.clone());
        let events = view.show(ui);
        *panes = view.panes().clone();
        events
            .into_iter()
            .filter_map(|e| match e {
                EpubEvent::Close => Some(ReaderAction::Close),
                EpubEvent::SaveState(patch) => Some(ReaderAction::SaveState(patch)),
                EpubEvent::SavePrefs(patch) => Some(ReaderAction::SavePrefs(patch)),
                EpubEvent::Status(text) => Some(ReaderAction::Status(Some(text))),
                EpubEvent::OpenUrl(url) => {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                    None
                }
            })
            .collect()
    }

    pub fn flush(&mut self) -> Option<Value> {
        match self {
            Self::Open(view) => view.flush(),
            Self::Failed { .. } => None,
        }
    }
}

/// The reader bar and the reason the book didn't open.
fn failed(ui: &mut Ui, title: &str, error: &str) -> Vec<ReaderAction> {
    let p = theme::palette(ui.ctx());
    let mut out = Vec::new();
    egui::Panel::top("epub-bar")
        .frame(bar_frame(&p))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                if back_bar(ui, title) {
                    out.push(ReaderAction::Close);
                }
            });
        });
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE.fill(p.bg_base))
        .show(ui, |ui| {
            NotPorted::new("This EPUB")
                .note(&format!("It couldn't be opened: {error}"))
                .show(ui);
        });
    out
}
