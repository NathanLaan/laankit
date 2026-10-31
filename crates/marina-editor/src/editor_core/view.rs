//! The core engine's widget: virtualized layout, input and painting.
//!
//! Each frame: rescan what changed (the highlighter), work out every line's
//! top (measured heights, estimates for lines not laid out yet, nothing for
//! folded lines), keep the first visible line in place if heights above it
//! changed, then inside the scroll area handle the keyboard and pointer, lay
//! out the visible lines and paint them. Lines off screen are measured a few
//! at a time on idle frames (with soft wrap on), and their galleys dropped
//! again past a budget, so memory stays bounded.

use std::ops::Range;
use std::sync::Arc;

use egui::emath::TSTransform;
use egui::text::{CCursor, CCursorRange};
use egui::{
    pos2, vec2, Align, Align2, Event, FontFamily, FontId, Galley, Key, Modifiers, PointerButton,
    Pos2, Rect, Response, ScrollArea, Sense, Shape, Stroke, Ui, Vec2,
};
use marina_egui::{icons, theme};

use super::fold;
use super::ops;
use super::selection::{Sel, Selections};
use super::Drag;
use crate::editor::{EditorEvent, GUTTER_MIN, PAD};
use crate::highlight::{
    cols_rect, link_at, merged_job, EditorColors, LayoutOptions, LinkTarget, Look,
};
use crate::list_edits::renumber_after_edit;
use crate::matching::{self, MAX_SELECTION_MATCHES};
use crate::spell::SpellChecker;
use crate::state::{Completion, EditorState};

/// The fold gutter's width.
pub(crate) const FOLD_W: f32 = 14.0;
/// Line galleys kept before the ones far from the viewport are dropped.
const MAX_GALLEYS: usize = 3_000;
/// Lines kept around the viewport when galleys are dropped.
const KEEP_AROUND: usize = 300;
/// Time spent measuring off-screen lines on an idle frame (soft wrap only).
const MEASURE_BUDGET_S: f64 = 0.002;
/// Completion entries shown at most (`.slice(0, 50)`).
const MAX_COMPLETIONS: usize = 50;

/// This frame's layout settings.
pub(crate) struct Lay {
    pub colors: EditorColors,
    pub font_size: f32,
    pub wrap: bool,
    pub spell: Option<(Arc<dyn SpellChecker>, u64)>,
    pub look: Look,
    pub row_h: f32,
    pub wrap_w: f32,
    pub gutter: f32,
}

impl Lay {
    fn opts(&self) -> LayoutOptions<'_> {
        LayoutOptions {
            colors: &self.colors,
            font_size: self.font_size,
            wrap: self.wrap,
            spell: self
                .spell
                .as_ref()
                .map(|(c, v)| (c.as_ref() as &dyn SpellChecker, *v)),
        }
    }

    /// A line's height before it's laid out: one row, or as many as its
    /// length would wrap to at an average glyph width.
    fn estimate(&self, len: usize) -> f32 {
        if !self.wrap {
            return self.row_h;
        }
        let w = len as f32 * self.font_size * 0.52;
        self.row_h * (w / self.wrap_w.max(1.0)).ceil().max(1.0)
    }

    fn estimate_width(&self, len: usize) -> f32 {
        len as f32 * self.font_size * 0.52
    }
}

impl EditorState {
    /// The core engine's text area; returns its response and the character
    /// under the pointer (on click, drag or drop frames).
    pub(crate) fn core_body(
        &mut self,
        ui: &mut Ui,
        colors: &EditorColors,
        accessible_name: &str,
        context_menu: bool,
        time: f64,
        events: &mut Vec<EditorEvent>,
    ) -> (Response, Option<usize>) {
        let ctx = ui.ctx().clone();
        self.sync_rope();
        let lay = self.lay(ui, colors);
        let hint = self.core.hl_hint.take();
        self.highlighter
            .update_hinted(&ctx, &self.text, Some(self.version), hint);
        self.highlighter.set_look(lay.look);
        self.refresh_tops(&lay);

        let mut area = if lay.wrap {
            ScrollArea::vertical()
        } else {
            ScrollArea::both()
        }
        .id_salt(self.id.with("scroll"))
        .auto_shrink([false, false]);
        if let Some(offset) = self.scroll_request.take() {
            area = area.scroll_offset(offset);
        } else if let Some((line, top, version)) = self.core.anchor {
            // Heights above the first visible line changed (lines measured,
            // folds, a new width): keep that line where it was.
            if version == self.version && line < self.core.tops.len() - 1 {
                let now = self.core.tops[line];
                if (now - top).abs() > 0.5 {
                    area = area.vertical_scroll_offset((self.scroll_offset.y + now - top).max(0.0));
                }
            }
        }

        let scrolled = area.show_viewport(ui, |ui, viewport| {
            self.core_content(
                ui,
                &lay,
                viewport,
                accessible_name,
                context_menu,
                time,
                events,
            )
        });
        self.scroll_offset = scrolled.state.clamped_offset();
        self.core.scroll_id = Some(scrolled.id);
        self.core.viewport_h = scrolled.inner_rect.height();
        let first = self.core.visible.start.min(self.core.tops.len() - 1);
        self.core.anchor = Some((first, self.core.tops[first], self.version));
        scrolled.inner
    }

    /// This frame's layout settings, from the space available.
    fn lay(&self, ui: &Ui, colors: &EditorColors) -> Lay {
        let font_size = self.prefs.font_size;
        let lines = self.lines();
        let gutter = FOLD_W
            + if self.prefs.line_numbers {
                let digits = lines.max(1).to_string().len() as f32;
                GUTTER_MIN.max(digits * font_size * 0.62 + 16.0)
            } else {
                0.0
            };
        let avail = ui.available_rect_before_wrap();
        let bar = ui.spacing().scroll.allocated_width();
        let wrap_w = (avail.width() - gutter - 2.0 * PAD - bar).max(40.0);
        let spell = self
            .spell
            .clone()
            .filter(|_| self.prefs.spell_check)
            .map(|c| (c, self.spell_version));
        let row_h = ui.fonts_mut(|f| f.row_height(&FontId::proportional(font_size)));
        let look = Look::new(
            &LayoutOptions {
                colors,
                font_size,
                wrap: self.prefs.wrap,
                spell: spell
                    .as_ref()
                    .map(|(c, v)| (c.as_ref() as &dyn SpellChecker, *v)),
            },
            wrap_w,
        );
        Lay {
            colors: *colors,
            font_size,
            wrap: self.prefs.wrap,
            spell,
            look,
            row_h,
            wrap_w,
            gutter,
        }
    }

    /// Every line's top (and the total height), when anything they depend on changed.
    fn refresh_tops(&mut self, lay: &Lay) {
        let key = (
            self.highlighter.layout_version(),
            self.core.folds.rev(),
            lay.row_h.to_bits(),
            lay.look.wrap,
        );
        if self.core.tops_key == Some(key) {
            return;
        }
        self.core.tops_key = Some(key);
        let n = self.highlighter.line_count().max(1);
        let runs = self.core.folds.hidden_runs();
        let mut run = 0;
        let mut tops = Vec::with_capacity(n + 1);
        let mut y = 0.0;
        let mut widest: f32 = 0.0;
        for i in 0..n {
            tops.push(y);
            while run < runs.len() && runs[run].end <= i {
                run += 1;
            }
            if run < runs.len() && runs[run].contains(&i) {
                continue;
            }
            let (h, w) = match self.highlighter.measured_size(i) {
                Some(s) => (s.y, s.x),
                None => {
                    let len = self.highlighter.line_len(i);
                    (lay.estimate(len), lay.estimate_width(len))
                }
            };
            y += h;
            widest = widest.max(w);
        }
        tops.push(y);
        self.core.tops = tops;
        self.core.max_width = widest;
    }

