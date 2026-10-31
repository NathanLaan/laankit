//! Headless frame timings for the editor (the Phase 0 spike's benchmark,
//! on the production widget).
//!
//! `cargo run --profile fast-release -p marina-editor --example bench -- [--sizes 1000,5000] [--accesskit] [--fences N] [--engine core|textedit]`
//!
//! `--fences N` puts N 200-line code fences (Rust, Python, JSON, shell) in
//! each document and adds a column for typing inside one; the syntaxes are
//! loaded before measuring.
//!
//! Each frame is `Context::run_ui` plus `Context::tessellate`: everything the
//! CPU does per frame except handing vertices to the GPU. Prints a Markdown
//! table of p50 / p95 / max per scenario.

use std::time::{Duration, Instant};

use egui::{
    pos2, vec2, Context, Event, Key, Modifiers, MouseWheelUnit, Pos2, RawInput, Rect, Vec2,
};
use marina_editor::{gen, Editor, EditorState, Engine};

const SCREEN: Vec2 = vec2(1200.0, 800.0);
const KEYSTROKES: usize = 60;
const SCROLL_FRAMES: usize = 60;

#[derive(Default)]
struct Stats(Vec<Duration>);

impl Stats {
    fn push(&mut self, d: Duration) {
        self.0.push(d);
    }

    fn percentile(&self, p: f64) -> Duration {
        let mut s = self.0.clone();
        s.sort();
        s.get(((s.len().max(1) - 1) as f64 * p).round() as usize)
            .copied()
            .unwrap_or_default()
    }

    fn cell(&self) -> String {
        format!(
            "{} / {} / {}",
            ms(self.percentile(0.5)),
            ms(self.percentile(0.95)),
            ms(self.percentile(1.0))
        )
    }
}

fn ms(d: Duration) -> String {
    format!("{:.2}", d.as_secs_f64() * 1000.0)
}

struct Bench {
    ctx: Context,
    state: EditorState,
    time: f64,
}

impl Bench {
    fn new(text: &str, accesskit: bool, engine: Engine) -> Self {
        let ctx = Context::default();
        marina_egui::fonts::install(&ctx);
        marina_egui::theme::ThemeState::default().apply(&ctx);
        if accesskit {
            ctx.enable_accesskit();
        }
        let mut state = EditorState::new("bench");
        let mut prefs = state.prefs();
        prefs.line_numbers = std::env::var_os("BENCH_NO_GUTTER").is_none();
        prefs.engine = engine;
        state.set_prefs(prefs);
        // Fonts load at the start of a pass: one empty frame first.
        let _ = ctx.run_ui(RawInput::default(), |_| {});
        state.set_text(&ctx, text);
        state.focus();
        Self {
            ctx,
            state,
            time: 0.0,
        }
    }

    /// One frame with `events`; returns its CPU time.
    fn frame(&mut self, events: Vec<Event>) -> Duration {
        self.time += 1.0 / 60.0;
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, SCREEN)),
            time: Some(self.time),
            events,
            ..Default::default()
        };
        let start = Instant::now();
        let state = &mut self.state;
        let mut out = self.ctx.run_ui(input, |ui| {
            Editor::new(state).show(ui);
        });
        out.textures_delta.clear();
        let prims = self.ctx.tessellate(out.shapes, out.pixels_per_point);
        std::hint::black_box(prims);
        start.elapsed()
    }

    /// Puts the caret at character `c` and lets the view scroll to it.
    fn place_cursor(&mut self, c: usize) {
        let ctx = self.ctx.clone();
        self.state.set_cursor(&ctx, c, c);
        self.state.focus();
        for _ in 0..3 {
            self.frame(Vec::new());
        }
    }
}

fn key(key: Key) -> Event {
    Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    }
}

/// Character index of the end of 0-based `line`.
fn line_end_char(text: &str, line: usize) -> usize {
    let mut chars = 0;
    for (i, l) in text.split('\n').enumerate() {
        chars += l.chars().count();
        if i == line {
            return chars;
        }
        chars += 1;
    }
    chars
}

/// First list item at or after `line`.
fn list_line_from(text: &str, line: usize) -> usize {
    text.split('\n')
        .enumerate()
        .skip(line)
        .find(|(_, l)| l.starts_with("- ") && !l.contains('['))
        .map_or(line, |(i, _)| i)
}

/// Character index of the end of the first line of the middle `big` fence.
fn inside_fence(text: &str) -> Option<usize> {
    let lines: Vec<&str> = text.split('\n').collect();
    let openers: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| ["```rust", "```python", "```json", "```sh"].contains(l))
        .map(|(i, _)| i)
        .collect();
    let opener = *openers.get(openers.len() / 2)?;
    Some(line_end_char(text, opener + 50))
}

