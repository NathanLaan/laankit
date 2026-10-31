//! The Properties pane, below the diagram list: what's selected, and its
//! Fill and Stroke (the current value, Default and the presets as swatches,
//! a hex field), plus Reset Style.
//!
//! A row whose shapes don't all agree shows **Mixed**: a split swatch, an
//! empty field with a "Mixed" placeholder, and no swatch ringed. Picking a
//! swatch or entering a hex value sets it on all of them. Each row is judged
//! on its own, so one can be shared while the other is Mixed.

use blockliner_core::style::{Colour, Common, PRESETS};
use blockliner_core::{Diagram, ShapeKind};
use egui::{vec2, Rect, Sense, Ui};
use marina_egui::dialog::{self, ButtonKind};
use marina_egui::panes::HEADER_HEIGHT;
use marina_egui::{fonts, theme};

use crate::canvas::paint::{self, SwatchLook};
use crate::canvas::{current_swatch, SelectionStyle, Swatch};
use crate::overlays::colour::INVALID;

/// The pane's height when there's no saved one.
pub const DEFAULT_HEIGHT: f32 = 380.0;
/// Its smallest height, and the least the diagram list keeps above it.
pub const MIN_HEIGHT: f32 = 120.0;

/// What the pane asks for. The app maps them to the same commands as the
/// right-click menu and the palette.
#[derive(Clone, Debug, PartialEq)]
pub enum PropertiesAction {
    Fill(Swatch),
    Stroke(Swatch),
    FillHex(Colour),
    StrokeHex(Colour),
    ResetStyle,
}

/// The pane's own state between frames: its two hex fields.
#[derive(Default)]
pub struct PropertiesState {
    fill: HexField,
    stroke: HexField,
}

#[derive(Default)]
struct HexField {
    text: String,
    /// The value the field last showed; while the field isn't being typed
    /// in, a different current value replaces the text.
    shown: Option<Common>,
    error: Option<&'static str>,
}

/// What the pane shows.
pub struct PropertiesProps {
    /// "2 shapes, 1 line", "Rectangle “API Gateway”", "Nothing selected".
    pub summary: String,
    pub style: SelectionStyle,
}

impl PropertiesProps {
    pub fn new(d: &Diagram, selection: &[String], style: SelectionStyle) -> Self {
        Self {
            summary: summary(d, selection, style.lines),
            style,
        }
    }

    /// Why the rows are off, if they are.
    fn reason(&self) -> Option<String> {
        if self.style.shapes > 0 || self.style.lines > 0 {
            return None;
        }
        Some(match self.style.external.as_slice() {
            [] => "Select shapes or lines to change their colours.".into(),
            [id] => format!("“{id}” is declared in a kept Mermaid statement."),
            _ => "These shapes are declared in kept Mermaid statements.".into(),
        })
    }

    /// Whether the Fill row is on: lines have no fill.
    fn fills(&self) -> bool {
        self.style.shapes > 0
    }

    /// What a change won't touch, beside what it will.
    fn note(&self) -> Option<String> {
        if self.reason().is_some() {
            return None;
        }
        let kept = match self.style.external.len() {
            0 => None,
            1 => Some("1 kept-statement shape not affected.".to_owned()),
            n => Some(format!("{n} kept-statement shapes not affected.")),
        };
        let lines = (!self.fills()).then(|| "Lines take a stroke colour only.".to_owned());
        match (lines, kept) {
            (Some(a), Some(b)) => Some(format!("{a} {b}")),
            (a, b) => a.or(b),
        }
    }
}

/// "Nothing selected", a single shape's kind and label, or the counts.
pub fn summary(d: &Diagram, selection: &[String], lines: usize) -> String {
    let count = |n: usize, one: &str| match n {
        0 => None,
        1 => Some(format!("1 {one}")),
        n => Some(format!("{n} {one}s")),
    };
    match (selection, lines) {
        ([], 0) => "Nothing selected".into(),
        ([id], 0) => match d.shape(id) {
            Some(s) => {
                let kind = match s.kind {
                    ShapeKind::Rect => "Rectangle",
                    ShapeKind::RoundRect => "Rounded Rectangle",
                    ShapeKind::Text => "Text",
                    ShapeKind::Diamond => "Diamond",
                    ShapeKind::Hexagon => "Hexagon",
                    ShapeKind::Database => "Database",
                    ShapeKind::Subroutine => "Subroutine",
                    ShapeKind::Stadium => "Stadium",
                    ShapeKind::Circle => "Circle",
                    ShapeKind::Parallelogram => "Parallelogram",
                    ShapeKind::Trapezoid => "Trapezoid",
                    ShapeKind::Other { .. } => "Shape",
                };
                let label = s.label.lines().next().unwrap_or("").trim();
                if label.is_empty() {
                    kind.into()
                } else {
                    format!("{kind} “{label}”")
                }
            }
            None => "Nothing selected".into(),
        },
        (shapes, lines) => [count(shapes.len(), "shape"), count(lines, "line")]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(", "),
    }
}

