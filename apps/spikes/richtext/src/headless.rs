//! Lays documents out without a window, to time them: the first frame of a document measures
//! every block; later frames only lay out what's near the viewport.

use std::time::{Duration, Instant};

use egui::{Pos2, RawInput, Rect, ScrollArea};

use crate::fonts::{self, Face};
use crate::model::Document;
use crate::render::{Position, RichView, Theme};

pub struct Headless {
    ctx: egui::Context,
    theme: Theme,
    size: egui::Vec2,
}

/// One document's layout timings.
#[derive(Clone, Copy, Debug, Default)]
pub struct LayoutTimes {
    /// The first frame: every block measured.
    pub measure: Duration,
    /// A later frame at the top (culled).
    pub steady: Duration,
    /// A frame after jumping to the middle.
    pub jump: Duration,
    pub content_height: f32,
    pub laid_out: usize,
}

impl Headless {
    pub fn new(face: Face, width: f32, height: f32) -> Self {
        let ctx = egui::Context::default();
        let installed = fonts::install(&ctx);
        let real = match face {
            Face::Sans => installed.sans,
            Face::Serif => installed.serif,
        };
        let theme = Theme::from_visuals(&egui::Visuals::dark(), face, real);
        let headless = Self {
            ctx,
            theme,
            size: egui::vec2(width, height),
        };
        // Fonts take effect from the next frame.
        headless.frame(&mut RichView::default(), &Document::default(), None);
        headless
    }

    /// Times a document's first frame, a steady frame, and a jump to its middle block.
    pub fn time(&self, doc: &Document) -> LayoutTimes {
        let mut view = RichView::default();
        let (measure, _) = self.frame(&mut view, doc, None);
        let (steady, h) = self.frame(&mut view, doc, None);
        let laid_out = view.laid_out;
        view.restore(Position {
            block: doc.blocks.len() / 2,
            offset: 0,
        });
        self.frame(&mut view, doc, None);
        let target = view.take_scroll_target();
        let (jump, _) = self.frame(&mut view, doc, target);
        LayoutTimes {
            measure,
            steady,
            jump,
            content_height: h,
            laid_out,
        }
    }

    fn frame(&self, view: &mut RichView, doc: &Document, offset: Option<f32>) -> (Duration, f32) {
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.size)),
            ..Default::default()
        };
        let start = Instant::now();
        let mut height = 0.0;
        let mut out = self.ctx.run_ui(input, |ui| {
            egui::CentralPanel::default_margins().show(ui, |ui| {
                let mut area = ScrollArea::vertical().auto_shrink(false);
                if let Some(y) = offset {
                    area = area.vertical_scroll_offset(y);
                }
                let out = area.show_viewport(ui, |ui, viewport| {
                    view.show(ui, viewport, doc, &self.theme, &mut |_, _| None)
                });
                height = out.content_size.y;
            });
        });
        let _ = self
            .ctx
            .tessellate(std::mem::take(&mut out.shapes), out.pixels_per_point);
        out.drop_without_applying_deltas();
        (start.elapsed(), height)
    }
}

impl Headless {
    /// Restores `pos`, lets the scroll land, and reads the position back.
    pub fn round_trip(&self, doc: &Document, pos: Position) -> Position {
        let mut view = RichView::default();
        view.restore(pos);
        let mut offset = None;
        for _ in 0..4 {
            self.frame(&mut view, doc, offset);
            offset = view.take_scroll_target().or(offset);
        }
        view.position()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::convert::{from_html, Options};

    #[test]
    fn positions_round_trip() {
        let para = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(30);
        let html: String = (0..40).map(|i| format!("<p>{i} {para}</p>")).collect();
        let doc = from_html(&html, &Options::default());
        let headless = Headless::new(Face::Serif, 900.0, 700.0);
        for pos in [
            Position {
                block: 0,
                offset: 0,
            },
            Position {
                block: 12,
                offset: 0,
            },
            Position {
                block: 25,
                offset: 700,
            },
        ] {
            let back = headless.round_trip(&doc, pos);
            assert_eq!(back.block, pos.block, "{pos:?} → {back:?}");
            // The offset comes back as the start of the line that holds it.
            assert!(
                back.offset <= pos.offset && pos.offset - back.offset < 150,
                "{pos:?} → {back:?}"
            );
        }
    }
}