    /// Line `line`'s galley.
    fn galley(&mut self, ui: &Ui, lay: &Lay, line: usize) -> Arc<Galley> {
        let range = self.line_bytes(line);
        let EditorState {
            highlighter, text, ..
        } = self;
        highlighter
            .line_galley(ui, line, &text[range], &lay.opts(), lay.look)
            .expect("line in range")
    }

    /// The line at `y` (relative to the text origin), skipping folded lines.
    fn line_at_y(&self, y: f32) -> usize {
        let tops = &self.core.tops;
        let n = tops.len() - 1;
        let i = tops[..n].partition_point(|&t| t <= y).saturating_sub(1);
        // Folded lines have no height; land on the visible line holding y.
        let mut i = i.min(n - 1);
        while i > 0 && self.core.folds.hidden(i) {
            i -= 1;
        }
        i
    }

    /// The character at `p` (relative to the text origin).
    fn hit(&mut self, ui: &Ui, lay: &Lay, p: Vec2) -> usize {
        let line = self.line_at_y(p.y.max(0.0));
        let g = self.galley(ui, lay, line);
        let local = vec2(p.x, p.y - self.core.tops[line]);
        let col = g.cursor_from_pos(local).index.0;
        self.rope.line_to_char(line) + col
    }

    /// The caret rect of character `c` (relative to the text origin).
    fn caret_rect(&mut self, ui: &Ui, lay: &Lay, c: usize) -> Rect {
        let (line, col) = self.line_col(c);
        let g = self.galley(ui, lay, line);
        g.pos_from_cursor(CCursor::new(col))
            .translate(vec2(0.0, self.core.tops[line]))
    }

    /// One row up or down from `c`, aiming for `x`; returns the new
    /// character and the x it aimed for.
    fn vertical(
        &mut self,
        ui: &Ui,
        lay: &Lay,
        c: usize,
        x: Option<f32>,
        down: bool,
    ) -> (usize, f32) {
        let (line, col) = self.line_col(c);
        let g = self.galley(ui, lay, line);
        let lc = g.layout_from_cursor(CCursor::new(col));
        let x = x.unwrap_or_else(|| g.pos_from_cursor(CCursor::new(col)).left());
        let row = lc.row as isize + if down { 1 } else { -1 };
        if row >= 0 && (row as usize) < g.rows.len() {
            let y = g.rows[row as usize].rect().center().y;
            let col = g.cursor_from_pos(vec2(x, y)).index.0;
            return (self.rope.line_to_char(line) + col, x);
        }
        // Onto the next or previous visible line.
        let mut l = line as isize;
        loop {
            l += if down { 1 } else { -1 };
            if l < 0 {
                return (0, x);
            }
            if l as usize >= self.lines() {
                return (self.chars_len(), x);
            }
            if !self.core.folds.hidden(l as usize) {
                break;
            }
        }
        let l = l as usize;
        let g = self.galley(ui, lay, l);
        let r = if down { g.rows.first() } else { g.rows.last() };
        let y = r.map_or(0.0, |r| r.rect().center().y);
        let col = g.cursor_from_pos(vec2(x, y)).index.0;
        (self.rope.line_to_char(l) + col, x)
    }

    /// The scroll area's content: input, layout and painting.
    #[allow(clippy::too_many_arguments)]
    fn core_content(
        &mut self,
        ui: &mut Ui,
        lay: &Lay,
        viewport: Rect,
        accessible_name: &str,
        context_menu: bool,
        time: f64,
        events: &mut Vec<EditorEvent>,
    ) -> (Response, Option<usize>) {
        let ctx = ui.ctx().clone();
        let width = if lay.wrap {
            viewport.width()
        } else {
            (self.core.max_width + lay.gutter + 2.0 * PAD + 40.0).max(viewport.width())
        };
        let total = *self.core.tops.last().expect("tops") + 2.0 * PAD;
        let content =
            Rect::from_min_size(ui.max_rect().min, vec2(width, total.max(viewport.height())));
        ui.allocate_rect(content, Sense::hover());
        let response = ui
            .interact(content, self.id, Sense::click_and_drag())
            .accessible_name(accessible_name);
        let origin = content.min + vec2(lay.gutter + PAD, PAD);
        self.core.origin = origin;
        let clip = ui.clip_rect();
        let text_clip = Rect::from_min_max(pos2(clip.left() + lay.gutter, clip.top()), clip.max);

        let focused = ctx.memory(|m| m.has_focus(self.id));
        // Focus moved away mid-composition: keep what was composed, as browsers
        // do, rather than leave a composition open that swallows later input.
        if !focused && self.core.ime.is_some() {
            self.end_composition(time, events);
        }
        if focused {
            ctx.memory_mut(|m| {
                m.set_focus_lock_filter(
                    self.id,
                    egui::EventFilter {
                        tab: true,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: true,
                    },
                )
            });
        }

        // Keyboard, then pointer.
        let before = (self.version, self.core.sel.clone());
        if focused {
            self.core_keys(ui, lay, time, events);
        }
        let char_at_pointer =
            self.core_pointer(ui, lay, &response, origin, text_clip, time, events);
        if self.version != before.0 {
            let hint = self.core.hl_hint.take();
            self.highlighter
                .update_hinted(&ctx, &self.text, Some(self.version), hint);
            if self.core.tops.len() != self.lines() + 1 {
                self.refresh_tops(lay);
            }
        }
        if self.version != before.0 || self.core.sel != before.1 {
            self.core.last_interaction = time;
            self.update_core_completion(focused);
        }

        // What's on screen (in text coordinates).
        let vy = viewport.min.y - PAD..viewport.max.y - PAD;
        let first = self.line_at_y(vy.start.max(0.0));
        let n = self.lines();
        let mut last = first;
        while last + 1 < n && self.core.tops[last + 1] < vy.end {
            last += 1;
        }
        self.core.visible = first..last + 1;
        // Lay out what's visible (heights settle; positions can shift for the
        // lines laid out for the first time, so take the tops again).
        for line in first..=last {
            if !self.core.folds.hidden(line) {
                self.galley(ui, lay, line);
            }
        }
        self.refresh_tops(lay);

        // Scroll the caret into view after input or an API request.
        if let Some((c, top)) = self.reveal.take() {
            let r = self.caret_rect(ui, lay, c).translate(origin.to_vec2());
            ui.scroll_to_rect(
                r.expand2(vec2(0.0, 4.0)),
                Some(if top { Align::TOP } else { Align::Center }),
            );
        } else if std::mem::take(&mut self.core.reveal) {
            let head = self.core.sel.primary().head;
            let r = self.caret_rect(ui, lay, head).translate(origin.to_vec2());
            ui.scroll_to_rect(r.expand2(vec2(0.0, lay.row_h)), None);
        }

        if self.completion.is_some() {
            let pos = self.core_completion_pos(ui, lay);
            ctx.data_mut(|d| d.insert_temp(self.id.with("complete-pos"), pos));
        }
        self.paint(ui, lay, origin, clip, text_clip, focused, time);
        self.paint_gutter_and_folds(ui, lay, origin, clip);

        if focused {
            let head = self.core.sel.primary().head;
            let caret = self.caret_rect(ui, lay, head).translate(origin.to_vec2());
            let to_global = ctx
                .layer_transform_to_global(ui.layer_id())
                .unwrap_or_default();
            ui.output_mut(|o| {
                o.ime = Some(egui::output::IMEOutput {
                    purpose: egui::IMEPurpose::Normal,
                    rect: to_global * text_clip,
                    cursor_rect: to_global * caret,
                    should_interrupt_composition: false,
                });
            });
        }
        self.core_accesskit(ui, lay, origin);
        self.measure_offscreen(ui, lay);

        // Ctrl/Cmd+click opens links and wikilinks.
        if response.clicked() && ui.input(|i| i.modifiers.command) {
            if let Some(c) = char_at_pointer {
                if let Some(link) = self.core_link_at(c) {
                    events.push(match link {
                        LinkTarget::Wiki(name) => EditorEvent::OpenWikilink(name),
                        LinkTarget::Url(url) => EditorEvent::OpenLink(url),
                    });
                }
            }
        }
        let dropped: Vec<std::path::PathBuf> = ui.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        if !dropped.is_empty()
            && ctx
                .pointer_latest_pos()
                .is_none_or(|p| response.rect.contains(p))
        {
            events.push(EditorEvent::DropFiles(dropped));
        }
        if context_menu {
            if response.secondary_clicked() {
                self.menu_word = char_at_pointer.and_then(|c| self.misspelled_at(c));
            }
            response.context_menu(|ui| self.context_menu_ui(ui, time, events));
        }
        (response, char_at_pointer)
    }

