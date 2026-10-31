//! The command palette, natively: `CommandPalette.svelte`.
//!
//! A modal overlay with a search field and up to [`MAX_ITEMS`] results:
//! the registry's applicable commands, plus whatever domain items (notes,
//! tags, recent files, …) the app supplies for the current query. Results
//! are ranked with [`fuzzy_score`]; the matched part of each label is
//! highlighted, and a divider separates runs of different kinds.
//!
//! The app keeps an `Option<CommandPalette>` and calls [`CommandPalette::show`]
//! every frame while it is `Some`; the [`PaletteOutcome`] says whether to keep
//! it open and which action to run.

use egui::text::LayoutJob;
use egui::{
    pos2, vec2, Align2, Area, Color32, CornerRadius, FontFamily, FontId, Frame, Id, Key, Margin,
    Modal, Order, Rect, RichText, ScrollArea, Sense, Stroke, StrokeKind, TextEdit, TextFormat, Ui,
};

use crate::commands::{fuzzy_score, Registry};
use crate::icons;
use crate::theme::{self, Palette};

/// Results shown at most.
pub const MAX_ITEMS: usize = 50;
/// `.palette { width: 600px; max-width: 90vw; max-height: 50vh; min-height: 240px }`.
const WIDTH: f32 = 600.0;
const MIN_HEIGHT: f32 = 240.0;
/// `padding-top: 18vh` on the backdrop.
const TOP_FRACTION: f32 = 0.18;
/// `.palette-row { padding: 10px 18px }` around 14 px text.
const ROW_HEIGHT: f32 = 38.0;
const ROW_PAD_X: f32 = 18.0;
/// `border-radius: 10px`.
const RADIUS: CornerRadius = CornerRadius::same(10);

/// What a result is; picks its icon and groups the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemKind {
    Command,
    Note,
    Tag,
    Recent,
    Other,
}

impl ItemKind {
    fn icon(self) -> &'static str {
        match self {
            ItemKind::Command => icons::BOLT,
            ItemKind::Note => icons::FILE_LINES,
            ItemKind::Tag => icons::TAG,
            ItemKind::Recent => icons::FOLDER,
            ItemKind::Other => icons::CIRCLE,
        }
    }
}

/// One result row. Apps return these from the extra-items provider.
#[derive(Clone, Debug)]
pub struct PaletteItem<A> {
    pub kind: ItemKind,
    /// Unique among the results.
    pub id: String,
    pub label: String,
    /// The right-hand chip (a shortcut, a path). When empty, the section is
    /// shown there instead.
    pub sub: String,
    pub section: String,
    /// Higher sorts first; use [`fuzzy_score`] for consistency.
    pub score: i32,
    pub action: A,
}

/// What happened this frame.
#[derive(Clone, Debug, PartialEq)]
pub enum PaletteOutcome<A> {
    /// Still open.
    Open,
    /// Dismissed (Escape, or a click outside).
    Closed,
    /// An item was picked; the palette is done.
    Run(A),
}

/// The palette's own state: the query and the selected row.
#[derive(Default)]
pub struct CommandPalette {
    pub query: String,
    selected: usize,
    /// The query the selection belongs to; a new query resets it.
    selected_for: Option<String>,
    focused_once: bool,
    scroll_to_selected: bool,
    placeholder: Option<String>,
}

impl CommandPalette {
    pub fn new() -> Self {
        Self::default()
    }

