//! `spike-article report`: converts every cached article headlessly and reports conversion and
//! layout times, how much text survived, what was dropped, and whether it's readable.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::time::Instant;

use anyhow::Result;
use spike_common::Stats;
use spike_richtext::fonts::Face;
use spike_richtext::headless::Headless;
use spike_richtext::{convert, stats};

use crate::corpus::{self, Article};

/// An article is readable when at least this share of its text survives…
const MIN_KEPT: f64 = 0.9;
/// …and it isn't an embed (video, iframe, audio) with almost no text and no image around it.
const EMBED_ONLY_MAX_CHARS: usize = 200;
const EMBEDS: &[&str] = &["iframe", "video", "audio", "object", "embed"];

struct Row {
    feed: String,
    title: String,
    convert_ms: f64,
    measure_ms: f64,
    steady_ms: f64,
    kept: f64,
    blocks: usize,
    images: usize,
    dropped: BTreeMap<&'static str, usize>,
    readable: bool,
    reason: &'static str,
}

pub fn run() -> Result<()> {
    let articles = corpus::load()?;
    let headless = Headless::new(Face::Sans, 1000.0, 800.0);
    let (mut convert_t, mut measure_t, mut steady_t, mut jump_t) = (
        Stats::default(),
        Stats::default(),
        Stats::default(),
        Stats::default(),
    );
    let mut rows = Vec::new();
    for a in &articles {
        let start = Instant::now();
        let doc = convert::from_article_html(&a.content, a.link.as_deref());
        let convert_d = start.elapsed();
        convert_t.push(convert_d);
        let times = headless.time(&doc);
        measure_t.push(times.measure);
        steady_t.push(times.steady);
        jump_t.push(times.jump);
        let source = stats::text_content(&a.content);
        let kept = stats::kept_ratio(&source, &doc);
        let dropped = stats::dropped_elements(&a.content);
        let (readable, reason) = classify(
            a,
            kept,
            &dropped,
            stats::visible_chars(&source),
            doc.image_count(),
        );
        rows.push(Row {
            feed: a.feed.clone(),
            title: a.title.chars().take(48).collect(),
            convert_ms: ms(convert_d),
            measure_ms: ms(times.measure),
            steady_ms: ms(times.steady),
            kept,
            blocks: doc.blocks.len(),
            images: doc.image_count(),
            dropped,
            readable,
            reason,
        });
    }

    let mut md = String::new();
    writeln!(md, "| # | Feed | Title | Convert ms | Measure ms | Steady ms | Kept | Blocks | Images | Dropped | Readable |")?;
    writeln!(md, "|---|---|---|---|---|---|---|---|---|---|---|")?;
    for (i, r) in rows.iter().enumerate() {
        let dropped = r
            .dropped
            .iter()
            .map(|(k, n)| format!("{k}×{n}"))
            .collect::<Vec<_>>()
            .join(" ");
        writeln!(
            md,
            "| {i} | {} | {} | {:.2} | {:.2} | {:.2} | {:.0}% | {} | {} | {} | {} |",
            r.feed,
            r.title.replace('|', "/"),
            r.convert_ms,
            r.measure_ms,
            r.steady_ms,
            r.kept * 100.0,
            r.blocks,
            r.images,
            dropped,
            if r.readable {
                "yes".to_string()
            } else {
                format!("**no** ({})", r.reason)
            }
        )?;
    }
    let readable = rows.iter().filter(|r| r.readable).count();
    let mut totals: BTreeMap<&str, usize> = BTreeMap::new();
    for r in &rows {
        for (k, n) in &r.dropped {
            *totals.entry(k).or_default() += n;
        }
    }
    writeln!(md)?;
    writeln!(
        md,
        "Articles: {} from {} feeds",
        rows.len(),
        rows.iter()
            .map(|r| &r.feed)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
    )?;
    writeln!(
        md,
        "Readable: {readable}/{} ({:.0}%)",
        rows.len(),
        readable as f64 * 100.0 / rows.len().max(1) as f64
    )?;
    writeln!(md, "{}", convert_t.summary("Sanitize + convert"))?;
    writeln!(
        md,
        "{}",
        measure_t.summary("First frame (measure all blocks)")
    )?;
    writeln!(md, "{}", steady_t.summary("Steady frame"))?;
    writeln!(
        md,
        "{}",
        jump_t.summary("Frame after jumping to the middle")
    )?;
    writeln!(md, "Dropped elements (all articles): {totals:?}")?;
    print!("{md}");
    let path = corpus::cache_dir().join("report.md");
    std::fs::write(&path, &md)?;
    log::info!("Wrote {}", path.display());
    Ok(())
}

fn classify(
    a: &Article,
    kept: f64,
    dropped: &BTreeMap<&str, usize>,
    source_chars: usize,
    images: usize,
) -> (bool, &'static str) {
    let embeds = EMBEDS.iter().any(|k| dropped.contains_key(k));
    if embeds && source_chars < EMBED_ONLY_MAX_CHARS && images == 0 {
        return (false, "embed only");
    }
    if source_chars == 0 && images == 0 {
        log::warn!("{}: no text and no images", a.title);
        return (false, "empty");
    }
    if kept < MIN_KEPT {
        return (false, "text lost");
    }
    (true, "")
}

fn ms(d: std::time::Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}
