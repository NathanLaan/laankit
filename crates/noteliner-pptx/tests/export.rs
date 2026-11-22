//! The exported file, read back: every part is well-formed, every
//! relationship points at a part in the file, and the slides hold what the
//! deck holds.

use std::collections::{BTreeMap, HashSet};
use std::io::{Cursor, Read};

use noteliner_pptx::{export, needs, Assets, Format, Formula, Options, Picture};
use noteliner_slides::{parse_deck, Deck};
use serde_json::{json, Value};

type Parts = BTreeMap<String, Vec<u8>>;

fn deck(markdown: &str, config: Value) -> Deck {
    parse_deck(markdown, Some(&config))
}

fn unzip(bytes: &[u8]) -> Parts {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("a zip file");
    let mut out = Parts::new();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).unwrap();
        let mut body = Vec::new();
        f.read_to_end(&mut body).unwrap();
        assert!(
            out.insert(f.name().to_string(), body).is_none(),
            "a part is in the file twice"
        );
    }
    out
}

fn text(parts: &Parts, name: &str) -> String {
    String::from_utf8(
        parts
            .get(name)
            .unwrap_or_else(|| panic!("no {name}"))
            .clone(),
    )
    .unwrap()
}

/// A relationship's target as a part name.
fn resolve(part: &str, target: &str) -> String {
    let mut segs: Vec<&str> = part.split('/').collect();
    segs.pop();
    for seg in target.split('/') {
        match seg {
            ".." => {
                segs.pop();
            }
            "." | "" => {}
            s => segs.push(s),
        }
    }
    segs.join("/")
}

