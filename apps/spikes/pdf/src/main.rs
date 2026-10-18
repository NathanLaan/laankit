//! `spike-pdf <file.pdf> [--state <state.json>]`: a continuous-scroll PDF reader on hayro.
//! `spike-pdf --bench [--pages N] [--png-dir DIR] <file.pdf>...`: headless measurements.
//!
//! Pages render on worker threads into textures kept in a small LRU cache; drag on a page to
//! select text, then "Highlight" stores it in PageLiner's state format.

mod bench;
mod reader;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--bench") {
        spike_common::init_logging();
        return bench::run(&args[1..]);
    }
    let Some(path) = args.first().cloned() else {
        anyhow::bail!("usage: spike-pdf <file.pdf> [--state <state.json>] | --bench <file.pdf>...");
    };
    let state = args
        .iter()
        .position(|a| a == "--state")
        .and_then(|i| args.get(i + 1))
        .map(Into::into);
    let bytes = std::sync::Arc::new(std::fs::read(&path)?);
    spike_common::run("PDF spike", move |cc| {
        reader::Reader::new(cc, path.into(), bytes, state)
    })
    .map_err(|e| anyhow::anyhow!("{e}"))
}