/// Draws the pane in `ui` (its own area, below the list): a header like
/// the list's, then the contents, scrolling when the pane is short.
pub fn show(
    ui: &mut Ui,
    state: &mut PropertiesState,
    props: &PropertiesProps,
) -> Option<PropertiesAction> {
    let p = theme::palette(ui.ctx());
    let (header, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), HEADER_HEIGHT), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(header, 0.0, p.bg_base);
    for y in [header.top() + 0.5, header.bottom() - 0.5] {
        painter.hline(header.x_range(), y, egui::Stroke::new(1.0, p.border));
    }
    painter.text(
        egui::pos2(header.left() + 16.0, header.center().y),
        egui::Align2::LEFT_CENTER,
        "PROPERTIES",
        fonts::Style {
            weight: Some(fonts::SEMIBOLD),
            ..Default::default()
        }
        .font(11.0),
        p.text_muted,
    );
    egui::ScrollArea::vertical()
        .id_salt("blockliner-properties")
        .auto_shrink([false, false])
        .show(ui, |ui| contents(ui, state, props))
        .inner
}

fn contents(
    ui: &mut Ui,
    state: &mut PropertiesState,
    props: &PropertiesProps,
) -> Option<PropertiesAction> {
    let p = theme::palette(ui.ctx());
    let mut out = None;
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.add_space(16.0);
        ui.vertical(|ui| {
            ui.set_width((ui.available_width() - 16.0).max(80.0));
            ui.label(
                egui::RichText::new(&props.summary)
                    .size(12.0)
                    .color(p.text_secondary),
            );
            ui.add_space(8.0);
            let reason = props.reason();
            ui.add_enabled_ui(reason.is_none(), |ui| {
                for stroke in [false, true] {
                    let field = if stroke {
                        &mut state.stroke
                    } else {
                        &mut state.fill
                    };
                    let current = if stroke {
                        &props.style.stroke
                    } else {
                        &props.style.fill
                    };
                    let on = stroke || props.fills();
                    let picked = ui
                        .add_enabled_ui(on, |ui| row(ui, field, current, stroke))
                        .inner;
                    if let Some(a) = picked {
                        out = Some(a);
                    }
                    ui.add_space(10.0);
                }
                if dialog::button(ui, "Reset Style", ButtonKind::Secondary, reason.is_none())
                    .clicked()
                {
                    out = Some(PropertiesAction::ResetStyle);
                }
            });
            if let Some(text) = reason.or_else(|| props.note()) {
                ui.add_space(6.0);
                dialog::muted_text(ui, &text);
            }
        });
    });
    out
}