    /// The link around character `c`.
    fn core_link_at(&self, c: usize) -> Option<LinkTarget> {
        let (line, col) = self.line_col(c);
        let text = self.line_text(line);
        let b = text.char_indices().nth(col).map_or(text.len(), |(b, _)| b);
        link_at(text, b)
    }

    // --- Editing -------------------------------------------------------------

    /// Replaces each range with its text in one step, then puts the carets
    /// after the inserted texts (`ranges` are char ranges, one per selection,
    /// in selection order).
    #[allow(clippy::too_many_arguments)]
    fn replace(
        &mut self,
        ranges: Vec<Range<usize>>,
        reps: Vec<String>,
        typing: bool,
        structural: bool,
        time: f64,
        events: &mut Vec<EditorEvent>,
    ) {
        let primary = self.core.sel.primary_index();
        let mut order: Vec<usize> = (0..ranges.len()).collect();
        order.sort_by_key(|&i| ranges[i].start);
        // Merge overlaps (from extended deletions) by dropping the later ones.
        let mut sorted: Vec<(usize, Range<usize>, String)> = Vec::new();
        for i in order {
            let r = ranges[i].clone();
            if let Some((_, last, _)) = sorted.last() {
                if r.start < last.end {
                    continue;
                }
            }
            sorted.push((i, r, reps[i].clone()));
        }
        let rs: Vec<Range<usize>> = sorted.iter().map(|(_, r, _)| r.clone()).collect();
        let reps: Vec<String> = sorted.iter().map(|(_, _, s)| s.clone()).collect();
        let rope = &self.rope;
        let (new_text, placed) =
            ops::replace_ranges(&self.text, &rs, &reps, |c| rope.char_to_byte(c));
        if new_text == self.text {
            return;
        }
        let new_primary = sorted
            .iter()
            .position(|(i, _, _)| *i == primary)
            .unwrap_or(0);
        let sels: Vec<Sel> = placed.iter().map(|r| Sel::caret(r.end)).collect();
        self.commit(
            new_text,
            Selections::new(sels, new_primary),
            typing,
            structural,
            time,
            events,
        );
    }

    /// Makes `new_text` the note as one history step with these selections.
    fn commit(
        &mut self,
        new_text: String,
        sels: Selections,
        typing: bool,
        structural: bool,
        time: f64,
        events: &mut Vec<EditorEvent>,
    ) {
        let before = {
            let p = self.core.sel.primary();
            (p.anchor, p.head)
        };
        let new_text = if structural {
            renumber_after_edit(&self.text, &new_text).unwrap_or(new_text)
        } else {
            new_text
        };
        let p = sels.primary();
        self.history.record(
            &self.recorded,
            &new_text,
            before,
            (p.anchor, p.head),
            time,
            typing,
        );
        self.text = new_text;
        self.recorded.clone_from(&self.text);
        self.version += 1;
        self.sync_rope();
        let mut sels = sels;
        sels.clamp(self.chars_len());
        self.core.sel = sels;
        self.core.goal_x = None;
        self.core.reveal = true;
        self.core_unfold_carets();
        events.push(EditorEvent::Changed);
    }

    /// Changes the text without a history step (IME preedit).
    fn splice_raw(&mut self, range: Range<usize>, s: &str) {
        let (bf, bt) = (self.char_to_byte(range.start), self.char_to_byte(range.end));
        self.text.replace_range(bf..bt, s);
        self.version += 1;
        self.sync_rope();
    }

    /// Inserts `s` at every selection.
    fn insert_all(
        &mut self,
        s: &str,
        typing: bool,
        structural: bool,
        time: f64,
        events: &mut Vec<EditorEvent>,
    ) {
        let ranges: Vec<Range<usize>> = self
            .core
            .sel
            .ranges()
            .iter()
            .map(|r| r.from()..r.to())
            .collect();
        let reps = vec![s.to_owned(); ranges.len()];
        self.replace(ranges, reps, typing, structural, time, events);
    }

    /// Deletes every selection, or from each caret to `target(caret)`.
    fn delete_with(
        &mut self,
        mut target: impl FnMut(&Self, usize) -> usize,
        time: f64,
        events: &mut Vec<EditorEvent>,
    ) {
        let ranges: Vec<Range<usize>> = self
            .core
            .sel
            .ranges()
            .iter()
            .map(|r| {
                if r.is_empty() {
                    let t = target(self, r.head);
                    t.min(r.head)..t.max(r.head)
                } else {
                    r.from()..r.to()
                }
            })
            .collect();
        if ranges.iter().all(|r| r.is_empty()) {
            return;
        }
        let reps = vec![String::new(); ranges.len()];
        self.replace(ranges, reps, false, true, time, events);
    }

    /// Runs a text helper (`(text, anchor, head) -> edit`, in characters) on
    /// the caret's line alone when the selection is within one line, so its
    /// offset conversions don't scan the whole note; returns the edit for
    /// the whole note. Falls back to the whole text otherwise.
    pub(crate) fn core_line_edit(
        &mut self,
        anchor: usize,
        head: usize,
        f: impl Fn(&str, usize, usize) -> Option<crate::list_edits::DocEdit>,
    ) -> Option<crate::list_edits::DocEdit> {
        self.sync_rope();
        let (la, ca) = self.line_col(anchor);
        let (lh, ch) = self.line_col(head);
        if la != lh {
            return f(&self.text, anchor, head);
        }
        let bytes = self.line_bytes(la);
        let start = self.line_chars(la).start;
        let edit = f(&self.text[bytes.clone()], ca, ch)?;
        let mut text = String::with_capacity(self.text.len() + edit.text.len());
        text.push_str(&self.text[..bytes.start]);
        text.push_str(&edit.text);
        text.push_str(&self.text[bytes.end..]);
        Some(crate::list_edits::DocEdit {
            text,
            anchor: start + edit.anchor,
            head: start + edit.head,
        })
    }

    // --- Keyboard ------------------------------------------------------------

