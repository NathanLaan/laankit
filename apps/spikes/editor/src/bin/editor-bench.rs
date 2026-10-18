//! Headless frame timings for the egui Markdown editor.
//!
//! `cargo run --profile spike -p spike-editor --bin editor-bench -- [--sizes 1000,5000] [--accesskit] [--plain] [--line-galleys | --whole-job]`
//!
//! Each frame is `Context::run_ui` plus `Context::tessellate`: everything the
//! CPU does per frame except handing vertices to the GPU. Prints a Markdown
//! table of p50 / p95 / max per scenario.

use std::time::{Duration, Instant};

use egui::{
    pos2, vec2, Context, Event, Key, Modifiers, MouseWheelUnit, Pos2, RawInput, Rect, Vec2,
};
use spike_common::Stats;
use spike_editor::editor::MarkdownEditor;
use spike_editor::highlight::Strategy;
use spike_editor::{editor_area, gen};

const SCREEN: Vec2 = vec2(1200.0, 800.0);
const KEYSTROKES: usize = 60;
const SCROLL_FRAMES: usize = 60;

struct Bench {
    ctx: Context,
    editor: MarkdownEditor,
    text: String,
    time: f64,
}

impl Bench {
    fn new(text: String, plain: bool, accesskit: bool, strategy: Strategy) -> Self {
        let ctx = Context::default();
        if accesskit {
            ctx.enable_accesskit();
        }
        let mut editor = MarkdownEditor::new(egui::Id::unique("bench-editor"));
        editor.plain = plain;
        editor.highlighter.strategy = strategy;
        ctx.memory_mut(|m| m.request_focus(editor.id));
        Self {
            ctx,
            editor,
            text,
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
        let (editor, text) = (&mut self.editor, &mut self.text);
        let mut out = self.ctx.run_ui(input, |ui| {
            editor_area(ui, editor, text);
        });
        out.textures_delta.clear();
        let prims = self.ctx.tessellate(out.shapes, out.pixels_per_point);
        std::hint::black_box(prims);
        start.elapsed()
    }

    /// Puts the cursor at character `c` and lets the view scroll to it.
    fn place_cursor(&mut self, c: usize) {
        let id = self.editor.id;
        let mut state = egui::TextEdit::load_state(&self.ctx, id).unwrap_or_default();
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::one(
                egui::text::CCursor::new(c),
            )));
        state.store(&self.ctx, id);
        self.ctx.memory_mut(|m| m.request_focus(id));
        // A no-op move makes TextEdit scroll the cursor into view.
        for key in [Key::ArrowRight, Key::ArrowLeft] {
            self.frame(vec![key_event(key)]);
        }
        for _ in 0..3 {
            self.frame(Vec::new());
        }
    }
}