/// Everything a `.pptx` must get right for any program to open it.
fn check(bytes: &[u8]) -> Parts {
    let parts = unzip(bytes);
    assert_eq!(
        zip::ZipArchive::new(Cursor::new(bytes))
            .unwrap()
            .by_index(0)
            .unwrap()
            .name(),
        "[Content_Types].xml"
    );
    let types = text(&parts, "[Content_Types].xml");
    let types_doc = roxmltree::Document::parse(&types).unwrap();
    let defaults: HashSet<&str> = types_doc
        .descendants()
        .filter(|n| n.has_tag_name("Default"))
        .filter_map(|n| n.attribute("Extension"))
        .collect();
    let overrides: HashSet<&str> = types_doc
        .descendants()
        .filter(|n| n.has_tag_name("Override"))
        .filter_map(|n| n.attribute("PartName"))
        .collect();
    for o in &overrides {
        assert!(parts.contains_key(&o[1..]), "{o} has a type and no part");
    }
    for (name, body) in &parts {
        if name == "[Content_Types].xml" {
            continue;
        }
        let ext = name.rsplit('.').next().unwrap();
        assert!(defaults.contains(ext), "{name}: no type for .{ext}");
        if ext == "xml" {
            assert!(
                overrides.contains(format!("/{name}").as_str()),
                "{name} has no content type"
            );
        }
        if ext != "xml" && ext != "rels" {
            continue;
        }
        let xml = std::str::from_utf8(body).unwrap();
        let doc = roxmltree::Document::parse(xml)
            .unwrap_or_else(|e| panic!("{name} is not well-formed: {e}\n{xml}"));
        if ext == "rels" {
            // `ppt/slides/_rels/slide1.xml.rels` belongs to `ppt/slides/slide1.xml`.
            let owner = name
                .replace("_rels/", "")
                .trim_end_matches(".rels")
                .to_string();
            let mut ids = HashSet::new();
            for rel in doc.descendants().filter(|n| n.has_tag_name("Relationship")) {
                assert!(
                    ids.insert(rel.attribute("Id").unwrap()),
                    "{name}: an id twice"
                );
                if rel.attribute("TargetMode") == Some("External") {
                    continue;
                }
                let target = resolve(&owner, rel.attribute("Target").unwrap());
                assert!(
                    parts.contains_key(&target),
                    "{name}: {target} is not in the file"
                );
            }
        } else {
            // Every `r:id` and `r:embed` is a relationship of the part.
            let rels_name = match name.rsplit_once('/') {
                Some((dir, file)) => format!("{dir}/_rels/{file}.rels"),
                None => format!("_rels/{name}.rels"),
            };
            let known: HashSet<String> = parts
                .get(&rels_name)
                .map(|b| {
                    roxmltree::Document::parse(std::str::from_utf8(b).unwrap())
                        .unwrap()
                        .descendants()
                        .filter_map(|n| n.attribute("Id").map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let mut shape_ids = HashSet::new();
            for n in doc.descendants().filter(|n| n.is_element()) {
                for a in n.attributes() {
                    let is_rel = a.namespace()
                        == Some(
                            "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
                        );
                    if is_rel {
                        assert!(
                            known.contains(a.value()),
                            "{name}: {} is unknown",
                            a.value()
                        );
                    }
                }
                if n.tag_name().name() == "cNvPr" {
                    assert!(
                        shape_ids.insert(n.attribute("id").unwrap()),
                        "{name}: a shape id twice"
                    );
                }
                if n.tag_name().name() == "ext" && n.has_attribute("cx") {
                    for key in ["cx", "cy"] {
                        let v: i64 = n.attribute(key).unwrap().parse().unwrap();
                        assert!(v >= 0, "{name}: a negative size");
                    }
                }
            }
        }
    }
    parts
}

fn slide_texts(parts: &Parts, n: usize) -> Vec<String> {
    let xml = text(parts, &format!("ppt/slides/slide{n}.xml"));
    let doc = roxmltree::Document::parse(&xml).unwrap();
    doc.descendants()
        .filter(|n| {
            n.tag_name().name() == "p"
                && n.tag_name()
                    .namespace()
                    .is_some_and(|ns| ns.ends_with("drawingml/2006/main"))
        })
        .map(|p| {
            p.descendants()
                .filter(|n| n.tag_name().name() == "t")
                .filter_map(|n| n.text())
                .collect::<String>()
        })
        .filter(|t| !t.is_empty())
        .collect()
}

fn title(parts: &Parts, n: usize) -> Option<String> {
    let xml = text(parts, &format!("ppt/slides/slide{n}.xml"));
    let doc = roxmltree::Document::parse(&xml).unwrap();
    let sp = doc.descendants().find(|n| {
        n.tag_name().name() == "sp"
            && n.descendants()
                .any(|d| d.tag_name().name() == "ph" && d.attribute("type") == Some("title"))
    })?;
    Some(
        sp.descendants()
            .filter(|n| n.tag_name().name() == "t")
            .filter_map(|n| n.text())
            .collect(),
    )
}

const DECK: &str =
    "# Kickoff\n\nA plan for *everyone*\n\n<!-- notes\nSay hello.\nThen start.\n-->\n\n\
## Agenda\n\n- One **bold**\n- Two\n  1. Nested\n  2. Again\n- [x] Done\n\n\
## Numbers\n\n| Quarter | Revenue |\n|---|--:|\n| Q1 | 1 < 2 & 3 |\n\n\
## Code\n\n```rust\nfn main() {\n\tlet a = \"<&>\";\n}\n```\n\n\
## Quote\n\n> Keep it simple.\n\n\
## Links\n\nSee [the site](https://example.com/a?b=1&c=2) and `code` and ~~gone~~.\n";

#[test]
fn a_deck_becomes_a_well_formed_file() {
    let d = deck(
        DECK,
        json!({ "theme": "dark", "slideNumbers": true, "footer": "{{name}} {{page}}/{{total}}" }),
    );
    let options = Options {
        title: "Kickoff & <Plan>".into(),
        vars: [("name".to_string(), "Kickoff".to_string())].into(),
        created: Some("2026-11-21T10:00:00Z".into()),
        ..Default::default()
    };
    let out = export(&d, &Assets::default(), &options).unwrap();
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    let parts = check(&out.bytes);
    assert_eq!(d.slides.len(), 6);
    for n in 1..=6 {
        assert!(parts.contains_key(&format!("ppt/slides/slide{n}.xml")));
        assert!(parts.contains_key(&format!("ppt/notesSlides/notesSlide{n}.xml")));
    }
    assert!(!parts.contains_key("ppt/slides/slide7.xml"));
    assert_eq!(title(&parts, 1).as_deref(), Some("Kickoff"));
    assert_eq!(title(&parts, 2).as_deref(), Some("Agenda"));

    let pres = text(&parts, "ppt/presentation.xml");
    assert!(
        pres.contains("<p:sldSz cx=\"12192000\" cy=\"6858000\"/>"),
        "{pres}"
    );
    assert_eq!(pres.matches("<p:sldId ").count(), 6);

    // Notes, a line each.
    let notes = text(&parts, "ppt/notesSlides/notesSlide1.xml");
    assert!(notes.contains("<a:t>Say hello.</a:t>") && notes.contains("<a:t>Then start.</a:t>"));

    // Lists: bullets, numbers that say their own number, and a task box.
    let agenda = text(&parts, "ppt/slides/slide2.xml");
    assert_eq!(agenda.matches("<a:buChar").count(), 3, "{agenda}");
    assert!(agenda.contains("startAt=\"1\"") && agenda.contains("startAt=\"2\""));
    assert!(agenda.contains("☑ "));
    assert!(agenda.contains("b=\"1\"") && agenda.contains("<a:t>bold</a:t>"));

    // A real table, with escaped text and a right-aligned column.
    let numbers = text(&parts, "ppt/slides/slide3.xml");
    assert!(numbers.contains("<a:tbl>") && numbers.contains("firstRow=\"1\""));
    assert_eq!(numbers.matches("<a:tc>").count(), 4);
    assert!(numbers.contains("1 &lt; 2 &amp; 3"));
    assert!(numbers.contains("algn=\"r\""));

    // Code keeps its lines and its spaces, in the monospace font.
    let code = slide_texts(&parts, 4);
    assert!(
        code.contains(&"    let a = \"<&>\";".to_string()),
        "{code:?}"
    );
    assert!(text(&parts, "ppt/slides/slide4.xml").contains("typeface=\"JetBrains Mono\""));

    // A link is a relationship to the address.
    let rels = text(&parts, "ppt/slides/_rels/slide6.xml.rels");
    assert!(
        rels.contains("Target=\"https://example.com/a?b=1&amp;c=2\" TargetMode=\"External\""),
        "{rels}"
    );
    assert!(text(&parts, "ppt/slides/slide6.xml").contains("strike=\"sngStrike\""));

    // The footer's placeholders are filled in, and the number is a field.
    let first = text(&parts, "ppt/slides/slide1.xml");
    assert!(first.contains("<a:t>Kickoff 1/6</a:t>"), "{first}");
    assert!(first.contains("type=\"slidenum\""));

    // The theme's colours and fonts.
    let theme = text(&parts, "ppt/theme/theme1.xml");
    assert!(theme.contains("typeface=\"Inter\""));
    let core = text(&parts, "docProps/core.xml");
    assert!(core.contains("Kickoff &amp; &lt;Plan&gt;") && core.contains("2026-11-21T10:00:00Z"));
}

#[test]
fn the_same_deck_gives_the_same_bytes() {
    let d = deck(DECK, json!({}));
    let a = export(&d, &Assets::default(), &Options::default()).unwrap();
    let b = export(&d, &Assets::default(), &Options::default()).unwrap();
    assert_eq!(a.bytes, b.bytes);
}

#[test]
fn slide_sizes_follow_the_aspect() {
    for (aspect, size) in [
        ("16:9", "cx=\"12192000\" cy=\"6858000\""),
        ("16:10", "cx=\"12192000\" cy=\"7620000\""),
        ("4:3", "cx=\"9144000\" cy=\"6858000\""),
    ] {
        let d = deck("# A\n", json!({ "aspect": aspect }));
        let parts = check(
            &export(&d, &Assets::default(), &Options::default())
                .unwrap()
                .bytes,
        );
        assert!(
            text(&parts, "ppt/presentation.xml").contains(&format!("<p:sldSz {size}/>")),
            "{aspect}"
        );
    }
}

/// A 1 by 1 PNG.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

fn png(w: f32, h: f32) -> Picture {
    Picture {
        bytes: PNG.to_vec(),
        format: Format::sniff(PNG).unwrap(),
        natural: (w, h),
    }
}

const PICTURES: &str = "![Wide](_attachments/wide.png)\n\n---\n\n\
## Titled\n\n![Wide](_attachments/wide.png)\n\n---\n\n\
## Split\n\nSome text beside it.\n\n![Tall](_attachments/tall.png)\n\n---\n\n\
## Gone\n\n![Not there](_attachments/gone.png)\n\n---\n\n\
## Math\n\n$$E = mc^2$$\n\n---\n\n\
## Video\n\nhttps://youtu.be/abc123\n";

#[test]
fn pictures_formulas_and_videos() {
    let d = deck(PICTURES, json!({ "slideLevel": 2 }));
    let n = needs(&d);
    assert_eq!(
        n.pictures,
        [
            "_attachments/wide.png",
            "_attachments/tall.png",
            "_attachments/gone.png"
        ]
    );
    assert_eq!(n.formulas.len(), 1);
    assert_eq!(n.formulas[0].latex, "E = mc^2");

    let mut assets = Assets::default();
    assets
        .pictures
        .insert("_attachments/wide.png".into(), png(3200.0, 900.0));
    assets
        .pictures
        .insert("_attachments/tall.png".into(), png(400.0, 1600.0));
    assets
        .formulas
        .insert(n.formulas[0].clone(), png(300.0, 60.0));
    let out = export(&d, &assets, &Options::default()).unwrap();
    let parts = check(&out.bytes);
    assert_eq!(out.warnings.len(), 1, "{:?}", out.warnings);
    assert!(
        out.warnings[0].starts_with("Slide 4: "),
        "{:?}",
        out.warnings
    );

    // The picture used twice is in the file once.
    let media: Vec<&String> = parts
        .keys()
        .filter(|k| k.starts_with("ppt/media/"))
        .collect();
    assert_eq!(media.len(), 3, "{media:?}");

    // Full bleed: the whole slide, with the sides cut to fit.
    let full = text(&parts, "ppt/slides/slide1.xml");
    assert!(
        full.contains("<a:off x=\"0\" y=\"0\"/><a:ext cx=\"12192000\" cy=\"6858000\"/>"),
        "{full}"
    );
    assert!(
        full.contains("<a:srcRect l=\"25000\" t=\"0\" r=\"25000\" b=\"0\"/>"),
        "{full}"
    );
    assert!(full.contains("descr=\"Wide\""));

    // Contained: never wider than the room, and the same shape as the picture.
    let titled = text(&parts, "ppt/slides/slide2.xml");
    let doc = roxmltree::Document::parse(&titled).unwrap();
    let pic = doc
        .descendants()
        .find(|n| n.tag_name().name() == "pic")
        .unwrap();
    let ext = pic
        .descendants()
        .find(|n| n.tag_name().name() == "ext")
        .unwrap();
    let (cx, cy): (f64, f64) = (
        ext.attribute("cx").unwrap().parse().unwrap(),
        ext.attribute("cy").unwrap().parse().unwrap(),
    );
    assert!(cx <= (1920.0 - 240.0) * 6350.0 + 1.0);
    assert!((cx / cy - 3200.0 / 900.0).abs() < 0.01, "{cx} {cy}");

    // A missing picture is a box with its alt text.
    let gone = slide_texts(&parts, 4);
    assert!(gone.contains(&"Not there".to_string()), "{gone:?}");
    // The formula is a picture that says its source.
    assert!(text(&parts, "ppt/slides/slide5.xml").contains("descr=\"E = mc^2\""));
    // A video is a link.
    let rels = text(&parts, "ppt/slides/_rels/slide6.xml.rels");
    assert!(rels.contains("https://youtu.be/abc123"), "{rels}");
}

#[test]
fn a_formula_without_a_picture_shows_its_source() {
    let d = deck("## Math\n\n$$E = mc^2$$\n", json!({}));
    let out = export(&d, &Assets::default(), &Options::default()).unwrap();
    let parts = check(&out.bytes);
    assert!(slide_texts(&parts, 1)
        .iter()
        .any(|t| t.contains("E = mc^2")));
    let _ = Formula {
        latex: String::new(),
        size: 0,
        color: [0; 3],
    };
}

#[test]
fn directives_colours_and_text_xml_cannot_hold() {
    let d = deck(
        "## One\n<!-- slide: bg=#3b2a5c header=\"Top <&> {{page}}\" -->\n\nText with a \u{1} control, \u{202E}right to left, and 😀.\n\n---\n\n<!-- slide: class=theme-light -->\n\n## Two\n\nLight.\n",
        json!({ "theme": "dark" }),
    );
    let out = export(&d, &Assets::default(), &Options::default()).unwrap();
    let parts = check(&out.bytes);
    let one = text(&parts, "ppt/slides/slide1.xml");
    assert!(
        one.contains("<p:bg><p:bgPr><a:solidFill><a:srgbClr val=\"3B2A5C\"/>"),
        "{one}"
    );
    assert!(one.contains("Top &lt;&amp;&gt; 1"));
    assert!(one.contains("😀") && !one.contains('\u{1}'));
    let two = text(&parts, "ppt/slides/slide2.xml");
    assert!(!two.contains("3B2A5C"));
    assert_ne!(
        one.split("<p:spTree>").next(),
        two.split("<p:spTree>").next(),
        "a slide's own theme sets its background"
    );
}

#[test]
fn an_empty_deck_is_still_a_file() {
    let d = deck("", json!({}));
    let out = export(&d, &Assets::default(), &Options::default()).unwrap();
    check(&out.bytes);
}

#[test]
fn every_shape_is_on_the_slide() {
    let long = "word ".repeat(400);
    let md = format!(
        "## Long\n\n{long}\n\n- {long}\n\n```\n{}\n```\n\n| a | b |\n|---|---|\n{}",
        "line\n".repeat(60),
        "| 1 | 2 |\n".repeat(40)
    );
    for layout in [
        "auto",
        "hero",
        "title-body",
        "split",
        "grid",
        "full-bleed",
        "quote",
        "table",
    ] {
        let d = deck(
            &format!("<!-- slide: layout={layout} -->\n{md}"),
            json!({ "header": "H", "footer": "F", "slideNumbers": true }),
        );
        let out = export(&d, &Assets::default(), &Options::default()).unwrap();
        let parts = check(&out.bytes);
        let xml = text(&parts, "ppt/slides/slide1.xml");
        let doc = roxmltree::Document::parse(&xml).unwrap();
        for off in doc.descendants().filter(|n| n.tag_name().name() == "off") {
            let ext = off.next_sibling_element().unwrap();
            let get = |n: roxmltree::Node, k: &str| n.attribute(k).unwrap().parse::<i64>().unwrap();
            let (x, y, w, h) = (get(off, "x"), get(off, "y"), get(ext, "cx"), get(ext, "cy"));
            assert!(x >= 0 && y >= 0, "{layout}: {x},{y}");
            assert!(x + w <= 12192000 + 10, "{layout}: right edge {}", x + w);
            assert!(y + h <= 6858000 + 10, "{layout}: bottom edge {}", y + h);
        }
    }
}

/// A small deterministic PRNG (xorshift64*).
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) % n as u64) as usize
    }
}