    /// The keys, text, clipboard and IME events the editor's own handlers left.
    fn core_keys(&mut self, ui: &mut Ui, lay: &Lay, time: f64, events: &mut Vec<EditorEvent>) {
        let input: Vec<Event> = ui.input(|i| i.events.clone());
        for e in input {
            match e {
                Event::Ime(ime) => self.core_ime(ime, time, events),
                // Copying doesn't change the text, so it works mid-composition too.
                Event::Copy => {
                    self.copy(ui);
                }
                _ if self.core.ime.is_some() => {}
                Event::Text(t) => {
                    if !t.chars().any(char::is_control) {
                        self.insert_all(&t, true, false, time, events);
                    }
                }
                Event::Cut => {
                    if self.copy(ui) {
                        self.delete_with(|_, c| c, time, events);
                    }
                }
                Event::Paste(s) => self.paste(&s, time, events),
                Event::Key {
                    key,
                    physical_key,
                    pressed: true,
                    modifiers,
                    ..
                } => self.core_key(ui, lay, key, physical_key, modifiers, time, events),
                _ => {}
            }
        }
    }

    /// Copies the selected text (one line per non-empty selection); true if there was any.
    fn copy(&mut self, ui: &Ui) -> bool {
        let parts: Vec<String> = self
            .core
            .sel
            .ranges()
            .iter()
            .filter(|r| !r.is_empty())
            .map(|r| self.text[self.char_to_byte(r.from())..self.char_to_byte(r.to())].to_owned())
            .collect();
        if parts.is_empty() {
            return false;
        }
        ui.ctx().copy_text(parts.join("\n"));
        true
    }

    /// Pastes `s`: one line per selection when the counts match (CodeMirror's
    /// multi-cursor paste), otherwise all of it at each.
    fn paste(&mut self, s: &str, time: f64, events: &mut Vec<EditorEvent>) {
        let s = s.replace("\r\n", "\n");
        let ranges: Vec<Range<usize>> = self
            .core
            .sel
            .ranges()
            .iter()
            .map(|r| r.from()..r.to())
            .collect();
        let lines: Vec<&str> = s.split('\n').collect();
        let reps: Vec<String> = if ranges.len() > 1 && lines.len() == ranges.len() {
            lines.iter().map(|l| (*l).to_owned()).collect()
        } else {
            vec![s.clone(); ranges.len()]
        };
        self.replace(ranges, reps, false, true, time, events);
    }

    /// IME composition: the preedit text sits in the note (not in the
    /// history, no `Changed`) until the commit, which is one step.
    fn core_ime(&mut self, ime: egui::ImeEvent, time: f64, events: &mut Vec<EditorEvent>) {
        match ime {
            egui::ImeEvent::Preedit { text, .. } => {
                if text.is_empty() && self.core.ime.is_none() {
                    return;
                }
                if text == "\n" || text == "\r" {
                    return;
                }
                let at = self.take_preedit_or_selection();
                if text.is_empty() {
                    self.core.sel = Selections::single(Sel::caret(at));
                    self.composing = false;
                } else {
                    self.splice_raw(at..at, &text);
                    let n = text.chars().count();
                    self.core.ime = Some((at, n));
                    self.core.sel = Selections::single(Sel::caret(at + n));
                    self.composing = true;
                }
                self.core.reveal = true;
            }
            egui::ImeEvent::Commit(text) => {
                if text.is_empty() && self.core.ime.is_none() {
                    return;
                }
                if text == "\n" || text == "\r" {
                    return;
                }
                let at = self.take_preedit_or_selection();
                self.splice_raw(at..at, &text);
                let caret = at + text.chars().count();
                self.core.ime = None;
                self.composing = false;
                // One step from the text before the composition (the history
                // diffs against what was last recorded).
                let new_text = self.text.clone();
                self.commit(
                    new_text,
                    Selections::single(Sel::caret(caret)),
                    false,
                    false,
                    time,
                    events,
                );
            }
            egui::ImeEvent::DeleteSurrounding {
                before_chars,
                after_chars,
            } => {
                let head = self.core.sel.primary().head;
                let from = head.saturating_sub(before_chars);
                let to = (head + after_chars).min(self.chars_len());
                let ranges: Vec<Range<usize>> = std::iter::once(from..to).collect();
                self.replace(ranges, vec![String::new()], false, false, time, events);
            }
            // The input method was switched off (or never sent a final
            // commit): keep the composed text, so the composition can't stay
            // open and swallow every later key, copy and paste.
            #[allow(deprecated)]
            egui::ImeEvent::Disabled => self.end_composition(time, events),
            #[allow(deprecated)]
            egui::ImeEvent::Enabled => {}
        }
    }

    /// Ends an open composition, keeping its text as typed (one undo step).
    fn end_composition(&mut self, time: f64, events: &mut Vec<EditorEvent>) {
        let Some((at, n)) = self.core.ime.take() else {
            return;
        };
        self.composing = false;
        let new_text = self.text.clone();
        self.commit(
            new_text,
            Selections::single(Sel::caret(at + n)),
            false,
            false,
            time,
            events,
        );
    }

    /// Removes the preedit text (or the primary selection); returns where it was.
    fn take_preedit_or_selection(&mut self) -> usize {
        if let Some((s, len)) = self.core.ime.take() {
            self.splice_raw(s..s + len, "");
            return s;
        }
        let p = self.core.sel.primary();
        self.core.sel.collapse_to_primary();
        if !p.is_empty() {
            self.splice_raw(p.from()..p.to(), "");
        }
        p.from()
    }

    #[allow(clippy::too_many_arguments)]
    fn core_key(
        &mut self,
        ui: &Ui,
        lay: &Lay,
        key: Key,
        physical: Option<Key>,
        mods: Modifiers,
        time: f64,
        events: &mut Vec<EditorEvent>,
    ) {
        let mac = cfg!(target_os = "macos");
        let word = if mac { mods.alt } else { mods.ctrl };
        let line_mod = mac && mods.mac_cmd;
        let extend = mods.shift;
        let phys = physical.unwrap_or(key);
        match key {
            Key::ArrowLeft | Key::ArrowRight => {
                let right = key == Key::ArrowRight;
                self.move_each(extend, |s, r| {
                    if !r.is_empty() && !extend && !word && !line_mod {
                        return if right { r.to() } else { r.from() };
                    }
                    let (line, col) = s.line_col(r.head);
                    let chars = s.line_chars(line);
                    if line_mod {
                        return if right { chars.end } else { chars.start };
                    }
                    if word {
                        let text = s.line_text(line);
                        let len = chars.end - chars.start;
                        return match (right, col) {
                            (true, c) if c >= len => (chars.end + 1).min(s.chars_len()),
                            (true, c) => chars.start + ops::word_right(text, c),
                            (false, 0) => chars.start.saturating_sub(1),
                            (false, c) => chars.start + ops::word_left(text, c),
                        };
                    }
                    if right {
                        (r.head + 1).min(s.chars_len())
                    } else {
                        r.head.saturating_sub(1)
                    }
                });
                self.skip_folds(key == Key::ArrowRight);
            }
            Key::ArrowUp | Key::ArrowDown if !mods.alt => {
                if line_mod {
                    let to = if key == Key::ArrowDown {
                        self.chars_len()
                    } else {
                        0
                    };
                    self.move_each(extend, |_, _| to);
                } else {
                    self.move_vertical(ui, lay, key == Key::ArrowDown, extend, 1);
                }
            }
            Key::PageUp | Key::PageDown => {
                let rows = ((self.core.viewport_h / lay.row_h).floor() as usize).max(1);
                self.move_vertical(ui, lay, key == Key::PageDown, extend, rows);
            }
            Key::Home | Key::End => {
                let end = key == Key::End;
                if mods.command && !mac || line_mod {
                    let to = if end { self.chars_len() } else { 0 };
                    self.move_each(extend, |_, _| to);
                } else {
                    self.move_each(extend, |s, r| {
                        let (line, col) = s.line_col(r.head);
                        let chars = s.line_chars(line);
                        if end {
                            return chars.end;
                        }
                        // Smart Home: the indentation first, then the line start.
                        let first = ops::first_non_space(s.line_text(line));
                        chars.start + if col == first { 0 } else { first }
                    });
                }
            }
            Key::Backspace => {
                let word = word && !line_mod;
                self.delete_with(
                    |s, c| {
                        let (line, col) = s.line_col(c);
                        let start = s.line_chars(line).start;
                        if line_mod {
                            start
                        } else if word && col > 0 {
                            start + ops::word_left(s.line_text(line), col)
                        } else {
                            c.saturating_sub(1)
                        }
                    },
                    time,
                    events,
                );
            }
            Key::Delete => {
                self.delete_with(
                    |s, c| {
                        let (line, col) = s.line_col(c);
                        let chars = s.line_chars(line);
                        if word && c < chars.end {
                            chars.start + ops::word_right(s.line_text(line), col)
                        } else {
                            (c + 1).min(s.chars_len())
                        }
                    },
                    time,
                    events,
                );
            }
            Key::Enter if mods.is_none() || mods.shift_only() => {
                self.insert_all("\n", false, true, time, events);
            }
            Key::A if mods.command && !mods.shift && !mods.alt => {
                self.core.sel = Selections::single(Sel::new(0, self.chars_len()));
            }
            Key::D if mods.command && !mods.shift && !mods.alt => self.select_next_occurrence(),
            Key::Escape if mods.is_none() && self.core.sel.is_multi() => {
                self.core.sel.collapse_to_primary();
            }
            _ if mods.command && mods.shift && !mods.alt && phys == Key::OpenBracket => {
                self.fold_at_caret();
            }
            _ if mods.command && mods.shift && !mods.alt && phys == Key::CloseBracket => {
                self.unfold_at_caret();
            }
            _ => {}
        }
    }

