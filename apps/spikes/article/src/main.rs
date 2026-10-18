//! Spike 2 (see `docs/plans/plan-native-rust-egui.md`, Phase 0): ThreadLiner's article HTML
//! rendered natively, without a webview.
//!
//! ```text
//! spike-article fetch    # fetch the feeds and cache their entries in $SPIKE_ARTICLE_DIR
//! spike-article report   # headless: timings, text kept, elements dropped, readability
//! spike-article          # the reader (MARINA_ARTICLE=<n> picks the entry)
//! ```

mod app;
mod corpus;
mod report;

fn main() -> anyhow::Result<()> {
    spike_common::init_logging();
    match std::env::args().nth(1).as_deref() {
        Some("fetch") => corpus::fetch(),
        Some("report") => report::run(),
        _ => {
            let articles = corpus::load()?;
            spike_common::run("Spike: articles", move |cc| {
                app::ArticleApp::new(cc, articles)
            })
            .map_err(|e| anyhow::anyhow!("{e}"))
        }
    }
}