const LINES: &[&str] = &[
    "",
    "",
    "# Title",
    "## Alpha",
    "### Sub point",
    "---",
    "Plain paragraph text.",
    "Second line of a paragraph",
    "- bullet",
    "  - nested bullet",
    "1. first",
    "2) second",
    "- [ ] task",
    "> a quote",
    "> - a list in a quote",
    "```",
    "```js",
    "let a = `<b>` & 1;",
    "| a | b |",
    "|---|:-:|",
    "| 1 | 2 |",
    "$$",
    "x = 1",
    "inline $a^2$ math",
    "![alt text](img/photo.png)",
    "![clip](media/demo.mp4)",
    "https://youtu.be/abc123",
    "<!-- notes: Say hello. -->",
    "<!-- skip -->",
    "<!-- slide: layout=split -->",
    "<!-- slide: layout=grid bg=#101418 -->",
    "<!-- slide: layout=hero -->",
    "<!-- slide: layout=full-bleed -->",
    "<!-- slide: layout=quote -->",
    "<!-- slide: layout=table -->",
    "<!-- slide: notes-only -->",
    "<!-- slide: class=\"theme-light layout-grid\" header='Top' -->",
    "<!-- slide: footer=\"{{page}}/{{total}}\" bg=rgba(0,0,0,0.5) -->",
    "<mark>marked</mark> <kbd>Ctrl</kbd> H<sub>2</sub>O x<sup>2</sup> <u>under</u>",
    "<svg width=\"10\" height=\"10\"><rect width=\"10\" height=\"10\"/></svg>",
    "[a link](https://example.com/?q=<x>&y=\"z\") and [bad](javascript:alert(1)) and [[Wiki]]",
    "Ünïcödé wörds — ’quoted’ 12 \u{202E}rtl\u{202C} 😀 \u{7}bell ]]> &amp; &#0; <not-a-tag",
];

