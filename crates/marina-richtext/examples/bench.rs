//! Frame times of progressive layout against measuring every block in the first frame (the
//! Phase 0 spike's approach), headless at 1000×800 with the Marina fonts.
//!
//! ```text
//! cargo run --profile fast-release -p marina-richtext --example bench [articles.json]
//! ```
//!
//! `articles.json` is a cache of real feed articles (the Phase 0 article spike made one): an array of
//! `{ "title", "link", "content" }`. Without it, only synthetic documents are measured.

use std::time::{Duration, Instant};

use egui::{Pos2, RawInput, Rect};
use marina_richtext::{
    from_article_html, from_html, Document, Options, Position, RichStyle, RichView, WebResolver,
};

fn main() {
    let mut docs: Vec<(String, Document)> = [200, 600, 2000]
        .into_iter()
        .map(|n| (format!("synthetic {n} blocks"), synthetic(n)))
        .collect();
    if let Some(path) = std::env::args().nth(1) {
        let json = std::fs::read_to_string(&path).expect("read articles");
        let articles: Vec<serde_json::Value> = serde_json::from_str(&json).expect("parse");
        let mut converted: Vec<(String, Document)> = articles
            .iter()
            .map(|a| {
                let title = a["title"].as_str().unwrap_or("").chars().take(40).collect();
                let doc =
                    from_article_html(a["content"].as_str().unwrap_or(""), a["link"].as_str());
                (title, doc)
            })
            .collect();
        // The longest articles are the interesting ones.
        converted.sort_by_key(|(_, d)| std::cmp::Reverse(d.blocks.len()));
        docs.extend(converted.into_iter().take(5));
    }

    println!(
        "{:<44} {:>6} | {:>10} | {:>10} {:>10} {:>7} {:>10}",
        "document", "blocks", "eager 1st", "prog 1st", "prog max", "frames", "jump"
    );
    for (name, doc) in &docs {
        let eager = first_frame(doc, Duration::MAX);
        let (first, max, frames) = settle(doc);
        let jump = jump(doc);
        println!(
            "{:<44} {:>6} | {:>10} | {:>10} {:>10} {:>7} {:>10}",
            name,
            doc.blocks.len(),
            ms(eager),
            ms(first),
            ms(max),
            frames,
            ms(jump)
        );
    }
}

/// A context as the app has it when the user opens another document: fonts installed and glyphs
/// rasterized (by a warm-up document in every style), but nothing of this document's text laid
/// out yet. Each measurement gets a fresh one so egui's galley cache starts cold.
fn fresh() -> egui::Context {
    let ctx = egui::Context::default();
    if std::env::var_os("BENCH_EGUI_FONTS").is_none() {
        marina_egui::fonts::install(&ctx);
    }
    marina_egui::theme::ThemeState::default().apply(&ctx);
    let warm = from_html(WARM_UP, &Options::default());
    let mut view = RichView::with_budget(Duration::MAX);
    for _ in 0..3 {
        frame(&ctx, &mut view, &warm);
    }
    ctx
}

/// Every character class and style the documents use, in text they don't contain.
const WARM_UP: &str = "<h1>QWERTY qwerty</h1><h2>Zyxw 0123456789</h2><h3>Vutsr</h3><h4>Ponml</h4>\
<p>ABCDEFGHIJKLMNOPQRSTUVWXYZ abcdefghijklmnopqrstuvwxyz 0123456789 .,;:!?'\"()[]{}<>/\\|@#$%^&*-_+=~` \
’‘“”–—…•·©®™°±×÷€£¥ àáâäæçèéêëìíîïñòóôöøùúûüýÿ</p>\
<p><b>ABCDEFGHIJKLMNOPQRSTUVWXYZ abcdefghijklmnopqrstuvwxyz 0123456789</b> \
<i>ABCDEFGHIJKLMNOPQRSTUVWXYZ abcdefghijklmnopqrstuvwxyz 0123456789</i> \
<b><i>ABCDEFGHIJKLMNOPQRSTUVWXYZ abcdefghijklmnopqrstuvwxyz 0123456789</i></b> \
<code>ABCDEFGHIJKLMNOPQRSTUVWXYZ abcdefghijklmnopqrstuvwxyz 0123456789{}()</code> \
<small>ABCDEFGHIJKLMNOPQRSTUVWXYZ abcdefghijklmnopqrstuvwxyz</small></p>\
<pre>ABCDEFGHIJKLMNOPQRSTUVWXYZ abcdefghijklmnopqrstuvwxyz 0123456789 {}()[];:=&lt;&gt;!\"'#</pre>\
<figure><figcaption>ABCDEFGHIJKLMNOPQRSTUVWXYZ abcdefghijklmnopqrstuvwxyz 0123456789</figcaption></figure>\
<ol><li>1</li></ol><ul><li>x</li></ul>";

/// The first frame of a fresh view, median of 5 cold runs.
fn first_frame(doc: &Document, budget: Duration) -> Duration {
    let mut times: Vec<Duration> = (0..5)
        .map(|_| frame(&fresh(), &mut RichView::with_budget(budget), doc))
        .collect();
    times.sort();
    times[2]
}

/// Progressive layout from open to settled: first frame, slowest frame, frame count.
fn settle(doc: &Document) -> (Duration, Duration, usize) {
    let ctx = &fresh();
    let mut view = RichView::default();
    let first = frame(ctx, &mut view, doc);
    let (mut max, mut frames) = (first, 1);
    while !view.settled() && frames < 10_000 {
        max = max.max(frame(ctx, &mut view, doc));
        frames += 1;
    }
    (first, max, frames)
}

/// Opening a document straight at its middle block (a restored reading position).
fn jump(doc: &Document) -> Duration {
    let ctx = &fresh();
    let mut view = RichView::default();
    view.restore(Position {
        block: doc.blocks.len() / 2,
        offset: 0,
    });
    frame(ctx, &mut view, doc)
}

fn frame(ctx: &egui::Context, view: &mut RichView, doc: &Document) -> Duration {
    let input = RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 800.0))),
        ..Default::default()
    };
    let start = Instant::now();
    let mut out = ctx.run_ui(input, |ui| {
        let style = RichStyle::from_ctx(ui.ctx());
        view.show(ui, doc, &style, &mut WebResolver);
    });
    let _ = ctx.tessellate(std::mem::take(&mut out.shapes), out.pixels_per_point);
    out.drop_without_applying_deltas();
    start.elapsed()
}

fn synthetic(blocks: usize) -> Document {
    let html: String = (0..blocks)
        .map(|i| match i % 6 {
            0 => format!("<h2>Section {i}</h2>"),
            1 => "<ul><li>One <em>item</em></li><li>Two</li><li>Three</li></ul>".to_string(),
            2 => "<pre>fn main() {\n    println!(\"hi\");\n}</pre>".to_string(),
            3 => "<blockquote><p>Quoted text, quoted at length. </p></blockquote>".to_string(),
            _ => format!(
                "<p>{i} {}</p>",
                "Lorem ipsum <b>dolor</b> sit amet, <a href=\"#x\">consectetur</a> adipiscing. "
                    .repeat(10)
            ),
        })
        .collect();
    from_html(&html, &Options::default())
}

fn ms(d: Duration) -> String {
    format!("{:.2} ms", d.as_secs_f64() * 1000.0)
}
