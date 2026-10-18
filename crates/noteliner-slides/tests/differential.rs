//! Runs the Rust port and the JS originals (`lib/slides.js`,
//! `lib/slideEdits.js`) on the same generated notes and edits, and requires
//! identical output: decks as JSON, and rewritten Markdown plus caret line.
//!
//! Skipped when `node` isn't on the PATH.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use noteliner_slides::deck::parse_deck;
use noteliner_slides::edits::{self, Direction};
use serde_json::{json, Value};

/// Lines notes are built from: every construct the parser distinguishes, and
/// the near-misses around each.
const LINES: &[&str] = &[
    "",
    "",
    "",
    "# Title",
    "## Alpha",
    "## Beta ##",
    "### Sub point",
    "#### Deep",
    "####### seven",
    "##NoSpace",
    "## C#",
    "  ## Indented",
    "    ## Code-indented",
    "---",
    "-----",
    "--- ",
    "   ---",
    "--",
    "***",
    "Plain paragraph text.",
    "Revenue",
    "Second line of a paragraph",
    "- bullet",
    "* star item",
    "+ plus item",
    "1. first",
    "2) second",
    "> a quote",
    ">nested",
    "```",
    "```js",
    "~~~",
    "````",
    "| a | b |",
    "|---|:-:|",
    "| 1 | 2 |",
    "text | with pipe",
    "$$",
    "x = 1",
    "![alt text](img/photo.png)",
    "![clip](media/demo.mp4)",
    "![t](pic.jpg \"title\")",
    "https://youtu.be/abc123",
    "https://www.youtube.com/watch?v=xyz",
    "<!-- notes",
    "<!-- notes: Say hello. -->",
    "<!--notes-->",
    "<!-- NOTES upper case -->",
    "-->",
    "more notes text",
    "<!-- skip -->",
    "<!-- skip",
    "<!-- slide: layout=split -->",
    "<!-- slide: layout=grid bg=#101418 -->",
    "<!-- slide: notes-only -->",
    "<!-- slide: notes-only=true -->",
    "<!-- slide: class=\"big quote\" header='Top' layout -->",
    "<!-- slide: layout=hero",
    "<!-- slide: layout=bogus -->",
    "<!-- slide: footer=\"{{page}}/{{total}}\" -->",
    "<!-- slides: nope -->",
    "key: value",
    "presentation:",
    "  theme: dark",
    "Ünïcödé wörds — ’quoted’ 12",
];

const PRESENTATIONS: &[&str] = &[
    "null",
    "true",
    "false",
    "{}",
    r#"{"slideLevel": 0}"#,
    r#"{"slideLevel": 1}"#,
    r#"{"slideLevel": 3}"#,
    r#"{"slideLevel": "2"}"#,
    r#"{"slideLevel": 9}"#,
    r#"{"slideLevel": null}"#,
    r#"{"slideLevel": 2.5}"#,
    r#"{"firstSlideTitle": false}"#,
    r#"{"theme": " light ", "aspect": "4:3"}"#,
    r#"{"aspect": "21:9", "slideNumbers": 0, "header": "{{name}}"}"#,
    r#""yes""#,
];

/// A small deterministic PRNG (xorshift64*).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.below(items.len())]
    }
}

fn note(rng: &mut Rng) -> String {
    let len = rng.below(28);
    let mut lines: Vec<&str> = (0..len).map(|_| rng.pick(LINES)).collect();
    // Some notes start with frontmatter, real or fake.
    let front: &[&str] = match rng.below(8) {
        0 => &["---", "presentation:", "  theme: dark", "---"],
        1 => &["---", "Not yaml", "---"],
        _ => &[],
    };
    lines.splice(0..0, front.iter().copied());
    let joined = lines.join("\n");
    if rng.below(5) == 0 {
        joined.replace('\n', "\r\n")
    } else {
        joined
    }
}

fn js_lib(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/noteliner/src/renderer/lib")
        .join(name)
        .canonicalize()
        .expect("JS original")
}

