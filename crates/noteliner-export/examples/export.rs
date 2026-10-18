//! `cargo run --profile spike -p noteliner-export --example export -- [--repeat N] [--typ OUT.typ] <in.md> <out.pdf|out.html>`
//!
//! Exports a note the way the native NoteLiner does: the bundled fonts, the
//! file name as the title, images from the note's folder. `--repeat N`
//! exports N more times and reports cold vs. warm timings.

use std::path::{Path, PathBuf};
use std::time::Instant;

use noteliner_export::{ExportOptions, Exporter, Title};

fn main() {
    let mut args = std::env::args().skip(1);
    let (mut repeat, mut typ, mut paths) = (0usize, None, Vec::new());
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--repeat" => repeat = args.next().and_then(|n| n.parse().ok()).unwrap_or(0),
            "--typ" => typ = args.next().map(PathBuf::from),
            _ => paths.push(PathBuf::from(arg)),
        }
    }
    let [input, output] = paths.as_slice() else {
        eprintln!("usage: export [--repeat N] [--typ OUT.typ] <in.md> <out.pdf|out.html>");
        std::process::exit(2);
    };
    let raw = std::fs::read_to_string(input).expect("read the note");
    let name = input.file_stem().and_then(|s| s.to_str()).unwrap_or("Note");
    if output.extension().is_some_and(|e| e == "html") {
        std::fs::write(output, noteliner_export::export_html(name, &raw)).expect("write");
        return;
    }
    let options = ExportOptions {
        title: Title::Text(name.into()),
        base_dir: Some(
            input
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."))
                .to_path_buf(),
        ),
        ..Default::default()
    };
    if let Some(typ) = typ {
        std::fs::write(typ, noteliner_export::typst_source(&raw, &options)).expect("write .typ");
    }
    let faces = marina_egui::fonts::bundled_faces().iter().map(|(_, b)| *b);
    let t = Instant::now();
    let exporter = Exporter::new(faces);
    let fonts_ms = t.elapsed().as_secs_f64() * 1000.0;
    let t = Instant::now();
    let pdf = exporter
        .export(&raw, &options)
        .unwrap_or_else(|e| panic!("{e}"));
    let cold_ms = t.elapsed().as_secs_f64() * 1000.0;
    std::fs::write(output, &pdf).expect("write the PDF");
    println!(
        "fonts {fonts_ms:.1} ms, first export {cold_ms:.1} ms, {} KB",
        pdf.len() / 1024
    );
    let mut warm = Vec::new();
    for _ in 0..repeat {
        let t = Instant::now();
        exporter.export(&raw, &options).unwrap();
        warm.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    if !warm.is_empty() {
        warm.sort_by(f64::total_cmp);
        println!(
            "re-export: median {:.1} ms (n={})",
            warm[warm.len() / 2],
            warm.len()
        );
    }
}
