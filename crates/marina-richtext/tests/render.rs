//! Renders a document through egui_kittest in a light and a dark theme, and checks it through
//! the accessibility tree.

use egui::accesskit::Role;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use marina_egui::theme::{ThemeId, ThemeState};
use marina_richtext::{from_article_html, Document, RichStyle, RichView, WebResolver};

const HTML: &str = r#"
<h1>Release notes</h1>
<p>Some <strong>bold</strong>, <em>italic</em> and <code>code</code>, plus
<a href="https://example.com/more">a link</a>.</p>
<ul><li>First</li><li>Second</li></ul>
<blockquote><p>A quote.</p></blockquote>
<pre>let x = 1;</pre>
<table><tr><th>Name</th><th>Value</th></tr><tr><td>a</td><td>1</td></tr></table>
"#;

struct State {
    doc: Document,
    view: RichView,
    theme: ThemeState,
    ready: bool,
}

fn harness(theme: ThemeId) -> Harness<'static, State> {
    let state = State {
        doc: from_article_html(HTML, Some("https://example.com/post")),
        view: RichView::default(),
        theme: ThemeState {
            theme,
            ..Default::default()
        },
        ready: false,
    };
    Harness::builder()
        .with_size(egui::vec2(800.0, 600.0))
        .build_ui_state(
            |ui, s: &mut State| {
                if !s.ready {
                    // Fonts are installed in one frame and used from the next.
                    marina_egui::fonts::install(ui.ctx());
                    s.theme.apply(ui.ctx());
                    s.ready = true;
                    return;
                }
                let style = RichStyle::from_ctx(ui.ctx());
                s.view.show(ui, &s.doc, &style, &mut WebResolver);
            },
            state,
        )
}

#[test]
fn renders_in_light_and_dark_themes() {
    for theme in [ThemeId::Light, ThemeId::Midnight] {
        let mut h = harness(theme);
        h.run();
        let style = RichStyle::from_ctx(&h.ctx);
        assert_eq!(style.text, theme.palette().text_primary);
        assert_eq!(h.ctx.global_style().visuals.dark_mode, theme.palette().dark);
        for text in ["Release notes", "First", "A quote.", "let x = 1;", "Value"] {
            h.get_by_label_contains(text);
        }
        assert!(h.query_all_by_role(Role::Label).count() >= 8);
        assert!(h.state().view.settled());
    }
}
