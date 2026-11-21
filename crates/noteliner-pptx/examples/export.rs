//! `cargo run -p noteliner-pptx --example export -- deck.md out.pptx`
//!
//! Exports a presentation the way the app does, without formulas. Pictures
//! are read from the paths on the slides, beside the note.

use std::path::Path;

use noteliner_pptx::{export, needs, Assets, Format, Options, Picture};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [input, output] = args.as_slice() else {
        eprintln!("usage: export deck.md out.pptx");
        std::process::exit(2);
    };
    let raw = std::fs::read_to_string(input).expect("read the note");
    let deck = noteliner_slides::parse_deck(&raw, front(&raw).as_ref());
    let dir = Path::new(input).parent().unwrap_or(Path::new("."));
    let mut assets = Assets::default();
    for src in needs(&deck).pictures {
        let Ok(bytes) = std::fs::read(dir.join(&src)) else {
            continue;
        };
        let (Some(format), Ok(size)) = (
            Format::sniff(&bytes),
            image::image_dimensions(dir.join(&src)),
        ) else {
            continue;
        };
        assets.pictures.insert(
            src,
            Picture {
                bytes,
                format,
                natural: (size.0 as f32, size.1 as f32),
            },
        );
    }
    let name = Path::new(input)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let options = Options {
        title: name.clone(),
        vars: [
            ("name".to_string(), name.clone()),
            ("title".to_string(), name),
        ]
        .into(),
        ..Default::default()
    };
    let out = export(&deck, &assets, &options).expect("export");
    for w in &out.warnings {
        eprintln!("{w}");
    }
    std::fs::write(output, out.bytes).expect("write the file");
    println!("{} slides", deck.slides.len());
}

/// The `presentation:` block's simple `key: value` lines, enough for a demo.
fn front(raw: &str) -> Option<serde_json::Value> {
    let rest = raw.strip_prefix("---\n")?;
    let block = &rest[..rest.find("\n---")?];
    let mut map = serde_json::Map::new();
    let mut inside = false;
    for line in block.lines() {
        if line.starts_with("presentation:") {
            inside = true;
        } else if inside && line.starts_with("  ") {
            let (k, v) = line.trim().split_once(':')?;
            let v = v.trim().trim_matches('"');
            let value = match v {
                "true" => true.into(),
                "false" => false.into(),
                _ => v.parse::<u64>().map_or_else(|_| v.into(), Into::into),
            };
            map.insert(k.to_string(), value);
        } else {
            inside = false;
        }
    }
    Some(serde_json::Value::Object(map))
}