const CONFIGS: &[&str] = &[
    "{}",
    r#"{"slideLevel": 1}"#,
    r#"{"slideLevel": 3, "theme": "light"}"#,
    r#"{"aspect": "4:3", "slideNumbers": true, "header": "{{name}}"}"#,
    r#"{"aspect": "16:10", "footer": "<&>", "theme": "darkPurple"}"#,
    r#"{"firstSlideTitle": false, "theme": "nope"}"#,
];

#[test]
fn generated_decks_export_to_well_formed_files() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut slides = 0;
    for _ in 0..400 {
        let len = rng.below(30);
        let md: Vec<&str> = (0..len).map(|_| LINES[rng.below(LINES.len())]).collect();
        let md = md.join("\n");
        let config: Value = serde_json::from_str(CONFIGS[rng.below(CONFIGS.len())]).unwrap();
        let d = deck(&md, config);
        let mut assets = Assets::default();
        let n = needs(&d);
        // Half the time the pictures are there.
        if rng.below(2) == 0 {
            for src in n.pictures {
                assets.pictures.insert(src, png(800.0, 600.0));
            }
            for f in n.formulas {
                assets.formulas.insert(f, png(200.0, 50.0));
            }
        }
        let out = export(&d, &assets, &Options::default()).unwrap_or_else(|e| panic!("{e}\n{md}"));
        check(&out.bytes);
        slides += d.slides.len();
    }
    assert!(slides > 400, "the generator made {slides} slides");
}
