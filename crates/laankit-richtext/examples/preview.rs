//! A Markdown note rendered the way NoteLiner's preview does it (comrak HTML through
//! `from_html`), for checking task lists, aligned tables, `<mark>`, `<kbd>` and selection by eye.
//!
//! ```bash
//! cargo run -p laankit-richtext --example preview [note.md]
//! WAYLAND_DISPLAY= LAANKIT_THEME=light LAANKIT_SCREENSHOT=/tmp/preview.png LAANKIT_SCREENSHOT_EXIT=1 \
//!   cargo run -p laankit-richtext --example preview
//! ```

use laankit_egui::diagnostics::{self, Screenshot};
use laankit_egui::theme::ThemeState;
use laankit_richtext::{from_html, Document, Options, RichStyle, RichView, WebResolver};

const SAMPLE: &str = "\
# Preview sample

Tasks for the week:

- [x] Port the preview
- [ ] Check the **aligned** table
  1. [x] Left, centre and right
  2. [ ] A column without alignment
- A plain bullet

| Item      | Qty | Price  | Note       |
|:----------|:---:|-------:|------------|
| Apples    |  3  |  $1.20 | fresh      |
| Pears     | 12  | $14.75 | ripe       |
| Total     | 15  | $15.95 |            |

Press <kbd>Ctrl</kbd>+<kbd>C</kbd> to copy, and <mark>highlighted text</mark> stands out.
Double-click a word, triple-click a paragraph, or Shift+click to extend.
";

struct Preview {
    doc: Document,
    view: RichView,
    screenshot: Option<Screenshot>,
}

impl eframe::App for Preview {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let style = RichStyle::from_ctx(ui.ctx());
        egui::CentralPanel::default().show(ui, |ui| {
            self.view.show(ui, &self.doc, &style, &mut WebResolver);
        });
        if let Some(shot) = &mut self.screenshot {
            shot.tick(ui.ctx());
        }
    }
}

fn main() -> eframe::Result {
    let md = std::env::args()
        .nth(1)
        .map(|p| std::fs::read_to_string(p).expect("read the note"))
        .unwrap_or_else(|| SAMPLE.to_string());
    let html = noteliner_core::export::markdown_to_html(&md);
    let doc = from_html(&html, &Options::default());
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([820.0, 620.0]),
        ..Default::default()
    };
    eframe::run_native(
        "laankit-richtext preview",
        options,
        Box::new(move |cc| {
            laankit_egui::fonts::install(&cc.egui_ctx);
            let mut theme = ThemeState::default();
            diagnostics::theme_overrides(&mut theme);
            theme.apply(&cc.egui_ctx);
            Ok(Box::new(Preview {
                doc,
                view: RichView::default(),
                screenshot: Screenshot::from_env(),
            }))
        }),
    )
}