/// One property: its name, the current value's swatch and hex field, then
/// Default and the presets.
fn row(
    ui: &mut Ui,
    field: &mut HexField,
    current: &Common,
    stroke: bool,
) -> Option<PropertiesAction> {
    let p = theme::palette(ui.ctx());
    let name = if stroke { "Stroke" } else { "Fill" };
    let mut out = None;
    dialog::field_label(ui, name);

    let (look, value) = match current {
        Common::Mixed => (SwatchLook::Mixed, "Mixed".to_owned()),
        Common::Same(Some(c)) => (
            paint::colour(Some(c)).map_or(SwatchLook::Default, |c| SwatchLook::Colour {
                fill: if stroke { p.bg_base } else { c },
                edge: if stroke { c } else { p.border },
                outline: stroke,
            }),
            c.text.clone(),
        ),
        Common::Same(None) | Common::None => (SwatchLook::Default, "Default".to_owned()),
    };
    let hint = match current {
        Common::Mixed => "Mixed",
        Common::Same(None) => "Default",
        _ => "#rrggbb",
    };
    let resp = ui
        .horizontal(|ui| {
            let (square, swatch) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::hover());
            swatch.widget_info(|| {
                let current = format!("Current {}: {value}", name.to_lowercase());
                egui::WidgetInfo::labeled(egui::Role::Image, true, current)
            });
            paint::swatch(ui.painter(), square, look, false, &p);
            dialog::text_input(ui, &mut field.text, hint, true)
        })
        .inner;

    // A value typed in is applied when the field lets go (Enter or a click
    // elsewhere); a bad one is refused with the reason.
    let shown_text = match &field.shown {
        Some(Common::Same(Some(c))) => c.text.clone(),
        _ => String::new(),
    };
    if resp.changed() {
        field.error = None;
    }
    if resp.lost_focus() && field.text.trim() != shown_text {
        if field.text.trim().is_empty() {
            field.text = shown_text;
        } else {
            match Colour::custom(&field.text) {
                Some(c) if stroke => out = Some(PropertiesAction::StrokeHex(c)),
                Some(c) => out = Some(PropertiesAction::FillHex(c)),
                None => field.error = Some(INVALID),
            }
        }
    }
    // Not being typed in: follow the selection (a refusal stays until it
    // changes).
    if !resp.has_focus() && field.shown.as_ref() != Some(current) {
        field.text = match current {
            Common::Same(Some(c)) => c.text.clone(),
            _ => String::new(),
        };
        field.shown = Some(current.clone());
        field.error = None;
        ui.ctx().request_repaint();
    }
    if let Some(e) = field.error {
        ui.add_space(2.0);
        dialog::error_text(ui, e);
    }

    // Default and the presets, wrapping in a narrow sidebar.
    ui.add_space(6.0);
    let ringed = current_swatch(current, stroke);
    let enabled = ui.is_enabled();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        let mut swatch = |ui: &mut Ui, look: SwatchLook, label: String, s: Swatch| {
            let (rect, resp) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, &label));
            let square = Rect::from_center_size(rect.center(), vec2(16.0, 16.0));
            if resp.hovered() {
                ui.painter().rect_filled(rect, 4.0, p.bg_item_hover);
            }
            paint::swatch(ui.painter(), square, look, ringed == Some(s), &p);
            if resp.on_hover_text(label).clicked() {
                out = Some(if stroke {
                    PropertiesAction::Stroke(s)
                } else {
                    PropertiesAction::Fill(s)
                });
            }
        };
        swatch(
            ui,
            SwatchLook::Default,
            format!("{name}: Default"),
            Swatch::Default,
        );
        for (i, preset) in PRESETS.iter().enumerate() {
            let look = paint::preset_look(i, stroke, &p);
            swatch(
                ui,
                look,
                format!("{name}: {}", preset.name),
                Swatch::Preset(i),
            );
        }
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use blockliner_core::{Rect as ModelRect, Shape};

    #[test]
    fn summaries() {
        let mut d = Diagram::new();
        d.shapes.push(Shape::new(
            "api",
            ShapeKind::Rect,
            "API Gateway\nv2",
            ModelRect::new(0.0, 0.0, 10.0, 10.0),
        ));
        d.shapes.push(Shape::new(
            "x",
            ShapeKind::Text,
            "",
            ModelRect::new(0.0, 0.0, 10.0, 10.0),
        ));
        let ids = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(summary(&d, &[], 0), "Nothing selected");
        assert_eq!(summary(&d, &ids(&["api"]), 0), "Rectangle “API Gateway”");
        assert_eq!(summary(&d, &ids(&["x"]), 0), "Text");
        assert_eq!(summary(&d, &ids(&["api", "x"]), 1), "2 shapes, 1 line");
        assert_eq!(summary(&d, &[], 3), "3 lines");
    }

    #[test]
    fn the_ringed_swatch() {
        let c = Colour::parse;
        let same = |v: Option<&str>| Common::Same(v.map(c));
        assert_eq!(current_swatch(&same(None), false), Some(Swatch::Default));
        assert_eq!(
            current_swatch(&same(Some("#DBEAFE")), false),
            Some(Swatch::Preset(1))
        );
        assert_eq!(
            current_swatch(&same(Some("#3b82f6")), true),
            Some(Swatch::Preset(1))
        );
        assert_eq!(
            current_swatch(&same(Some("#123456")), false),
            None,
            "custom"
        );
        assert_eq!(current_swatch(&Common::Mixed, false), None);
        assert_eq!(current_swatch(&Common::None, false), None);
    }

    #[test]
    fn reasons_and_notes() {
        let props = |shapes, lines, external: &[&str]| PropertiesProps {
            summary: String::new(),
            style: SelectionStyle {
                fill: Common::None,
                stroke: Common::None,
                shapes,
                lines,
                external: external.iter().map(|s| s.to_string()).collect(),
            },
        };
        assert_eq!(
            props(0, 0, &[]).reason().unwrap(),
            "Select shapes or lines to change their colours."
        );
        assert_eq!(props(0, 2, &[]).reason(), None);
        assert_eq!(
            props(0, 2, &[]).note().unwrap(),
            "Lines take a stroke colour only."
        );
        assert_eq!(
            props(0, 0, &["c"]).reason().unwrap(),
            "“c” is declared in a kept Mermaid statement."
        );
        assert_eq!(props(2, 0, &[]).reason(), None);
        assert_eq!(props(2, 0, &[]).note(), None);
        assert_eq!(props(1, 1, &[]).note(), None);
        assert_eq!(
            props(1, 2, &["c"]).note().unwrap(),
            "1 kept-statement shape not affected."
        );
        assert_eq!(
            props(0, 1, &["c", "d"]).note().unwrap(),
            "Lines take a stroke colour only. 2 kept-statement shapes not affected."
        );
    }
}