    /// A different search-field placeholder (default "Type a command...").
    pub fn with_placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    /// The results for the current query: commands, then the provider's
    /// extra items, sorted by score and capped at [`MAX_ITEMS`].
    pub fn items<A: Clone, C>(
        &self,
        registry: &Registry<A, C>,
        ctx: &C,
        extra: &mut dyn FnMut(&str) -> Vec<PaletteItem<A>>,
    ) -> Vec<PaletteItem<A>> {
        let q = self.query.trim();
        let mut out: Vec<PaletteItem<A>> = Vec::new();
        for c in registry.applicable_commands(ctx) {
            let score = fuzzy_score(&c.label, q);
            if !q.is_empty() && score == 0 {
                continue;
            }
            out.push(PaletteItem {
                kind: ItemKind::Command,
                id: c.id.clone(),
                label: c.label.clone(),
                sub: c.shortcut_label().unwrap_or_default(),
                section: if c.section.is_empty() {
                    "Command".into()
                } else {
                    c.section.clone()
                },
                score: score + if q.is_empty() { 1000 } else { 0 },
                action: c.action.clone(),
            });
        }
        out.extend(extra(q));
        // Stable, like Array.prototype.sort: ties keep recency/alphabetical order.
        out.sort_by_key(|item| std::cmp::Reverse(item.score));
        out.truncate(MAX_ITEMS);
        out
    }

    /// Draws the palette over everything. Running a command records it in
    /// the registry's recents.
    pub fn show<A: Clone, C>(
        &mut self,
        egui_ctx: &egui::Context,
        registry: &mut Registry<A, C>,
        ctx: &C,
        extra: &mut dyn FnMut(&str) -> Vec<PaletteItem<A>>,
    ) -> PaletteOutcome<A> {
        let items = self.items(registry, ctx, extra);
        if self.selected_for.as_deref() != Some(self.query.as_str()) {
            self.selected = 0;
            self.selected_for = Some(self.query.clone());
        }
        let picked = self.handle_keys(egui_ctx, items.len());
        let p = theme::palette(egui_ctx);

        let screen = egui_ctx.content_rect();
        let width = WIDTH.min(screen.width() * 0.9);
        let max_height = (screen.height() * 0.5).max(MIN_HEIGHT);
        let area = Area::new(Id::unique("laankit-command-palette"))
            .kind(egui::UiKind::Modal)
            .order(Order::Foreground)
            .anchor(
                Align2::CENTER_TOP,
                vec2(0.0, screen.height() * TOP_FRACTION),
            );
        let frame = Frame::new()
            .fill(p.bg_surface)
            .stroke(Stroke::new(1.0, p.border))
            .corner_radius(RADIUS)
            .shadow(egui::epaint::Shadow {
                offset: [0, 16],
                blur: 48,
                spread: 0,
                color: Color32::from_black_alpha(140),
            });
        let mut clicked = None;
        let modal = Modal::new(Id::unique("laankit-command-palette-modal"))
            .area(area)
            .backdrop_color(p.modal_overlay)
            .frame(frame)
            .show(egui_ctx, |ui| {
                ui.set_width(width);
                ui.set_min_height(MIN_HEIGHT);
                ui.set_max_height(max_height);
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                self.input_row(ui, &p);
                clicked = self.list(ui, &p, &items);
            });

        if let Some(i) = picked.or(clicked) {
            let Some(item) = items.get(i) else {
                return PaletteOutcome::Open;
            };
            if item.kind == ItemKind::Command {
                if let Some(action) = registry.run(&item.id, ctx) {
                    return PaletteOutcome::Run(action);
                }
                return PaletteOutcome::Closed;
            }
            return PaletteOutcome::Run(item.action.clone());
        }
        if modal.should_close() {
            return PaletteOutcome::Closed;
        }
        PaletteOutcome::Open
    }

    /// Arrow/Home/End move the selection and Enter picks it, before the
    /// text field sees the keys.
    fn handle_keys(&mut self, ctx: &egui::Context, count: usize) -> Option<usize> {
        let last = count.saturating_sub(1);
        ctx.input_mut(|i| {
            let mut moved = false;
            if i.consume_key(egui::Modifiers::NONE, Key::ArrowDown) {
                self.selected = (self.selected + 1).min(last);
                moved = true;
            }
            if i.consume_key(egui::Modifiers::NONE, Key::ArrowUp) {
                self.selected = self.selected.saturating_sub(1);
                moved = true;
            }
            if i.consume_key(egui::Modifiers::NONE, Key::Home) {
                self.selected = 0;
                moved = true;
            }
            if i.consume_key(egui::Modifiers::NONE, Key::End) {
                self.selected = last;
                moved = true;
            }
            self.scroll_to_selected |= moved;
            (count > 0 && i.consume_key(egui::Modifiers::NONE, Key::Enter))
                .then_some(self.selected.min(last))
        })
    }