fn run(lines: usize, accesskit: bool, fences: usize, engine: Engine) -> String {
    let text = if fences > 0 {
        gen::document_with_fences(lines, fences, 200)
    } else {
        gen::document(lines)
    };
    let mut b = Bench::new(&text, accesskit, engine);
    let open = b.frame(Vec::new());
    let mut idle = Stats::default();
    for _ in 0..30 {
        idle.push(b.frame(Vec::new()));
    }
    let mut typing: [Stats; 3] = Default::default();
    for (slot, at) in [0.0, 0.5, 1.0].into_iter().enumerate() {
        let line = ((lines as f64 - 1.0) * at) as usize;
        let c = line_end_char(b.state.text(), line.max(10));
        b.place_cursor(c);
        for _ in 0..KEYSTROKES {
            typing[slot].push(b.frame(vec![Event::Text("a".into())]));
        }
    }
    // Typing inside a big code fence.
    let mut in_fence = Stats::default();
    if let Some(c) = inside_fence(b.state.text()) {
        b.place_cursor(c);
        for _ in 0..KEYSTROKES {
            in_fence.push(b.frame(vec![Event::Text("a".into())]));
        }
    }
    // Typing with an idle frame between keystrokes (the normal case at human speed).
    let mut after_pause = Stats::default();
    b.place_cursor(line_end_char(b.state.text(), lines / 2));
    for _ in 0..30 {
        b.frame(Vec::new());
        after_pause.push(b.frame(vec![Event::Text("a".into())]));
    }
    // Enter at the end of a list item mid-document: continue, then end the list.
    let mut enter = Stats::default();
    let list_line = list_line_from(b.state.text(), lines / 2);
    b.place_cursor(line_end_char(b.state.text(), list_line));
    for _ in 0..20 {
        enter.push(b.frame(vec![key(Key::Enter)]));
    }
    let laid_out = b.state.layout_stats().lines_laid_out;
    b.frame(vec![Event::Text("a".into())]);
    let per_edit = b.state.layout_stats().lines_laid_out - laid_out;
    // Scroll from the top with the wheel.
    b.place_cursor(0);
    let mut scroll = Stats::default();
    b.frame(vec![Event::PointerMoved(pos2(600.0, 400.0))]);
    for _ in 0..SCROLL_FRAMES {
        scroll.push(b.frame(vec![Event::MouseWheel {
            unit: MouseWheelUnit::Point,
            delta: vec2(0.0, -120.0),
            phase: egui::TouchPhase::Move,
            modifiers: Modifiers::NONE,
            source: egui::MouseWheelSource::Wheel,
        }]));
    }
    format!(
        "| {engine:?} | {lines} | {fences} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {per_edit} | {} |",
        if accesskit { "on" } else { "off" },
        ms(open),
        idle.cell(),
        typing[0].cell(),
        typing[1].cell(),
        typing[2].cell(),
        if fences > 0 { in_fence.cell() } else { "—".into() },
        after_pause.cell(),
        enter.cell(),
        scroll.cell(),
        b.state.history_bytes(),
    )
}

fn main() {
    let mut sizes = vec![1_000, 5_000, 20_000, 50_000];
    let mut accesskit = false;
    let mut fences = 0;
    let mut engine = Engine::Core;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--sizes" => {
                sizes = args
                    .next()
                    .unwrap_or_default()
                    .split(',')
                    .filter_map(|s| s.parse().ok())
                    .collect()
            }
            "--accesskit" => accesskit = true,
            "--fences" => fences = args.next().and_then(|n| n.parse().ok()).unwrap_or(4),
            "--engine" => {
                engine = match args.next().as_deref() {
                    Some("textedit") => Engine::TextEdit,
                    _ => Engine::Core,
                }
            }
            other => panic!("unknown argument {other}"),
        }
    }
    println!(
        "Frame CPU time in ms, p50 / p95 / max (run_ui + tessellate, {}x{} window).\n",
        SCREEN.x, SCREEN.y
    );
    // Highlight fences from the first frame, as once the background load is done.
    marina_editor::code::load_blocking();
    println!("| Engine | Lines | 200-line fences | AccessKit | Open | Idle | Type at start | Type in middle | Type at end | Type in a fence | Type after idle frame | Enter in list | Wheel scroll | Lines scanned per edit | Undo history (bytes) |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|");
    for &lines in &sizes {
        println!("{}", run(lines, accesskit, fences, engine));
    }
}
