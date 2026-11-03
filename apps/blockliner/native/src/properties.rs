//! The Properties pane, below the diagram list: what's selected, Change
//! Shape (the selected shapes' kind) and Change Line (whether the selected
//! lines have arrowheads), then Fill and Stroke (the current value, Default
//! and the presets as swatches, a hex field), Stroke Style (solid, dashed or
//! dotted), Stroke Weight (1px … 4px), Text Weight (Normal or Bold) and Text
//! Style (Normal or Italic), plus Reset Style.
//!
//! A row whose shapes don't all agree shows **Mixed**: a split swatch, an
//! empty field with a "Mixed" placeholder, and no swatch ringed. Picking a
//! swatch or entering a hex value sets it on all of them. Each row is judged
//! on its own, so one can be shared while the other is Mixed.

use blockliner_core::style::{
    Agreement, Colour, Common, Dash, DashCommon, Weight, PRESETS, WEIGHTS,
};
use blockliner_core::{Diagram, ShapeKind};
use egui::{pos2, vec2, Align2, CornerRadius, FontId, Rect, Sense, Stroke, StrokeKind, Ui};
use laankit_egui::dialog::{self, ButtonKind};
use laankit_egui::panes::HEADER_HEIGHT;
use laankit_egui::{fonts, icons, theme};

use crate::canvas::paint::{self, SwatchLook};
use crate::canvas::{
    current_swatch, Agreed, SelectionKinds, SelectionStyle, Swatch, Tool, STROKE_STYLES,
};
use crate::overlays::colour::INVALID;
use crate::views::Glyph;

/// The pane's height when there's no saved one: room for everything with
/// both Change Shape and Change Line showing (shapes and lines selected).
pub const DEFAULT_HEIGHT: f32 = 680.0;
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
    /// Change Shape: the selected shapes become this kind.
    ChangeShape(ShapeKind),
    /// Change Line: the selected lines become what this line tool draws
    /// (Line, Arrow, Right-Angle Line or Right-Angle Arrow).
    ChangeLine(Tool),
    /// Stroke Style: Solid (`None`), Dashed or Dotted.
    StrokeStyle(Option<Dash>),
    /// Stroke Weight (`None`: the default, 1.5px).
    StrokeWeight(Option<Weight>),
    /// Text Weight: Bold (`true`) or Normal.
    TextWeight(bool),
    /// Text Style: Italic (`true`) or Normal.
    TextStyle(bool),
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
    pub kinds: SelectionKinds,
}