    /// Moves every selection's head to `to(self, range)` (extending if asked).
    fn move_each(&mut self, extend: bool, mut to: impl FnMut(&Self, Sel) -> usize) {
        let ranges: Vec<Sel> = self.core.sel.ranges().to_vec();
        let moved: Vec<Sel> = ranges
            .iter()
            .map(|r| r.moved(to(self, *r), extend))
            .collect();
        self.core.sel = Selections::new(moved, self.core.sel.primary_index());
        self.core.goal_x = None;
        self.core.reveal = true;
    }

    /// Up or down by `rows` visual rows, keeping the primary's goal column.
    fn move_vertical(&mut self, ui: &Ui, lay: &Lay, down: bool, extend: bool, rows: usize) {
        let ranges: Vec<Sel> = self.core.sel.ranges().to_vec();
        let primary = self.core.sel.primary_index();
        let mut goal = None;
        let mut moved = Vec::with_capacity(ranges.len());
        for (i, r) in ranges.iter().enumerate() {
            let mut c = r.head;
            let mut x = if i == primary { self.core.goal_x } else { None };
            for _ in 0..rows {
                let (nc, nx) = self.vertical(ui, lay, c, x, down);
                c = nc;
                x = Some(nx);
            }
            if i == primary {
                goal = x;
            }
            moved.push(r.moved(c, extend));
        }
        self.core.sel = Selections::new(moved, primary);
        self.core.goal_x = goal;
        self.core.reveal = true;
    }

    /// After a horizontal move, a caret inside a fold jumps past it.
    fn skip_folds(&mut self, forward: bool) {
        if self.core.folds.is_empty() {
            return;
        }
        let folds: Vec<(usize, usize)> = self.core.folds.ranges().to_vec();
        let ranges: Vec<Sel> = self.core.sel.ranges().to_vec();
        let moved: Vec<Sel> = ranges
            .iter()
            .map(|r| {
                let (line, _) = self.line_col(r.head);
                match folds.iter().find(|&&(s, e)| s < line && line <= e) {
                    Some(&(s, e)) => {
                        let to = if forward {
                            if e + 1 < self.lines() {
                                self.rope.line_to_char(e + 1)
                            } else {
                                self.chars_len()
                            }
                        } else {
                            self.line_chars(s).end
                        };
                        Sel::new(r.anchor, to)
                    }
                    None => *r,
                }
            })
            .collect();
        self.core.sel = Selections::new(moved, self.core.sel.primary_index());
    }

    /// Ctrl/Cmd+D: selects the word at the caret, or adds the next
    /// occurrence of the primary selection (wrapping around).
    fn select_next_occurrence(&mut self) {
        let p = self.core.sel.primary();
        if p.is_empty() {
            let (line, col) = self.line_col(p.head);
            if let Some(w) = ops::word_at(self.line_text(line), col) {
                let start = self.line_chars(line).start;
                let mut sel = self.core.sel.clone();
                sel.map(|s| {
                    if s == p {
                        Sel::new(start + w.start, start + w.end)
                    } else {
                        s
                    }
                });
                self.core.sel = sel;
            }
            return;
        }
        let (bf, bt) = (self.char_to_byte(p.from()), self.char_to_byte(p.to()));
        let query = self.text[bf..bt].to_owned();
        let after = self
            .core
            .sel
            .ranges()
            .iter()
            .map(|r| self.char_to_byte(r.to()))
            .max()
            .unwrap_or(bt);
        let found = self.text[after..]
            .find(&query)
            .map(|i| after + i)
            .or_else(|| self.text.find(&query));
        if let Some(b) = found {
            let from = self.rope.byte_to_char(b);
            let to = from + query.chars().count();
            if self
                .core
                .sel
                .ranges()
                .iter()
                .any(|r| r.from() == from && r.to() == to)
            {
                return;
            }
            self.core.sel.push(Sel::new(from, to));
            self.core.reveal = true;
            self.core_unfold_carets();
        }
    }

    // --- Pointer -------------------------------------------------------------