fn key_event(key: Key) -> Event {
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

struct Row {
    lines: usize,
    mode: String,
    open: Duration,
    idle: Stats,
    typing: [Stats; 3],
    after_pause: Stats,
    enter: Stats,
    scroll: Stats,
    rebuild_lines: u64,
}

fn run(lines: usize, plain: bool, accesskit: bool, strategy: Strategy) -> Row {
    let text = gen::document(lines);
    let mode = format!(
        "{}{}{}",
        if plain { "plain" } else { "highlight" },
        match strategy {
            Strategy::Incremental => ", incremental",
            Strategy::LineGalleys => ", line galleys",
            Strategy::WholeJob => ", whole job",
        },
        if accesskit { ", accesskit" } else { "" }
    );
    let mut b = Bench::new(text, plain, accesskit, strategy);
    let open = b.frame(Vec::new());
    let mut idle = Stats::default();
    for _ in 0..30 {
        idle.push(b.frame(Vec::new()));
    }
    let mut typing: [Stats; 3] = Default::default();
    for (slot, at) in [0.0, 0.5, 1.0].into_iter().enumerate() {
        let line = ((lines as f64 - 1.0) * at) as usize;
        let c = line_end_char(&b.text, line.max(10));
        b.place_cursor(c);
        let before = b.editor.highlighter.stats;
        for _ in 0..KEYSTROKES {
            typing[slot].push(b.frame(vec![Event::Text("a".into())]));
        }
        if slot == 1 {
            let after = b.editor.highlighter.stats;
            let per = |i: usize| {
                (after.time[i] - before.time[i]).as_secs_f64() * 1000.0 / KEYSTROKES as f64
            };
            log::info!("{lines} lines, per keystroke: layouter calls {}, rebuilds {}, scan {:.2} ms, line galleys {:.2} ms, concat {:.2} ms", (after.calls - before.calls) / KEYSTROKES as u64, (after.rebuilds - before.rebuilds) / KEYSTROKES as u64, per(0), per(1), per(2));
        }
    }
    // Typing with an idle frame between keystrokes (the normal case at human speed).
    let mut after_pause = Stats::default();
    b.place_cursor(line_end_char(&b.text, lines / 2));
    for _ in 0..30 {
        b.frame(Vec::new());
        after_pause.push(b.frame(vec![Event::Text("a".into())]));
    }
    // Enter at the end of a list item mid-document: continue, then end the list.
    let mut enter = Stats::default();
    let list_line = list_line_from(&b.text, lines / 2);
    b.place_cursor(line_end_char(&b.text, list_line));
    for _ in 0..20 {
        enter.push(b.frame(vec![key_event(Key::Enter)]));
    }
    let scanned_before = b.editor.highlighter.stats.lines_scanned;
    b.frame(vec![Event::Text("a".into())]);
    let rebuild_lines = b.editor.highlighter.stats.lines_scanned - scanned_before;
    // Scroll from the top with the wheel.
    b.place_cursor(0);
    let mut scroll = Stats::default();
    b.frame(vec![Event::PointerMoved(pos2(600.0, 400.0))]);
    for _ in 0..SCROLL_FRAMES {
        let wheel = Event::MouseWheel {
            unit: MouseWheelUnit::Point,
            delta: vec2(0.0, -120.0),
            phase: egui::TouchPhase::Move,
            modifiers: Modifiers::NONE,
            source: egui::MouseWheelSource::Wheel,
        };
        scroll.push(b.frame(vec![wheel]));
    }
    Row {
        lines,
        mode,
        open,
        idle,
        typing,
        after_pause,
        enter,
        scroll,
        rebuild_lines,
    }
}

fn ms(d: Duration) -> String {
    format!("{:.2}", d.as_secs_f64() * 1000.0)
}

fn cell(s: &Stats) -> String {
    format!(
        "{} / {} / {}",
        ms(s.percentile(0.5)),
        ms(s.percentile(0.95)),
        ms(s.max())
    )
}

fn main() {
    spike_common::init_logging();
    let mut sizes = vec![1_000, 5_000, 20_000, 50_000];
    let (mut accesskit, mut plain, mut strategy) = (false, false, Strategy::Incremental);
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
            "--plain" => plain = true,
            "--whole-job" => strategy = Strategy::WholeJob,
            "--line-galleys" => strategy = Strategy::LineGalleys,
            other => panic!("unknown argument {other}"),
        }
    }
    println!(
        "Frame CPU time in ms, p50 / p95 / max (run_ui + tessellate, {}x{} window).\n",
        SCREEN.x, SCREEN.y
    );
    println!("| Lines | Mode | Open | Idle | Type at start | Type in middle | Type at end | Type after idle frame | Enter in list | Wheel scroll | Lines rescanned per edit |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|");
    for &lines in &sizes {
        let row = run(lines, plain, accesskit, strategy);
        println!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            row.lines,
            row.mode,
            ms(row.open),
            cell(&row.idle),
            cell(&row.typing[0]),
            cell(&row.typing[1]),
            cell(&row.typing[2]),
            cell(&row.after_pause),
            cell(&row.enter),
            cell(&row.scroll),
            row.rebuild_lines
        );
    }
}