impl PropertiesProps {
    pub fn new(
        d: &Diagram,
        selection: &[String],
        style: SelectionStyle,
        kinds: SelectionKinds,
    ) -> Self {
        Self {
            summary: summary(d, selection, style.lines),
            style,
            kinds,
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
        match self.style.external.len() {
            0 => None,
            1 => Some("1 kept-statement shape not affected.".to_owned()),
            n => Some(format!("{n} kept-statement shapes not affected.")),
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
                let kind = s.kind.name();
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
            // Change Shape: also shown (off) for kept-statement shapes alone,
            // whose reason is below.
            if props.style.shapes > 0 || !props.style.external.is_empty() {
                let on = props.style.shapes > 0;
                if let Some(kind) = ui
                    .add_enabled_ui(on, |ui| shape_row(ui, &props.kinds.shape))
                    .inner
                {
                    out = Some(PropertiesAction::ChangeShape(kind));
                }
                ui.add_space(10.0);
            }
            if props.style.lines > 0 {
                if let Some(tool) = line_row(ui, &props.kinds.line_tool) {
                    out = Some(PropertiesAction::ChangeLine(tool));
                }
                ui.add_space(10.0);
            }
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
                if let Some(dash) = dash_row(ui, &props.style.dash) {
                    out = Some(PropertiesAction::StrokeStyle(dash));
                }
                ui.add_space(10.0);
                if let Some(weight) = weight_row(ui, &props.style.weight) {
                    out = Some(PropertiesAction::StrokeWeight(weight));
                }
                ui.add_space(10.0);
                // Text Weight and Text Style side by side: both are short.
                ui.columns(2, |cols| {
                    if let Some(bold) = text_row(&mut cols[0], true, &props.style.bold) {
                        out = Some(PropertiesAction::TextWeight(bold));
                    }
                    if let Some(italic) = text_row(&mut cols[1], false, &props.style.italic) {
                        out = Some(PropertiesAction::TextStyle(italic));
                    }
                });
                ui.add_space(10.0);
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

/// Change Shape: the selected shapes' kind (or Mixed), opening a list of
/// every kind. The kind picked, unless they all already are it.
fn shape_row(ui: &mut Ui, current: &Agreed<ShapeKind>) -> Option<ShapeKind> {
    dialog::field_label(ui, "Change Shape:");
    let shown = match current {
        Agreed::Same(kind) => Some((
            Glyph::Tool(Tool::for_shape(kind).unwrap_or(Tool::Rect)),
            kind.name(),
        )),
        _ => None,
    };
    let mut picked = None;
    kind_select(ui, "Change Shape", shown, |ui, width| {
        for kind in ShapeKind::CHANGEABLE {
            let icon = Tool::for_shape(&kind).unwrap_or(Tool::Rect);
            let active = *current == Agreed::Same(kind.clone());
            if crate::views::icon_row(ui, icon, kind.name(), None, active, width).clicked() {
                picked = Some(kind);
            }
        }
    });
    picked.filter(|k| *current != Agreed::Same(k.clone()))
}

/// Change Line: Line, Arrow, Right-Angle Line or Right-Angle Arrow (or
/// Mixed) for the selected lines, named and drawn as their tools. The one
/// picked, unless they all already are it.
fn line_row(ui: &mut Ui, current: &Agreed<Tool>) -> Option<Tool> {
    dialog::field_label(ui, "Change Line:");
    let shown = match current {
        Agreed::Same(tool) => Some((Glyph::Tool(*tool), tool.name())),
        _ => None,
    };
    let mut picked = None;
    kind_select(ui, "Change Line", shown, |ui, width| {
        for tool in Tool::LINES {
            let active = *current == Agreed::Same(tool);
            if crate::views::icon_row(ui, tool, tool.name(), None, active, width).clicked() {
                picked = Some(tool);
            }
        }
    });
    picked.filter(|t| *current != Agreed::Same(*t))
}

/// Stroke Style: Solid, Dashed or Dotted (or Mixed, or a hand-written
/// Custom dash) for the selected shapes and lines. The style picked,
/// unless they all already have it.
fn dash_row(ui: &mut Ui, current: &DashCommon) -> Option<Option<Dash>> {
    dialog::field_label(ui, "Stroke Style:");
    let shown = match current {
        DashCommon::Same(dash) => Some((Glyph::Dash(dash.clone()), Dash::name(dash.as_ref()))),
        DashCommon::None => Some((Glyph::Dash(None), "Solid")),
        DashCommon::Mixed => None,
    };
    let mut picked = None;
    kind_select(ui, "Stroke Style", shown, |ui, width| {
        for dash in STROKE_STYLES {
            let active = *current == DashCommon::Same(dash.clone());
            let name = Dash::name(dash.as_ref());
            let glyph = Glyph::Dash(dash.clone());
            if crate::views::icon_row(ui, glyph, name, None, active, width).clicked() {
                picked = Some(dash);
            }
        }
    });
    picked.filter(|d| *current != DashCommon::Same(d.clone()))
}

/// Stroke Weight: 1px … 4px, with 1.5px the default (or Mixed, or a
/// hand-written Custom weight). The weight picked, unless they all already
/// have it.
fn weight_row(ui: &mut Ui, current: &Agreement<Option<Weight>>) -> Option<Option<Weight>> {
    dialog::field_label(ui, "Stroke Weight:");
    let name = match current {
        Agreement::Same(w) => Some(Weight::label(w.as_ref())),
        Agreement::None => Some(Weight::label(None)),
        Agreement::Mixed => None,
    };
    let shown = match current {
        Agreement::Same(w) => Some(Glyph::Weight(w.clone())),
        Agreement::None => Some(Glyph::Weight(None)),
        Agreement::Mixed => None,
    };
    let mut picked = None;
    kind_select(
        ui,
        "Stroke Weight",
        shown.zip(name.as_deref()),
        |ui, width| {
            for weight in WEIGHTS {
                let active = *current == Agreement::Same(weight.clone());
                let name = Weight::label(weight.as_ref());
                let glyph = Glyph::Weight(weight.clone());
                if crate::views::icon_row(ui, glyph, &name, None, active, width).clicked() {
                    picked = Some(weight);
                }
            }
        },
    );
    picked.filter(|w| *current != Agreement::Same(w.clone()))
}

/// Text Weight (`bold`: Normal or Bold) or Text Style (Normal or Italic)
/// for the selected labels. `true` for Bold or Italic, unless they all
/// already are.
fn text_row(ui: &mut Ui, bold: bool, current: &Agreement<bool>) -> Option<bool> {
    let (label, on) = if bold {
        ("Text Weight", "Bold")
    } else {
        ("Text Style", "Italic")
    };
    dialog::field_label(ui, &format!("{label}:"));
    let look = |value: bool| {
        (
            Glyph::Text {
                bold: bold && value,
                italic: !bold && value,
            },
            if value { on } else { "Normal" },
        )
    };
    let shown = match current {
        Agreement::Same(v) => Some(look(*v)),
        Agreement::None => Some(look(false)),
        Agreement::Mixed => None,
    };
    let mut picked = None;
    kind_select(ui, label, shown, |ui, width| {
        for value in [false, true] {
            let (glyph, name) = look(value);
            let active = *current == Agreement::Same(value);
            if crate::views::icon_row(ui, glyph, name, None, active, width).clicked() {
                picked = Some(value);
            }
        }
    });
    picked.filter(|v| *current != Agreement::Same(*v))
}

/// A closed select like Settings' (`laankit_egui::settings::select`), but
/// showing the current value's icon, or "Mixed" when there's no one value;
/// `options` fills the list below it, given the row width.
fn kind_select(
    ui: &mut Ui,
    label: &str,
    current: Option<(Glyph, &str)>,
    options: impl FnOnce(&mut Ui, f32),
) {
    let p = theme::palette(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, ui.is_enabled(), label));
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&resp));
    let fill = if resp.hovered() {
        p.bg_button_hover
    } else {
        p.bg_button
    };
    let edge = if open || resp.has_focus() {
        p.accent
    } else {
        p.border
    };
    let painter = ui.painter();
    painter.rect(
        rect,
        CornerRadius::same(6),
        fill,
        Stroke::new(1.0, edge),
        StrokeKind::Inside,
    );
    let font = FontId::proportional(theme::BODY_SIZE);
    match current {
        Some((icon, name)) => {
            let ink = p.text_primary;
            crate::views::paint_glyph(
                painter,
                &icon,
                pos2(rect.left() + 18.0, rect.center().y),
                ink,
            );
            painter.text(
                pos2(rect.left() + 38.0, rect.center().y),
                Align2::LEFT_CENTER,
                name,
                font,
                ink,
            );
        }
        None => {
            painter.text(
                pos2(rect.left() + 12.0, rect.center().y),
                Align2::LEFT_CENTER,
                "Mixed",
                font,
                p.text_muted,
            );
        }
    }
    painter.text(
        pos2(rect.right() - 12.0, rect.center().y),
        Align2::RIGHT_CENTER,
        icons::CHEVRON_DOWN,
        icons::font(11.0),
        p.text_muted,
    );
    let width = rect.width();
    egui::Popup::menu(&resp)
        .width(width)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
        .show(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            options(ui, width - 8.0);
        });
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
                dash: DashCommon::None,
                weight: Agreement::None,
                bold: Agreement::None,
                italic: Agreement::None,
                shapes,
                lines,
                external: external.iter().map(|s| s.to_string()).collect(),
            },
            kinds: crate::canvas::SelectionKinds {
                shape: Agreed::None,
                line: Agreed::None,
                line_tool: Agreed::None,
            },
        };
        assert_eq!(
            props(0, 0, &[]).reason().unwrap(),
            "Select shapes or lines to change their colours."
        );
        assert_eq!(props(0, 2, &[]).reason(), None);
        assert_eq!(props(0, 2, &[]).note(), None);
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
            "2 kept-statement shapes not affected."
        );
    }
}