    /// Clicks, drags and double/triple clicks; returns the character under
    /// the pointer when something needs it.
    #[allow(clippy::too_many_arguments)]
    fn core_pointer(
        &mut self,
        ui: &Ui,
        lay: &Lay,
        response: &Response,
        origin: Pos2,
        text_clip: Rect,
        time: f64,
        events: &mut Vec<EditorEvent>,
    ) -> Option<usize> {
        let ctx = ui.ctx().clone();
        let _ = events;
        let pointer = ctx.pointer_latest_pos();
        let in_text = pointer.is_some_and(|p| text_clip.contains(p));
        if response.hovered() && in_text {
            ctx.set_cursor_icon(egui::CursorIcon::Text);
        }
        let dropped = ui.input(|i| !i.raw.dropped_files.is_empty());
        let needs = response.clicked()
            || response.secondary_clicked()
            || response.dragged()
            || response.is_pointer_button_down_on()
            || dropped
            || egui::DragAndDrop::has_any_payload(&ctx)
            || ui.input(|i| i.pointer.any_released());
        let at = pointer
            .filter(|p| needs && response.rect.contains(*p))
            .map(|p| self.hit(ui, lay, p - origin));

        let mods = ui.input(|i| i.modifiers);
        let pressed = response.hovered()
            && in_text
            && ui.input(|i| i.pointer.button_pressed(PointerButton::Primary));
        if pressed {
            ctx.memory_mut(|m| m.request_focus(self.id));
            if let (Some(c), Some(p)) = (at, pointer) {
                self.core.last_interaction = time;
                if mods.alt {
                    let _ = p;
                    let (line, col) = self.line_col(c);
                    self.core.drag = Some(Drag::Rect { line, col });
                    self.core.sel = Selections::single(Sel::caret(c));
                } else if mods.command {
                    // Ctrl/Cmd+click on a link opens it (on release); elsewhere it adds a caret.
                    if self.core_link_at(c).is_none() {
                        self.core.sel.push(Sel::caret(c));
                        self.core.drag = Some(Drag::Text);
                    }
                } else if mods.shift {
                    let p0 = self.core.sel.primary();
                    self.core.sel = Selections::single(Sel::new(p0.anchor, c));
                    self.core.drag = Some(Drag::Text);
                } else {
                    self.core.sel = Selections::single(Sel::caret(c));
                    self.core.drag = Some(Drag::Text);
                }
                self.core.goal_x = None;
            }
        }
        if response.double_clicked() && !mods.command {
            if let Some(c) = at {
                let (line, col) = self.line_col(c);
                let g = ops::group_at(self.line_text(line), col);
                let start = self.line_chars(line).start;
                self.core.sel = Selections::single(Sel::new(start + g.start, start + g.end));
            }
        }
        if response.triple_clicked() && !mods.command {
            if let Some(c) = at {
                // The line's text only, not its line break, so copying or
                // typing over it leaves the following line where it was.
                let (line, _) = self.line_col(c);
                let chars = self.line_chars(line);
                // Notes saved with Windows line endings keep their `\r`.
                let end = if chars.end > chars.start
                    && self.text[self.char_to_byte(chars.end - 1)..].starts_with('\r')
                {
                    chars.end - 1
                } else {
                    chars.end
                };
                self.core.sel = Selections::single(Sel::new(chars.start, end));
            }
        }
        if let (Some(drag), true) = (self.core.drag, response.dragged_by(PointerButton::Primary)) {
            if let Some(p) = pointer {
                let local = p - origin;
                match drag {
                    Drag::Text => {
                        let c = self.hit(ui, lay, local);
                        let mut sel = self.core.sel.clone();
                        let primary = sel.primary();
                        sel.map(|s| {
                            if s == primary {
                                Sel::new(s.anchor, c)
                            } else {
                                s
                            }
                        });
                        self.core.sel = sel;
                    }
                    Drag::Rect { line, col } => {
                        let c = self.hit(ui, lay, local);
                        self.rect_select(line, col, c);
                    }
                }
                // Scroll while dragging past the edges.
                let clip = ui.clip_rect();
                let dy = if p.y < clip.top() {
                    p.y - clip.top()
                } else if p.y > clip.bottom() {
                    p.y - clip.bottom()
                } else {
                    0.0
                };
                if dy != 0.0 {
                    ui.scroll_with_delta(vec2(0.0, -dy.clamp(-40.0, 40.0)));
                    ctx.request_repaint();
                }
            }
        }
        if !ui.input(|i| i.pointer.primary_down()) {
            self.core.drag = None;
        }
        at
    }

    /// Alt+drag: one range per visible line from `line` to the line of
    /// character `c`, from column `col` to `c`'s column (clamped to each
    /// line's length).
    fn rect_select(&mut self, line: usize, col: usize, c: usize) {
        let (to_line, to_col) = self.line_col(c);
        let (a, b) = (line.min(to_line), line.max(to_line));
        let mut sels = Vec::new();
        for l in a..=b {
            if self.core.folds.hidden(l) {
                continue;
            }
            let chars = self.line_chars(l);
            let len = chars.end - chars.start;
            sels.push(Sel::new(
                chars.start + col.min(len),
                chars.start + to_col.min(len),
            ));
        }
        if !sels.is_empty() {
            let primary = if to_line >= line { sels.len() - 1 } else { 0 };
            self.core.sel = Selections::new(sels, primary);
        }
    }

    // --- Folding -------------------------------------------------------------

    /// The last line a fold at `line` would hide (cached per text version).
    pub(crate) fn foldable_end(&mut self, line: usize) -> Option<usize> {
        if self.core.fold_ends.0 != self.version {
            self.core.fold_ends = (self.version, Default::default());
        }
        if let Some(e) = self.core.fold_ends.1.get(&line) {
            return *e;
        }
        let n = self.lines();
        let end = fold::foldable_end(
            line,
            n,
            |i| self.line_text(i),
            |i| self.highlighter.line_blocks(i),
        );
        self.core.fold_ends.1.insert(line, end);
        end
    }

    /// Folds or unfolds at `line` (the fold gutter).
    pub(crate) fn toggle_fold(&mut self, line: usize) {
        if self.core.folds.unfold_at(line) {
            return;
        }
        if let Some(end) = self.foldable_end(line) {
            self.core.folds.fold(line, end);
            self.move_carets_out_of_folds();
        }
    }

    /// Ctrl/Cmd+Shift+[: folds the innermost foldable range holding the caret.
    fn fold_at_caret(&mut self) {
        let (line, _) = self.line_col(self.core.sel.primary().head);
        let lowest = line.saturating_sub(5_000);
        for s in (lowest..=line).rev() {
            if self.core.folds.folded_at(s) {
                continue;
            }
            if let Some(end) = self.foldable_end(s) {
                if end >= line {
                    self.core.folds.fold(s, end);
                    self.move_carets_out_of_folds();
                    return;
                }
            }
        }
    }

    /// Ctrl/Cmd+Shift+]: unfolds at the caret's line.
    fn unfold_at_caret(&mut self) {
        let (line, _) = self.line_col(self.core.sel.primary().head);
        if !self.core.folds.unfold_at(line) {
            self.core.folds.unfold_hiding(line);
        }
    }

    /// Carets hidden by a new fold move to the end of its first line.
    fn move_carets_out_of_folds(&mut self) {
        let folds: Vec<(usize, usize)> = self.core.folds.ranges().to_vec();
        let ranges: Vec<Sel> = self.core.sel.ranges().to_vec();
        let moved: Vec<Sel> = ranges
            .iter()
            .map(|r| {
                let (line, _) = self.line_col(r.head);
                match folds.iter().find(|&&(s, e)| s < line && line <= e) {
                    Some(&(s, _)) => Sel::caret(self.line_chars(s).end),
                    None => *r,
                }
            })
            .collect();
        self.core.sel = Selections::new(moved, self.core.sel.primary_index());
    }

