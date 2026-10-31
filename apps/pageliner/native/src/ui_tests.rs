//! Drives the app headlessly (egui_kittest) through the main flow, against a
//! temp config folder and a generated PDF: import → open → select text and
//! highlight it → back to the library and reopen, with the highlight and
//! page restored from `state/<id>.json` → remove the book.

use std::path::Path;
use std::time::Duration;

use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use serde_json::Value;

use crate::app::PageLinerApp;
use crate::config::Paths;
use crate::reader::pdf::fixtures;

fn harness(paths: Paths) -> Harness<'static, PageLinerApp> {
    Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .with_context_setup(marina_egui::fonts::install)
        .build_ui_state(
            |ui, app: &mut PageLinerApp| app.show(ui),
            PageLinerApp::new(None, paths),
        )
}

/// Steps the UI until `cond` holds (the backend and PDF workers run on
/// other threads).
fn wait_for(
    h: &mut Harness<'static, PageLinerApp>,
    what: &str,
    mut cond: impl FnMut(&mut PageLinerApp) -> bool,
) {
    for _ in 0..750 {
        h.step();
        if cond(h.state_mut()) {
            h.step();
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("timed out waiting for {what}");
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn import_open_highlight_reopen_and_remove() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = Paths {
        user_data: tmp.path().join("PageLiner"),
    };
    let library = paths.library_dir();
    let pdf = tmp.path().join("Field Notes.pdf");
    std::fs::write(
        &pdf,
        fixtures::text_pdf(&[&["First page"], &["Hello world", "Second line"], &["Last"]]),
    )
    .unwrap();
    let mut h = harness(paths.clone());

    // An empty library.
    wait_for(&mut h, "start-up", |a| a.is_ready());
    h.get_by_label("Import a book…");

    // Import: the new book opens in the PDF reader.
    h.state_mut().import_paths(vec![pdf]);
    wait_for(&mut h, "import", |a| a.books().len() == 1);
    let id = h.state().books()[0].id.clone();
    assert_eq!(h.state().books()[0].title, "Field Notes");
    assert!(library.join("books").join(format!("{id}.pdf")).exists());
    wait_for(&mut h, "the reader", |a| a.pdf_reader().is_some());
    assert_eq!(h.state_mut().pdf_reader().unwrap().page_count(), 3);
    h.get_by_label("Back to Library");
    h.get_by_label("Go to page");

    // Select "Hello world" on page 2 and highlight it.
    h.state_mut()
        .pdf_reader()
        .unwrap()
        .select(1, [73.0, 90.0], [130.0, 95.0]);
    wait_for(&mut h, "the page text", |a| {
        let r = a.pdf_reader().unwrap();
        if r.has_text(1) {
            r.select(1, [73.0, 90.0], [130.0, 95.0]);
            true
        } else {
            false
        }
    });
    h.get_by_label("Highlight selection").click();
    h.run_steps(2);
    let state_file = library.join("state").join(format!("{id}.json"));
    wait_for(&mut h, "the state file", |_| {
        state_file.exists() && read_json(&state_file)["highlights"].is_array()
    });
    let state = read_json(&state_file);
    let hl = &state["highlights"][0];
    assert_eq!(hl["kind"], "highlight");
    assert_eq!(hl["page"], 2);
    assert_eq!(hl["text"], "Hello world");
    assert!((hl["rects"][0]["x"].as_f64().unwrap() - 72.0).abs() < 0.5);
    assert!(state["lastOpenedAt"].is_string());

    // Jump to page 3 (as the pager does), then back to the library: the
    // position is flushed.
    h.get_by_label("Go to page").click();
    h.run_steps(2);
    // The page field is the toolbar's, so the first text input.
    h.get_all_by_role(egui::accesskit::Role::TextInput)
        .next()
        .expect("the page field")
        .type_text("3");
    h.step();
    h.key_press(egui::Key::Enter);
    wait_for(&mut h, "page 3", |a| {
        a.pdf_reader().unwrap().current_page() == 3
    });
    h.get_by_label("Back to Library").click();
    wait_for(&mut h, "the library", |a| a.selected().is_none());
    wait_for(&mut h, "the saved page", |_| {
        read_json(&state_file)["page"] == 3
    });
    assert_eq!(read_json(&state_file)["format"], "pdf");

    // Reopen: the highlight comes back from the state file.
    h.get_by_label("Field Notes").click();
    wait_for(&mut h, "the reader again", |a| a.pdf_reader().is_some());
    let restored = h.state_mut().pdf_reader().unwrap().highlights().to_vec();
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].text, "Hello world");
    wait_for(&mut h, "the restored page", |a| {
        a.pdf_reader().unwrap().current_page() == 3
    });

    // Remove it from the library, confirming.
    h.get_by_label("Back to Library").click();
    wait_for(&mut h, "the library", |a| a.selected().is_none());
    h.get_by_label("Remove Field Notes").click();
    h.run_steps(3);
    assert!(h.state().has_overlay());
    h.get_by_label("Remove").click();
    wait_for(&mut h, "the removal", |a| a.books().is_empty());
    assert!(!state_file.exists());
    assert!(!library.join("books").join(format!("{id}.pdf")).exists());
    let index = read_json(&library.join("pageliner.json"));
    assert_eq!(index["books"].as_array().unwrap().len(), 0);
}