    /// `.palette-input-row`: the magnifier and the query field on the input
    /// background, with a hairline below.
    fn input_row(&mut self, ui: &mut Ui, p: &Palette) {
        Frame::new()
            .fill(p.input_bg)
            .inner_margin(Margin::symmetric(18, 14))
            .corner_radius(CornerRadius {
                nw: RADIUS.nw,
                ne: RADIUS.ne,
                sw: 0,
                se: 0,
            })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    ui.add_sized(
                        [20.0, 20.0],
                        egui::Label::new(icons::icon(icons::MAGNIFYING_GLASS).color(p.text_muted)),
                    );
                    let placeholder = self.placeholder.as_deref().unwrap_or("Type a command...");
                    let edit = TextEdit::singleline(&mut self.query)
                        .id(Id::unique("laankit-command-palette-query"))
                        .frame(Frame::NONE)
                        .hint_text(RichText::new(placeholder).color(p.text_muted))
                        .text_color(p.text_primary)
                        .font(FontId::proportional(16.0))
                        .desired_width(f32::INFINITY);
                    let resp = ui.add(edit);
                    resp.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::Role::SearchInput, true, "Search commands")
                    });
                    if !self.focused_once {
                        resp.request_focus();
                        self.focused_once = true;
                    }
                });
            });
        let r = ui.min_rect();
        ui.painter()
            .hline(r.x_range(), r.bottom(), Stroke::new(1.0, p.border));
    }

    /// `.palette-list`; returns a clicked row.
    fn list<A>(&mut self, ui: &mut Ui, p: &Palette, items: &[PaletteItem<A>]) -> Option<usize> {
        let mut clicked = None;
        ScrollArea::vertical()
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.add_space(6.0);
                if items.is_empty() {
                    ui.add_space(18.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("No matching commands.")
                                .size(13.0)
                                .color(p.text_muted),
                        );
                    });
                    ui.add_space(18.0);
                    return;
                }
                for (i, item) in items.iter().enumerate() {
                    if i > 0 && items[i - 1].kind != item.kind {
                        // `.palette-divider`: 1 px, 6 px above and below, 14 px in.
                        let (rect, _) = ui
                            .allocate_exact_size(vec2(ui.available_width(), 13.0), Sense::hover());
                        ui.painter().hline(
                            rect.x_range().shrink(14.0),
                            rect.center().y,
                            Stroke::new(1.0, p.border),
                        );
                    }
                    if self.row(ui, p, item, i) {
                        clicked = Some(i);
                    }
                }
                ui.add_space(6.0);
            });
        clicked
    }

    /// `.palette-row`: kind icon, highlighted label, and the shortcut chip or
    /// the section. Hovering selects it, as `onmouseenter` does.
    fn row<A>(&mut self, ui: &mut Ui, p: &Palette, item: &PaletteItem<A>, i: usize) -> bool {
        let (rect, resp) =
            ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
        let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
        resp.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::Role::ListBoxOption,
                true,
                i == self.selected,
                &item.label,
            )
        });
        if resp.hovered() && ui.input(|inp| inp.pointer.delta() != egui::Vec2::ZERO) {
            self.selected = i;
        }
        let selected = i == self.selected;
        if selected && self.scroll_to_selected {
            resp.scroll_to_me(None);
            self.scroll_to_selected = false;
        }
        let painter = ui.painter_at(rect);
        if selected {
            painter.rect_filled(rect, 0.0, p.accent.gamma_multiply(0.15));
            painter.rect_filled(
                Rect::from_min_size(rect.min, vec2(3.0, rect.height())),
                0.0,
                p.accent,
            );
        }
        let cy = rect.center().y;
        let mut x = rect.left() + ROW_PAD_X;
        painter.text(
            pos2(x + 12.0, cy),
            Align2::CENTER_CENTER,
            item.kind.icon(),
            icons::font(13.0),
            if selected { p.accent } else { p.text_muted },
        );
        x += 24.0 + 12.0;

        // The right-hand side first, so the label knows how much room it has.
        let mut right = rect.right() - ROW_PAD_X;
        if item.sub.is_empty() {
            let section = item.section.to_uppercase();
            let g = painter.layout_no_wrap(
                section,
                FontId::proportional(10.0),
                p.text_muted.gamma_multiply(0.7),
            );
            right -= g.size().x;
            painter.galley(pos2(right, cy - g.size().y / 2.0), g, p.text_muted);
        } else {
            let g = painter.layout_no_wrap(
                item.sub.clone(),
                FontId::monospace(11.0),
                if selected {
                    p.text_primary
                } else {
                    p.text_secondary
                },
            );
            let chip = Rect::from_min_size(
                pos2(
                    right - g.size().x.min(240.0) - 16.0,
                    cy - g.size().y / 2.0 - 3.0,
                ),
                vec2(g.size().x.min(240.0) + 16.0, g.size().y + 6.0),
            );
            painter.rect(
                chip,
                theme::RADIUS,
                p.bg_button,
                Stroke::new(1.0, if selected { p.accent } else { p.border }),
                StrokeKind::Inside,
            );
            painter.with_clip_rect(chip.shrink(1.0)).galley(
                chip.min + vec2(8.0, 3.0),
                g,
                p.text_secondary,
            );
            right = chip.left();
        }

        let mut job = highlight_job(&item.label, self.query.trim(), p, selected);
        job.wrap.max_width = (right - 12.0 - x).max(0.0);
        job.wrap.max_rows = 1;
        job.wrap.break_anywhere = true;
        job.wrap.overflow_character = Some('…');
        let g = ui.fonts_mut(|f| f.layout_job(job));
        painter.galley(pos2(x, cy - g.size().y / 2.0), g, p.text_primary);
        resp.clicked()
    }
}

