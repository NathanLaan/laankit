//! End-to-end exports with the fonts the app bundles, checked by opening and
//! rendering the result with hayro (the PDF renderer PageLiner uses).

#![cfg(feature = "pdf")]

use std::path::Path;
use std::sync::{Arc, OnceLock};

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{PixmapSettings, RenderCache, RenderSettings};
use noteliner_export::{markup, ExportOptions, Exporter, LazyExporter, Title, WikiLinks};

fn faces() -> impl Iterator<Item = &'static [u8]> {
    laankit_egui::fonts::bundled_faces()
        .iter()
        .map(|(_, bytes)| *bytes)
}

/// One exporter for the whole test binary, as the app keeps one.
fn exporter() -> &'static Exporter {
    static EXPORTER: OnceLock<Exporter> = OnceLock::new();
    EXPORTER.get_or_init(|| Exporter::new(faces()))
}

fn fixtures() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures"))
}

/// Opens `pdf` with hayro and renders every page; returns the page count and,
/// per page, the share of pixels that aren't white.
fn render(pdf: &[u8]) -> (usize, Vec<f32>) {
    assert!(pdf.starts_with(b"%PDF-"), "not a PDF");
    let pdf = Pdf::new(Arc::new(pdf.to_vec())).expect("hayro can read the PDF");
    let settings = InterpreterSettings::default();
    let cache = RenderCache::new();
    let ink = pdf
        .pages()
        .iter()
        .map(|page| {
            let pixmap = hayro::render(
                page,
                &cache,
                &settings,
                &RenderSettings::default(),
                &PixmapSettings {
                    x_scale: 0.5,
                    y_scale: 0.5,
                    bg_color: WHITE,
                },
            );
            let px = pixmap.data_as_u8_slice();
            let inked = px
                .chunks(4)
                .filter(|p| p[0] < 200 || p[1] < 200 || p[2] < 200)
                .count();
            inked as f32 / (px.len() / 4) as f32
        })
        .collect();
    (pdf.pages().len(), ink)
}

fn options(title: &str, base: Option<&Path>) -> ExportOptions {
    ExportOptions {
        title: Title::Text(title.into()),
        base_dir: base.map(Path::to_path_buf),
        ..Default::default()
    }
}

#[test]
fn rich_note_exports_and_renders() {
    let raw = std::fs::read_to_string(fixtures().join("Rich Note.md")).unwrap();
    let pdf = exporter()
        .export(&raw, &options("Rich Note", Some(fixtures())))
        .unwrap();
    let (pages, ink) = render(&pdf);
    assert!(pages >= 2, "expected several pages, got {pages}");
    assert!(ink.iter().all(|&i| i > 0.005), "a blank page: {ink:?}");
    // The bundled faces are embedded (names are in the font dictionaries).
    let bytes = String::from_utf8_lossy(&pdf);
    assert!(bytes.contains("Inter"), "Inter isn't embedded");
    assert!(
        bytes.contains("JetBrainsMono"),
        "JetBrains Mono isn't embedded"
    );
}

