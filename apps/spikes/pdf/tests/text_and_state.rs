//! Text boxes on a hand-written PDF land where pdf.js puts them (scale-1 page units, top-left
//! origin), and highlights round-trip through `pageliner-core`'s state file.

use std::sync::Arc;

use hayro::hayro_interpret::{InterpreterCache, InterpreterSettings};
use hayro::RenderCache;
use pageliner_core::library::Library;
use spike_pdf::annotations::{self, Highlight};
use spike_pdf::{doc, text};

/// Helvetica's width for "H" (AFM), in 1000ths of the font size.
const HELVETICA_H: f32 = 722.0;

#[test]
fn glyph_boxes_use_page_units() {
    let pdf = doc::open(Arc::new(one_page_pdf(
        "",
        "BT /F1 12 Tf 72 700 Td (Hello) Tj ET",
    )))
    .unwrap();
    assert_eq!(doc::page_sizes(&pdf), vec![[612.0, 792.0]]);
    let t = text::extract(
        &pdf.pages()[0],
        &InterpreterCache::new(),
        &InterpreterSettings::default(),
    );
    let s: String = t.chars.iter().map(|c| c.text.as_str()).collect();
    assert_eq!(s, "Hello");
    let [x, y, w, h] = t.chars[0].rect;
    assert_close(x, 72.0);
    // Baseline at 792 - 700 = 92; the box spans 0.8 em above to 0.2 em below it.
    assert_close(y, 92.0 - 0.8 * 12.0);
    assert_close(h, 12.0);
    assert_close(w, HELVETICA_H / 1000.0 * 12.0);
}

#[test]
fn rotated_pages_put_text_where_it_is_drawn() {
    let pdf = doc::open(Arc::new(one_page_pdf(
        "/Rotate 90",
        "BT /F1 12 Tf 72 700 Td (Hello) Tj ET",
    )))
    .unwrap();
    assert_eq!(doc::page_sizes(&pdf), vec![[792.0, 612.0]]);
    let t = text::extract(
        &pdf.pages()[0],
        &InterpreterCache::new(),
        &InterpreterSettings::default(),
    );
    // Rotated 90° clockwise: PDF (x, y) lands at (y, x) from the top left, so text runs down.
    let [x, y, w, h] = t.chars[0].rect;
    assert_close(y, 72.0);
    assert_close(x, 700.0 - 0.2 * 12.0);
    assert_close(w, 12.0);
    assert!(h > 8.0 && h < 9.0);
    let image = doc::render_page(
        &pdf.pages()[0],
        &RenderCache::new(),
        &InterpreterSettings::default(),
        1.0,
    );
    assert_eq!(image.size, [792, 612]);
}

#[test]
fn drag_selection_yields_text_and_line_rects() {
    let pdf = doc::open(Arc::new(one_page_pdf(
        "",
        "BT /F1 12 Tf 72 700 Td (Hello world) Tj 0 -20 Td (Second line) Tj ET",
    )))
    .unwrap();
    let t = text::extract(
        &pdf.pages()[0],
        &InterpreterCache::new(),
        &InterpreterSettings::default(),
    );
    // "Second" spans x 72–112.7; its "d" starts at 106.
    let sel = t.select([73.0, 90.0], [108.0, 110.0]).unwrap();
    assert_eq!(sel.text, "Hello world Second");
    assert_eq!(sel.rects.len(), 2);
    assert_close(sel.rects[1][1], 112.0 - 9.6);
}

#[test]
fn highlights_round_trip_through_pageliner_state() {
    let dir = tempfile::tempdir().unwrap();
    let lib = Library::open(dir.path().to_path_buf()).unwrap();
    let h = Highlight::new(2, &[[72.0, 82.4, 30.5, 12.0]], "Hello");
    lib.set_state("book-1", &serde_json::json!({ "page": 3, "format": "pdf" }))
        .unwrap();
    let state = lib
        .set_state(
            "book-1",
            &annotations::highlights_patch(std::slice::from_ref(&h)),
        )
        .unwrap();
    assert_eq!(state["page"], 3);
    assert_eq!(state["highlights"][0]["createdAt"], h.created_at.as_str());
    assert_eq!(state["highlights"][0]["rects"][0]["w"], 30.5);
    assert_eq!(
        annotations::highlights_from_state(&lib.get_state("book-1")),
        vec![h]
    );
}

#[test]
fn reads_highlights_written_by_the_svelte_reader() {
    let state = serde_json::json!({
        "page": 4, "format": "pdf",
        "highlights": [{ "id": "a1", "kind": "highlight", "page": 4, "rects": [{ "x": 56.1, "y": 101.25, "w": 210.4, "h": 13.2 }], "text": "quoted text", "createdAt": "2026-09-30T10:00:00.000Z" }],
    });
    let hs = annotations::highlights_from_state(state.as_object().unwrap());
    assert_eq!(hs.len(), 1);
    assert_eq!(hs[0].page, 4);
    assert_eq!(hs[0].rects[0].y, 101.25);
}

fn assert_close(actual: f32, expected: f32) {
    assert!((actual - expected).abs() < 0.05, "{actual} != {expected}");
}

/// A Letter-size, one-page PDF using the (non-embedded) Helvetica standard font.
fn one_page_pdf(page_extra: &str, content: &str) -> Vec<u8> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] {page_extra} /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>"),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len() + 1),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_string(),
    ];
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}