fn node_available() -> bool {
    Command::new("node").arg("--version").output().is_ok()
}

/// Runs `cases` through the JS originals; one JSON result per case.
fn run_js(cases: &[Value]) -> Vec<Value> {
    let script = format!(
        r#"import * as S from "file://{slides}";
import * as E from "file://{edits}";
let input = "";
process.stdin.setEncoding("utf8");
process.stdin.on("data", (d) => (input += d));
process.stdin.on("end", () => {{
  const out = JSON.parse(input).map(({{ op, md, pres, args }}) => {{
    const deck = S.parseDeck(md, pres);
    switch (op) {{
      case "parse": return deck;
      case "insert": return E.insertSlide(md, deck, args[0]);
      case "duplicate": return E.duplicateSlide(md, deck, args[0]);
      case "delete": return E.deleteSlide(md, deck, args[0]);
      case "reorder": return E.reorderSlide(md, deck, args[0], args[1]);
      case "move": return E.moveSlide(md, deck, args[0], args[1]);
      case "merge": return E.mergeIntoPrevious(md, deck, args[0]);
      case "split": return E.splitAtLine(md, deck, args[0]);
      case "retitle": return E.retitleSlide(md, deck, args[0], args[1]);
      case "layout": return E.setSlideLayout(md, deck, args[0], args[1]);
      case "notes": return E.toggleNotes(md, args[0], args[1]);
      case "duration": return S.estimateDuration(deck);
      case "atLine": return S.slideAtLine(deck, args[0])?.index ?? null;
    }}
  }});
  process.stdout.write(JSON.stringify(out));
}});
"#,
        slides = js_lib("slides.js").display(),
        edits = js_lib("slideEdits.js").display(),
    );
    let mut child = Command::new("node")
        .args(["--input-type=module", "-e", &script])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run node");
    let input = serde_json::to_vec(cases).unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let out = child.wait_with_output().expect("node output");
    writer.join().unwrap().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("JSON from node")
}