/// The label with the first case-insensitive occurrence of `query` marked,
/// as the Svelte `highlight()` does.
fn highlight_job(label: &str, query: &str, p: &Palette, selected: bool) -> LayoutJob {
    let font = FontId::new(theme::BODY_SIZE, FontFamily::Proportional);
    let plain = TextFormat {
        font_id: font.clone(),
        color: p.text_primary,
        ..Default::default()
    };
    let mark = TextFormat {
        background: p.accent.gamma_multiply(if selected { 0.45 } else { 0.30 }),
        ..plain.clone()
    };
    let mut job = LayoutJob::default();
    match match_range(label, query) {
        Some((a, b)) => {
            job.append(&label[..a], 0.0, plain.clone());
            job.append(&label[a..b], 0.0, mark);
            job.append(&label[b..], 0.0, plain);
        }
        None => job.append(label, 0.0, plain),
    }
    job
}

/// The byte range of the first case-insensitive match of `query` in
/// `label`, if lowercasing keeps the byte offsets aligned.
fn match_range(label: &str, query: &str) -> Option<(usize, usize)> {
    if query.is_empty() {
        return None;
    }
    let lower = label.to_lowercase();
    if lower.len() != label.len() {
        return None;
    }
    let start = lower.find(&query.to_lowercase())?;
    let end = start + query.len();
    (label.is_char_boundary(start) && label.is_char_boundary(end)).then_some((start, end))
}

#[cfg(test)]
mod tests {
    use egui_kittest::kittest::Queryable;
    use egui_kittest::Harness;

    use super::*;
    use crate::commands::{Binding, Command};

    #[derive(Clone, Debug, PartialEq)]
    enum Act {
        New,
        Open,
        Zoom,
        Note(String),
    }

    struct State {
        palette: Option<CommandPalette>,
        registry: Registry<Act>,
        ran: Vec<Act>,
        closed: bool,
        with_notes: bool,
    }