    // --- Painting ------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    fn paint(
        &mut self,
        ui: &Ui,
        lay: &Lay,
        origin: Pos2,
        clip: Rect,
        text_clip: Rect,
        focused: bool,
        time: f64,
    ) {
        let painter = ui.painter().with_clip_rect(text_clip);
        let visuals = ui.visuals().clone();
        let colors = &lay.colors;
        let right = clip.right();
        let left = text_clip.left();
        let visible = self.core.visible.clone();
        let ranges: Vec<Sel> = self.core.sel.ranges().to_vec();
        let caret_lines: Vec<usize> = ranges.iter().map(|r| self.line_col(r.head).0).collect();
        let mut overlay = Vec::new();

        self.update_core_decor(visible.start, visible.end.saturating_sub(1));
        let find_matches: Vec<Range<usize>> = if self.find.open {
            self.find.matches(&self.text, self.version);
            let lo = self.rope.line_to_char(visible.start);
            let hi = if visible.end < self.lines() {
                self.rope.line_to_char(visible.end)
            } else {
                self.chars_len()
            };
            let all = self.find.char_matches();
            let i = all.partition_point(|m| m.end < lo);
            all[i..]
                .iter()
                .take_while(|m| m.start <= hi)
                .cloned()
                .collect()
        } else {
            Vec::new()
        };
        let primary = self.core.sel.primary();
        for line in visible.clone() {
            if self.core.folds.hidden(line) {
                continue;
            }
            let g = self.galley(ui, lay, line);
            let top = origin.y + self.core.tops[line];
            let pos = pos2(origin.x, top);
            if caret_lines.contains(&line) {
                painter.rect_filled(
                    Rect::from_min_max(pos2(left, top), pos2(right, top + g.size().y)),
                    0.0,
                    colors.active_line,
                );
            }
            let chars = self.line_chars(line);
            painter.galley(pos, g.clone(), colors.text);
            // Selections over the text: code spans paint an opaque
            // background that would hide them behind it (the palette's
            // selection colour is translucent, so the text shows through).
            for r in &ranges {
                if r.is_empty() || r.to() < chars.start || r.from() > chars.end {
                    continue;
                }
                let a = r.from().max(chars.start) - chars.start;
                let b = r.to().min(chars.end) - chars.start;
                let past_end = r.to() > chars.end;
                for rect in row_rects(&g, a, b, past_end, lay.font_size * 0.4) {
                    overlay.push(Shape::rect_filled(
                        rect.translate(pos.to_vec2()),
                        0.0,
                        visuals.selection.bg_fill,
                    ));
                }
            }
            if self.core.folds.folded_at(line) {
                let end = g
                    .rows
                    .last()
                    .map_or(pos, |r| pos + vec2(r.rect().right(), r.rect().top()));
                let r = Rect::from_min_size(end + vec2(6.0, 2.0), vec2(22.0, lay.row_h - 4.0));
                painter.rect_filled(r, 3.0, colors.code_bg);
                painter.text(
                    r.center(),
                    Align2::CENTER_CENTER,
                    "…",
                    FontId::proportional(lay.font_size * 0.9),
                    colors.muted,
                );
            }
            // Bracket and selection matches, on top (code spans paint their own background).
            for (l, cols) in &self.decor.matches {
                if *l == line {
                    if let Some(r) = cols_rect(&g, cols.clone()) {
                        overlay.push(Shape::rect_filled(
                            r.translate(pos.to_vec2()),
                            2.0,
                            colors.selection_match,
                        ));
                    }
                }
            }
            if let Some(((bl, bc), other)) = self.decor.brackets {
                let fill = if other.is_some() {
                    colors.bracket_match
                } else {
                    colors.bracket_unmatched
                };
                for (l, c) in std::iter::once((bl, bc)).chain(other) {
                    if l == line {
                        if let Some(r) = cols_rect(&g, c..c + 1) {
                            overlay.push(Shape::rect_filled(r.translate(pos.to_vec2()), 2.0, fill));
                        }
                    }
                }
            }
            for m in &find_matches {
                if m.end < chars.start || m.start > chars.end {
                    continue;
                }
                let a = m.start.max(chars.start) - chars.start;
                let b = m.end.min(chars.end) - chars.start;
                let fill = if (m.start, m.end) == (primary.from(), primary.to()) {
                    colors.find_current
                } else {
                    colors.find_match
                };
                for rect in row_rects(&g, a, b, false, 0.0) {
                    overlay.push(Shape::rect_filled(rect.translate(pos.to_vec2()), 2.0, fill));
                }
            }
            if let Some((s, n)) = self.core.ime {
                if s + n >= chars.start && s <= chars.end {
                    let a = s.max(chars.start) - chars.start;
                    let b = (s + n).min(chars.end) - chars.start;
                    for rect in row_rects(&g, a, b, false, 0.0) {
                        let r = rect.translate(pos.to_vec2());
                        overlay.push(Shape::hline(
                            r.x_range(),
                            r.bottom() - 1.0,
                            Stroke::new(1.0, colors.text),
                        ));
                    }
                }
            }
        }
        painter.extend(overlay);

        // Carets.
        if focused {
            let blink = &visuals.text_cursor;
            let since = time - self.core.last_interaction;
            let period = (blink.on_duration + blink.off_duration) as f64;
            let on = !blink.blink || period <= 0.0 || since % period < blink.on_duration as f64;
            if on {
                for r in &ranges {
                    let (line, _) = self.line_col(r.head);
                    if !visible.contains(&line) || self.core.folds.hidden(line) {
                        continue;
                    }
                    let c = self.caret_rect(ui, lay, r.head).translate(origin.to_vec2());
                    painter.line_segment(
                        [c.center_top(), c.center_bottom()],
                        Stroke::new(blink.stroke.width, blink.stroke.color),
                    );
                }
            }
            if blink.blink {
                let phase = since % period;
                let next = if on {
                    blink.on_duration as f64 - phase
                } else {
                    period - phase
                };
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_secs_f64(next.max(0.01)));
            }
        }
    }

    /// The gutter: line numbers and the fold chevrons (for foldable lines
    /// while the gutter is hovered, and for folded lines always).
    fn paint_gutter_and_folds(&mut self, ui: &Ui, lay: &Lay, origin: Pos2, clip: Rect) {
        let painter = ui.painter();
        let left = clip.left();
        let gutter = Rect::from_x_y_ranges(left..=left + lay.gutter, clip.y_range());
        painter.rect_filled(gutter, 0.0, lay.colors.gutter);
        painter.vline(
            left + lay.gutter,
            clip.y_range(),
            Stroke::new(1.0, theme::palette(ui.ctx()).border),
        );
        let hovered = ui
            .ctx()
            .pointer_hover_pos()
            .is_some_and(|p| gutter.contains(p));
        let active = self.line_col(self.core.sel.primary().head).0;
        let font = FontId::new(self.prefs.font_size * 0.86, FontFamily::Monospace);
        let mut toggle = None;
        for line in self.core.visible.clone() {
            if self.core.folds.hidden(line) {
                continue;
            }
            let top = origin.y + self.core.tops[line];
            let h = self
                .highlighter
                .measured_size(line)
                .map_or(lay.row_h, |s| s.y)
                .min(lay.row_h * 1.6);
            if self.prefs.line_numbers {
                painter.text(
                    pos2(left + lay.gutter - FOLD_W - 6.0, top + h / 2.0),
                    Align2::RIGHT_CENTER,
                    (line + 1).to_string(),
                    font.clone(),
                    if line == active {
                        lay.colors.gutter_active
                    } else {
                        lay.colors.gutter_text
                    },
                );
            }
            let folded = self.core.folds.folded_at(line);
            if folded || (hovered && self.foldable_end(line).is_some()) {
                let r = Rect::from_min_size(pos2(left + lay.gutter - FOLD_W, top), vec2(FOLD_W, h));
                let label = format!(
                    "{} line {}",
                    if folded { "Unfold" } else { "Fold" },
                    line + 1
                );
                let resp = ui
                    .interact(r, self.id.with(("fold", line)), Sense::click())
                    .on_hover_text(if folded { "Unfold" } else { "Fold" });
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
                let glyph = if folded {
                    icons::CHEVRON_RIGHT
                } else {
                    icons::CHEVRON_DOWN
                };
                painter.text(
                    r.center(),
                    Align2::CENTER_CENTER,
                    glyph,
                    icons::font(lay.font_size * 0.6),
                    if resp.hovered() {
                        lay.colors.text
                    } else {
                        lay.colors.gutter_text
                    },
                );
                if resp.clicked() {
                    toggle = Some(line);
                }
            }
        }
        if let Some(line) = toggle {
            self.toggle_fold(line);
            // Keep typing in the note after a click on a chevron (as CodeMirror).
            ui.ctx().memory_mut(|m| m.request_focus(self.id));
            ui.ctx().request_repaint();
        }
    }

    /// The bracket pair at the caret and other occurrences of the selected
    /// word in the visible lines.
    fn update_core_decor(&mut self, first_line: usize, last_line: usize) {
        let p = self.core.sel.primary();
        let key = (self.version, p.anchor, p.head, first_line, last_line);
        if self.decor.key == Some(key) {
            return;
        }
        self.decor.key = Some(key);
        let line_col = |s: &Self, b: usize| {
            let c = s.rope.byte_to_char(b);
            s.line_col(c)
        };
        self.decor.brackets = if p.is_empty() && !self.core.sel.is_multi() {
            matching::bracket_match(&self.text, self.char_to_byte(p.head))
                .map(|m| (line_col(self, m.at), m.other.map(|o| line_col(self, o))))
        } else {
            None
        };
        self.decor.matches.clear();
        let (bf, bt) = (self.char_to_byte(p.from()), self.char_to_byte(p.to()));
        let Some((query, whole)) = matching::selection_query(&self.text, bf, bt) else {
            return;
        };
        let query = query.to_owned();
        let query_chars = query.chars().count();
        for line in first_line..=last_line.min(self.lines().saturating_sub(1)) {
            let start = self.line_bytes(line).start;
            for r in matching::find_in_line(self.line_text(line), &query, whole) {
                if start + r.start == bf {
                    continue;
                }
                let col = self.line_text(line)[..r.start].chars().count();
                self.decor.matches.push((line, col..col + query_chars));
                if self.decor.matches.len() >= MAX_SELECTION_MATCHES {
                    return;
                }
            }
        }
    }

    /// The `[[` completion after an edit or caret move.
    fn update_core_completion(&mut self, focused: bool) {
        let p = self.core.sel.primary();
        if !focused || !p.is_empty() || self.core.sel.is_multi() {
            self.completion = None;
            return;
        }
        // On the caret's line only (the helper converts offsets by scanning
        // its text from the start).
        let (line, col) = self.line_col(p.head);
        let found = crate::edits::wikilink_query(self.line_text(line), col)
            .map(|(from, q)| (self.line_chars(line).start + from, q));
        let Some((from, query)) = found else {
            self.completion = None;
            self.completion_dismissed = None;
            return;
        };
        if self.completion_dismissed == Some(from) {
            return;
        }
        let q = query.to_lowercase();
        let items: Vec<usize> = self
            .link_targets
            .iter()
            .enumerate()
            .filter(|(_, n)| n.to_lowercase().contains(&q))
            .map(|(i, _)| i)
            .take(MAX_COMPLETIONS)
            .collect();
        if items.is_empty() {
            self.completion = None;
            return;
        }
        let selected = self
            .completion
            .as_ref()
            .filter(|c| c.from == from)
            .map_or(0, |c| c.selected.min(items.len() - 1));
        self.completion = Some(Completion {
            from,
            items,
            selected,
        });
    }

    /// Where the completion list goes: under the primary caret.
    pub(crate) fn core_completion_pos(&mut self, ui: &Ui, lay: &Lay) -> Pos2 {
        let head = self.core.sel.primary().head;
        let origin = self.core.origin;
        self.caret_rect(ui, lay, head)
            .translate(origin.to_vec2())
            .left_bottom()
    }

    /// Screen readers get the visible lines (as AccessKit text runs) and the
    /// selection when it's among them: no per-frame cost that grows with the note.
    fn core_accesskit(&mut self, ui: &Ui, lay: &Lay, origin: Pos2) {
        let ctx = ui.ctx().clone();
        if ctx.accesskit_node_builder(self.id, |_| ()).is_none() {
            return;
        }
        let lines: Vec<usize> = self
            .core
            .visible
            .clone()
            .filter(|&l| !self.core.folds.hidden(l))
            .collect();
        let Some(&first) = lines.first() else { return };
        let mut window = String::new();
        let mut starts = Vec::with_capacity(lines.len());
        let mut galleys = Vec::with_capacity(lines.len());
        let mut chars = 0;
        for (k, &l) in lines.iter().enumerate() {
            if k > 0 {
                window.push('\n');
                chars += 1;
            }
            starts.push((l, chars));
            let t = self.line_text(l);
            window.push_str(t);
            chars += t.chars().count();
            galleys.push(self.galley(ui, lay, l));
        }
        let map = |s: &Self, c: usize| -> Option<CCursor> {
            let (line, col) = s.line_col(c);
            starts
                .iter()
                .find(|(l, _)| *l == line)
                .map(|(_, at)| CCursor::new(at + col))
        };
        let p = self.core.sel.primary();
        let range = match (map(self, p.anchor), map(self, p.head)) {
            (Some(a), Some(h)) => Some(CCursorRange::two(a, h)),
            _ => None,
        };
        let job = merged_job(&window, &lay.opts(), lay.look);
        let galley = Galley::concat(Arc::new(job), &galleys, ctx.pixels_per_point());
        ctx.accesskit_node_builder(self.id, |b| {
            b.set_role(egui::accesskit::Role::MultilineTextInput);
            b.set_value(window.clone());
        });
        let to_global = ctx
            .layer_transform_to_global(ui.layer_id())
            .unwrap_or_default();
        egui::text_selection::accesskit_text::update_accesskit_for_text_widget(
            &ctx,
            self.id,
            range,
            to_global
                * TSTransform::from_translation(
                    (origin + vec2(0.0, self.core.tops[first])).to_vec2(),
                ),
            &galley,
        );
    }

    /// On idle frames with soft wrap: lays out a few off-screen lines to
    /// replace estimated heights; drops galleys far from the viewport past
    /// the budget.
    fn measure_offscreen(&mut self, ui: &Ui, lay: &Lay) {
        let busy = ui.input(|i| {
            i.events
                .iter()
                .any(|e| matches!(e, Event::Text(_) | Event::Key { .. } | Event::Paste(_)))
        });
        if lay.wrap && !busy {
            // Only on frames that happen anyway (no repaint is requested for
            // this): the estimates are good enough to scroll with, and each
            // measured line makes the scrollbar and positions more exact.
            let start = std::time::Instant::now();
            let n = self.lines();
            let mut i = self.core.measured_upto.min(n);
            while i < n && start.elapsed().as_secs_f64() < MEASURE_BUDGET_S {
                if self.highlighter.measured_size(i).is_none() && !self.core.folds.hidden(i) {
                    self.galley(ui, lay, i);
                }
                i += 1;
            }
            self.core.measured_upto = if i >= n { 0 } else { i };
        }
        if self.highlighter.galley_count() > MAX_GALLEYS {
            let v = &self.core.visible;
            let keep = v.start.saturating_sub(KEEP_AROUND)..v.end + KEEP_AROUND;
            self.highlighter.evict_galleys(keep);
        }
    }
}

/// The rectangles covering columns `a..b` of a line's galley, one per row
/// (relative to the galley). `past_end` extends the last one a little, to
/// show a selected newline.
fn row_rects(g: &Galley, a: usize, b: usize, past_end: bool, newline_w: f32) -> Vec<Rect> {
    let mut out = Vec::new();
    let mut start = 0;
    let rows = g.rows.len();
    for (i, row) in g.rows.iter().enumerate() {
        let n = row.char_count_excluding_newline().0;
        let end = start + n;
        let last = i + 1 == rows;
        let (lo, hi) = (a.max(start), b.min(end));
        if lo < hi || (past_end && last && b >= end && a <= end) {
            let x0 = row.pos.x + row.x_offset(egui::epaint::text::CharIndex(lo - start));
            let mut x1 =
                row.pos.x + row.x_offset(egui::epaint::text::CharIndex(hi.max(lo) - start));
            if past_end && last {
                x1 += newline_w;
            }
            out.push(Rect::from_min_max(
                pos2(x0, row.pos.y),
                pos2(x1.max(x0 + 1.0), row.pos.y + row.size.y),
            ));
        }
        start = end;
    }
    out
}