#[test]
fn every_markdown_feature_compiles() {
    let md = "\
---
id: x
tags: [a]
---
# Heading 1
## Heading 2
###### Heading 6

Text with **bold**, *italic*, ~~strike~~, `code`, a [link](https://example.com),
an autolink https://example.com/a_b and a line\\
break.

- bullet
  - nested
    1. ordered
- [ ] todo
- [x] done

1. one
2. two

> quote
>> nested quote

```rust
fn main() { println!(\"{}\", 1); }
```

    indented code

| left | center | right |
|:-----|:------:|------:|
| 1 | 2 | 3 |
| a | | |

See [[Other Note]] and [[Real Name|label]].

Footnote[^1] and again[^1].

[^1]: The footnote.

![missing](_attachments/none.png)

H<sub>2</sub>O, x<sup>2</sup>, <kbd>Ctrl</kbd>+<kbd>K</kbd>, <mark>marked</mark>,
<u>under</u>, <b>bold</b>, <span>dropped tag</span>, unclosed <sup>tag.

<div>
HTML block text
</div>

---
";
    let src = noteliner_export::typst_source(md, &options("Features", None));
    for needle in [
        "#sub[2]",
        "#super[2]",
        "#md-kbd[Ctrl]",
        "#highlight[marked]",
        "#underline[under]",
        "#strong[bold]",
        "dropped tag",
        "unclosed tag",
        "HTML block text",
        "#md-wikilink[Other Note]",
    ] {
        assert!(src.contains(needle), "missing {needle:?} in:\n{src}");
    }
    assert!(!src.contains("tags:"), "frontmatter leaked");
    let pdf = exporter().export(md, &options("Features", None)).unwrap();
    let (pages, _) = render(&pdf);
    assert!(pages >= 1);
}

#[test]
fn wikilinks_can_be_links() {
    let opts = ExportOptions {
        wikilinks: WikiLinks::Url("noteliner://note/{name}".into()),
        ..options("Links", None)
    };
    let src = noteliner_export::typst_source("See [[Other Note|it]].", &opts);
    assert!(
        src.contains("#link(\"noteliner://note/Other%20Note\")[it]"),
        "{src}"
    );
    exporter().export("See [[Other Note|it]].", &opts).unwrap();
}

#[test]
fn missing_images_become_placeholders() {
    let md = "![Cat](_attachments/none.png)\n\n![Real](_attachments/att-gradient.png)";
    let src = noteliner_export::typst_source(md, &options("Images", Some(fixtures())));
    assert!(src.contains("image\\: Cat"), "{src}");
    assert!(
        src.contains("#md-image(\"/_attachments/att-gradient.png\""),
        "{src}"
    );
    exporter()
        .export(md, &options("Images", Some(fixtures())))
        .unwrap();
}

/// CJK and emoji aren't in the bundled faces, so they come from system fonts
/// (when the machine has any).
#[test]
fn cjk_and_emoji_fall_back_to_system_fonts() {
    let md = "日本語のテキスト、中文文本。Emoji: 😀 🎉 👍";
    let pdf = exporter().export(md, &options("CJK 日本語", None)).unwrap();
    let (pages, ink) = render(&pdf);
    assert_eq!(pages, 1);
    assert!(ink[0] > 0.0);

    // The same note without the fallback has only the bundled faces to embed.
    let bundled_only = Exporter::new(faces()).with_system_fallback(false);
    let without = bundled_only
        .export(md, &options("CJK 日本語", None))
        .unwrap();
    // The fonts a PDF embeds, by base name (`/BaseFont /ABCDEF+Name`).
    let names = |pdf: &[u8]| -> std::collections::BTreeSet<String> {
        let text = String::from_utf8_lossy(pdf);
        text.split("/BaseFont")
            .skip(1)
            .map(|rest| {
                let name: String = rest
                    .trim_start_matches([' ', '/'])
                    .chars()
                    .take_while(|c| !c.is_whitespace() && *c != '/' && *c != '>')
                    .collect();
                name.split_once('+')
                    .map_or(name.clone(), |(_, n)| n.to_string())
            })
            .collect()
    };
    // Whether the fallback can find a CJK font at all, asked of the same scan
    // the exporter uses (not `fc-list`, which only exists on Linux: Windows
    // and macOS runners may have no CJK font Typst can reach).
    let has_cjk_font =
        typst_kit::fonts::system().any(|(_, info)| info.coverage.contains('日' as u32));
    if has_cjk_font {
        // With the fallback a system font is embedded that the bundled-only PDF lacks. (Without
        // it, Typst draws the missing glyphs' boxes with one of the bundled faces, so counting
        // fonts isn't enough.)
        let (with, without) = (names(&pdf), names(&without));
        assert!(
            with.difference(&without).next().is_some(),
            "no extra font embedded for CJK: {with:?} vs {without:?}"
        );
    } else {
        eprintln!("no system CJK font; only checked that the export succeeds");
    }
}

#[test]
fn lazy_exporter_builds_in_the_background() {
    let lazy = LazyExporter::spawn(faces());
    let pdf = lazy.get().export("Hello", &options("Lazy", None)).unwrap();
    assert!(pdf.starts_with(b"%PDF-"));
    assert!(lazy.try_get().is_some());
}

#[test]
fn deep_nesting_and_long_words() {
    let mut md = String::new();
    for depth in 0..40 {
        md.push_str(&"  ".repeat(depth));
        md.push_str("- level\n");
    }
    md.push_str(&format!("\n{}\n", "x".repeat(600)));
    md.push_str(&"> ".repeat(40));
    md.push_str("deep quote\n");
    exporter().export(&md, &options("Deep", None)).unwrap();
}

/// Random Markdown built from block markers, inline syntax, HTML tags and
/// characters special to Typst must always compile: escaping or structure
/// bugs show up as Typst errors.
#[test]
fn fuzz_random_markdown_compiles() {
    const PIECES: &[&str] = &[
        "# ",
        "###### ",
        "- ",
        "  - ",
        "1. ",
        "3) ",
        "> ",
        ">> ",
        "- [ ] ",
        "- [x] ",
        "```\n",
        "```rust\n",
        "~~~",
        "    ",
        "| a | b |\n|---|:-:|\n| 1 | 2 |\n",
        "[[",
        "]]",
        "|",
        "[^n]",
        "\n[^n]: note\n",
        "**",
        "*",
        "_",
        "~~",
        "`",
        "![x](_attachments/a.png)",
        "[l](http://x.y/a_b)",
        "<b>",
        "</b>",
        "<sup>",
        "</sup>",
        "<kbd>",
        "</kbd>",
        "<mark>",
        "</mark>",
        "<br>",
        "<div>",
        "\n",
        "\n\n",
        "\\",
        "#",
        "$",
        "[",
        "]",
        "<lbl>",
        "@r",
        "=",
        "+",
        "/",
        "~",
        "...",
        "--",
        "'",
        "\"",
        ":",
        "{",
        "}",
        "é😀",
        "日本",
        "word ",
        "---\n",
        "***\n",
        "Term\n: def\n",
        "&amp;",
        "&#35;",
        "\t",
    ];
    let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for case in 0..150 {
        let len = 5 + (next() % 60) as usize;
        let md: String = (0..len)
            .map(|_| PIECES[(next() % PIECES.len() as u64) as usize])
            .collect();
        if let Err(e) = exporter().export(&md, &options("Fuzz #*_[]", None)) {
            panic!(
                "case {case}: {e}\nmarkdown: {md:?}\ntypst:\n{}",
                markup::markdown_to_typst(&md, &markup::NoImages)
            );
        }
    }
}