    fn state() -> State {
        let mut registry = Registry::new();
        registry.register(
            Command::new("file.new", "New File", "File", Act::New)
                .key(Binding::logical(egui::Modifiers::COMMAND, Key::N))
                .shortcut("Ctrl+N"),
        );
        registry.register(Command::new("file.open", "Open Project", "File", Act::Open));
        registry.register(Command::new("view.zoom", "Zoom In", "View", Act::Zoom));
        State {
            palette: Some(CommandPalette::new()),
            registry,
            ran: Vec::new(),
            closed: false,
            with_notes: false,
        }
    }

    fn harness(state: State) -> Harness<'static, State> {
        Harness::builder()
            .with_context_setup(crate::fonts::install)
            .with_size(vec2(900.0, 700.0))
            .build_ui_state(
                |ui, s: &mut State| {
                    let Some(palette) = s.palette.as_mut() else {
                        return;
                    };
                    let with_notes = s.with_notes;
                    let mut extra = |q: &str| -> Vec<PaletteItem<Act>> {
                        if !with_notes {
                            return Vec::new();
                        }
                        ["Meeting notes", "Zebra facts"]
                            .iter()
                            .filter_map(|label| {
                                let score = fuzzy_score(label, q);
                                (score > 0).then(|| PaletteItem {
                                    kind: ItemKind::Note,
                                    id: format!("note:{label}"),
                                    label: label.to_string(),
                                    sub: String::new(),
                                    section: "Notes".into(),
                                    score,
                                    action: Act::Note(label.to_string()),
                                })
                            })
                            .collect()
                    };
                    match palette.show(ui.ctx(), &mut s.registry, &(), &mut extra) {
                        PaletteOutcome::Open => {}
                        PaletteOutcome::Closed => {
                            s.closed = true;
                            s.palette = None;
                        }
                        PaletteOutcome::Run(a) => {
                            s.ran.push(a);
                            s.palette = None;
                        }
                    }
                },
                state,
            )
    }

    #[test]
    fn filter_and_enter_runs_the_top_match() {
        let mut h = harness(state());
        h.run();
        h.get_by_label("Search commands").type_text("zoom");
        h.run();
        assert!(h.query_by_label("New File").is_none(), "filtered out");
        h.key_press(Key::Enter);
        h.run();
        assert_eq!(h.state().ran, vec![Act::Zoom]);
        assert_eq!(
            h.state().registry.recents().ids,
            vec!["view.zoom".to_string()]
        );
    }

    #[test]
    fn arrows_home_end_move_the_selection() {
        let mut h = harness(state());
        h.run();
        // Alphabetical with no recents: New File, Open Project, Zoom In.
        h.key_press(Key::ArrowDown);
        h.key_press(Key::ArrowDown);
        h.key_press(Key::ArrowDown);
        h.run();
        h.key_press(Key::ArrowUp);
        h.run();
        h.key_press(Key::Enter);
        h.run();
        assert_eq!(h.state().ran, vec![Act::Open]);

        let mut h = harness(state());
        h.run();
        h.key_press(Key::End);
        h.run();
        h.key_press(Key::Enter);
        h.run();
        assert_eq!(h.state().ran, vec![Act::Zoom]);
    }

    #[test]
    fn extra_items_merge_by_score() {
        let mut s = state();
        s.with_notes = true;
        let mut h = harness(s);
        h.run();
        h.get_by_label("Search commands").type_text("zebra");
        h.run();
        h.key_press(Key::Enter);
        h.run();
        assert_eq!(h.state().ran, vec![Act::Note("Zebra facts".into())]);
    }

    #[test]
    fn escape_closes_and_click_runs() {
        let mut h = harness(state());
        h.run();
        h.key_press(Key::Escape);
        h.run();
        assert!(h.state().closed);

        let mut h = harness(state());
        h.run();
        h.get_by_label("Open Project").click();
        h.run();
        assert_eq!(h.state().ran, vec![Act::Open]);
    }

    #[test]
    fn match_range_is_case_insensitive() {
        assert_eq!(match_range("New File", "file"), Some((4, 8)));
        assert_eq!(match_range("New File", ""), None);
        assert_eq!(match_range("New File", "xyz"), None);
    }
}