/// The Rust side of one case, as the JS would return it.
fn run_rust(case: &Value) -> Value {
    let md = case["md"].as_str().unwrap();
    let pres = &case["pres"];
    let deck = parse_deck(md, Some(pres));
    let args = case["args"].as_array().cloned().unwrap_or_default();
    let n = |i: usize| args.get(i).and_then(Value::as_u64).unwrap_or(0) as usize;
    let s = |i: usize| {
        args.get(i)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let r = match case["op"].as_str().unwrap() {
        "parse" => return deck.to_json(),
        "duration" => {
            let (words, minutes) = noteliner_slides::estimate_duration(&deck, 130.0);
            return json!({ "words": words, "minutes": minutes });
        }
        "atLine" => {
            return noteliner_slides::slide_at_line(&deck, n(0))
                .map_or(Value::Null, |s| json!(s.index))
        }
        "insert" => {
            let o = &args[0];
            let after = o["after"].as_u64().map(|a| a as usize);
            let title = o["title"].as_str().unwrap_or("");
            let body = o["body"].as_str().unwrap_or("");
            edits::insert_slide(md, &deck, after, title, body)
        }
        "duplicate" => edits::duplicate_slide(md, &deck, n(0)),
        "delete" => edits::delete_slide(md, &deck, n(0)),
        "reorder" => edits::reorder_slide(md, &deck, n(0), n(1)),
        "move" => {
            let dir = if s(1) == "up" {
                Direction::Up
            } else {
                Direction::Down
            };
            edits::move_slide(md, &deck, n(0), dir)
        }
        "merge" => edits::merge_into_previous(md, &deck, n(0)),
        "split" => edits::split_at_line(md, &deck, n(0)),
        "retitle" => edits::retitle_slide(md, &deck, n(0), &s(1)),
        "layout" => edits::set_slide_layout(md, &deck, n(0), args[1].as_str()),
        "notes" => edits::toggle_notes(md, n(0), n(1)),
        op => panic!("unknown op {op}"),
    };
    serde_json::to_value(r).unwrap()
}

fn cases(seed: u64, count: usize) -> Vec<Value> {
    let mut rng = Rng(seed);
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let md = note(&mut rng);
        let pres: Value = serde_json::from_str(rng.pick(PRESENTATIONS)).unwrap();
        let slides = parse_deck(&md, Some(&pres)).slides.len();
        let lines = md.split('\n').count();
        let idx = |rng: &mut Rng| rng.below(slides + 2);
        let line = |rng: &mut Rng| rng.below(lines + 3);
        let (op, args) = match rng.below(14) {
            0..=2 => ("parse", json!([])),
            3 => {
                let after = match rng.below(4) {
                    0 => Value::Null,
                    _ => json!(idx(&mut rng)),
                };
                let title = rng.pick(&["", "New Point", "#Hash"]);
                let body = rng.pick(&["", "- one\n- two", "Text"]);
                (
                    "insert",
                    json!([{ "after": after, "title": title, "body": body }]),
                )
            }
            4 => ("duplicate", json!([idx(&mut rng)])),
            5 => ("delete", json!([idx(&mut rng)])),
            6 => ("reorder", json!([idx(&mut rng), idx(&mut rng)])),
            7 => ("move", json!([idx(&mut rng), rng.pick(&["up", "down"])])),
            8 => ("merge", json!([idx(&mut rng)])),
            9 => ("split", json!([line(&mut rng)])),
            10 => (
                "retitle",
                json!([idx(&mut rng), rng.pick(&["", "Renamed", "  spaced  "])]),
            ),
            11 => {
                let layout = match rng.below(5) {
                    0 => Value::Null,
                    1 => json!("auto"),
                    2 => json!(""),
                    3 => json!("split"),
                    _ => json!("full-bleed"),
                };
                ("layout", json!([idx(&mut rng), layout]))
            }
            12 => ("notes", json!([line(&mut rng), line(&mut rng)])),
            _ => match rng.below(2) {
                0 => ("duration", json!([])),
                _ => ("atLine", json!([line(&mut rng)])),
            },
        };
        out.push(json!({ "op": op, "md": md, "pres": pres, "args": args }));
    }
    out
}

/// JSON equality with numbers compared by value (JS prints `0`, Rust `0.0`;
/// see below for the last digit) and object keys in any order.
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        // serde_json's default float parsing can be one ULP off what JS printed.
        (Value::Number(x), Value::Number(y)) => match (x.as_f64(), y.as_f64()) {
            (Some(p), Some(q)) => (p - q).abs() <= 1e-12 * p.abs().max(1.0),
            _ => false,
        },
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same(p, q))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same(v, w)))
        }
        _ => a == b,
    }
}

#[test]
fn matches_the_js_originals() {
    if !node_available() {
        eprintln!("skipping: node not available");
        return;
    }
    // `SLIDES_DIFF_CASES=50000 SLIDES_DIFF_SEED=7` for a longer one-off run.
    let env = |k: &str| std::env::var(k).ok().and_then(|v| v.parse().ok());
    let cases = cases(
        env("SLIDES_DIFF_SEED").unwrap_or(0x5eed_51de),
        env("SLIDES_DIFF_CASES").map_or(6000, |n: u64| n as usize),
    );
    let expected = run_js(&cases);
    assert_eq!(expected.len(), cases.len());
    let mut failures = 0;
    for (case, want) in cases.iter().zip(&expected) {
        let got = run_rust(case);
        if !same(&got, want) {
            failures += 1;
            if failures <= 5 {
                eprintln!(
                    "mismatch for {}:\n  md: {:?}\n  pres: {}\n  args: {}\n  js:   {}\n  rust: {}",
                    case["op"], case["md"], case["pres"], case["args"], want, got
                );
            }
        }
    }
    assert_eq!(failures, 0, "{failures} of {} cases differ", cases.len());
}
