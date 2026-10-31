//! egui_kittest flows for the sidebar panes, panels and dialogs, against a project
//! built with `noteliner-core` in a temp folder: search → jump to the
//! line, backlinks → jump, the history list and an old version, the TAGS
//! pane and tag filter, the tag editor, the MCP confirmation's three
//! decisions, and a PDF export. Needs `git` on the PATH, like the app.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::accesskit::Role;
use egui::{Key, Modifiers};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use noteliner_core::git::NoteGit;
use noteliner_core::project::Project;
use serde_json::json;

use crate::app::NoteLinerApp;
use crate::backend::McpConfirm;
use crate::config::Paths;
use crate::ui_tests::{file_by_name, harness, index, wait_for};

/// Alpha (tagged `work`) with a "needle" on line 2, and Beta linking to it.
fn fixture(dir: &Path) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let mut p = Project::new(NoteGit::new(Arc::new(|_| {})));
        p.init(dir, None).await.unwrap();
        p.set_git_config("Test User", "test@localhost")
            .await
            .unwrap();
        let alpha = p
            .create_file(
                "Alpha",
                &json!(["work"]),
                Some("line one\nfind the needle here\nline three\n".into()),
                None,
            )
            .await
            .unwrap();
        p.add_attachment(alpha["id"].as_str().unwrap(), b"plain text", "notes.txt")
            .await
            .unwrap();
        p.create_file(
            "Beta",
            &json!([]),
            Some("See [[Alpha]] for more.\n".into()),
            None,
        )
        .await
        .unwrap();
    });
}

/// A harness with the fixture project open, and its folder.
fn open_project(tmp: &Path) -> (Harness<'static, NoteLinerApp>, PathBuf) {
    let project = tmp.join("notes");
    std::fs::create_dir_all(&project).unwrap();
    fixture(&project);
    let mut h = harness(Paths {
        user_data: tmp.join("NoteLiner"),
    });
    h.run_steps(2);
    h.state_mut().open(project.clone());
    wait_for(&mut h, "the project", |a| a.entries().len() == 2);
    (h, project)
}

fn id_of(h: &Harness<'static, NoteLinerApp>, name: &str) -> String {
    h.state()
        .entries()
        .iter()
        .find(|e| e.name == name)
        .map(|e| e.id.clone())
        .unwrap()
}

fn select(h: &mut Harness<'static, NoteLinerApp>, name: &str) {
    h.get_by_role_and_label(Role::TreeItem, name).click();
    let id = id_of(h, name);
    wait_for(h, name, move |a| {
        a.editor_file_id().as_deref() == Some(id.as_str())
    });
}

fn close(mut h: Harness<'static, NoteLinerApp>) {
    h.state_mut().backend().close_project(None);
    wait_for(&mut h, "the close", |a| a.project_index().is_none());
}

#[test]
fn search_backlinks_and_history() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, _) = open_project(tmp.path());
    select(&mut h, "Beta");

    // SEARCH: Ctrl+F opens and focuses it; results arrive after the debounce.
    h.key_press_modifiers(Modifiers::COMMAND, Key::F);
    h.run_steps(3);
    h.get_by_label("Search all files").type_text("NEEDLE");
    let alpha = id_of(&h, "Alpha");
    for _ in 0..200 {
        h.step();
        if h.query_by_label("Line 2: find the needle here").is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    h.get_by_label("1 match in 1 file");
    h.get_by_label("Line 2: find the needle here").click();
    let a2 = alpha.clone();
    wait_for(&mut h, "the jump to Alpha line 2", move |a| {
        a.editor_file_id().as_deref() == Some(a2.as_str()) && a.editor_cursor().0 == 2
    });

    // BACKLINKS on Alpha: Beta's line, which jumps to Beta.
    h.state_mut().layout_mut().unwrap().show_backlinks = true;
    for _ in 0..200 {
        h.step();
        if h.query_by_label("Line 1: See [[Alpha]] for more.")
            .is_some()
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    h.get_by_label("Line 1: See [[Alpha]] for more.").click();
    let beta = id_of(&h, "Beta");
    wait_for(&mut h, "the backlink jump", move |a| {
        a.editor_file_id().as_deref() == Some(beta.as_str()) && a.editor_cursor().0 == 1
    });

    // HISTORY of Beta: its commit, and the note as it was.
    h.state_mut().layout_mut().unwrap().show_history = true;
    wait_for(&mut h, "the history", |a| !a.history().commits.is_empty());
    let commit = h.state().history().commits[0].clone();
    assert_eq!(commit.message, "Add Beta");
    h.get_by_label(&format!(
        "{} {}: {}",
        crate::views::history::format_date(&commit.date),
        commit.author,
        commit.message
    ))
    .click();
    wait_for(&mut h, "the old version", |a| a.history().has_content());
    close(h);
}

#[test]
fn tags_pane_filter_and_editor() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, project) = open_project(tmp.path());
    h.state_mut().layout_mut().unwrap().show_tags = true;
    select(&mut h, "Beta");

    // A chip toggles the tag on the selected note; the index is saved.
    h.get_by_role_and_label(Role::CheckBox, "work").click();
    let p2 = project.clone();
    wait_for(&mut h, "work on Beta", move |_| {
        file_by_name(&index(&p2), "Beta")["tags"] == json!(["work"])
    });

    // Ctrl+T adds a new tag through the pane's input.
    h.key_press_modifiers(Modifiers::COMMAND, Key::T);
    h.run_steps(3);
    h.get_by_label("New tag name").type_text("later");
    h.step();
    h.key_press(Key::Enter);
    let p3 = project.clone();
    wait_for(&mut h, "later on Beta", move |_| {
        file_by_name(&index(&p3), "Beta")["tags"] == json!(["work", "later"])
    });
    assert_eq!(index(&project)["tagPalette"], json!(["work", "later"]));

    // The tag filter: unchecking "work" hides both notes; Show all brings them back.
    h.get_by_label("Filter by tags").click();
    h.run_steps(2);
    h.get_by_role_and_label(Role::CheckBox, "Filter: work")
        .click();
    h.run_steps(2);
    assert!(h.state().hidden_tags().contains("work"));
    assert_eq!(
        h.query_all_by_role(Role::TreeItem).count(),
        1,
        "Beta still shows through `later`"
    );
    h.get_by_label("Hide all").click();
    h.run_steps(2);
    assert_eq!(h.query_all_by_role(Role::TreeItem).count(), 0);
    h.get_by_label("Show all").click();
    h.run_steps(2);
    assert_eq!(h.query_all_by_role(Role::TreeItem).count(), 2);
    h.key_press(Key::Escape);
    h.run_steps(2);

    // The tag editor paints "work" Tomato and saves it at once.
    h.get_by_label("Edit tag colors").click();
    h.run_steps(3);
    h.get_by_role_and_label(Role::ListItem, "work").click();
    h.run_steps(2);
    h.get_by_label("Tomato").click();
    let p4 = project.clone();
    wait_for(&mut h, "the tag colour", move |_| {
        index(&p4)["tagMeta"]["work"]["color"] == "Tomato"
    });
    h.get_by_label("Clear color").click();
    let p5 = project.clone();
    wait_for(&mut h, "the cleared colour", move |_| {
        index(&p5)["tagMeta"].get("work").is_none()
    });
    h.get_by_label("OK").click();
    h.run_steps(2);
    assert!(!h.state().has_overlay());
    close(h);
}

#[test]
fn attachments_insert_and_remove() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, project) = open_project(tmp.path());
    h.state_mut().layout_mut().unwrap().show_attachments = true;
    select(&mut h, "Alpha");
    h.run_steps(2);
    let item = h.get_by_role_and_label(Role::ListItem, "notes.txt");
    item.click_secondary();
    h.run_steps(2);
    h.get_by_label("Insert into Note").click();
    let p2 = project.clone();
    wait_for(&mut h, "the reference on disk", move |_| {
        std::fs::read_to_string(p2.join("alpha.md"))
            .is_ok_and(|s| s.contains("[notes.txt](./_attachments/att-"))
    });

    h.get_by_label("Remove notes.txt").click();
    h.run_steps(3);
    assert!(h.state().has_overlay());
    h.get_by_label("Remove").click();
    let p3 = project.clone();
    wait_for(&mut h, "the removal", move |_| {
        file_by_name(&index(&p3), "Alpha")["attachments"] == json!([])
    });
    let left = std::fs::read_dir(project.join("_attachments"))
        .map(|d| d.count())
        .unwrap_or(0);
    assert_eq!(left, 0, "the stored file is gone too");
    close(h);
}

#[test]
fn mcp_confirm_allow_session_deny() {
    let tmp = tempfile::tempdir().unwrap();
    let mut h = harness(Paths {
        user_data: tmp.path().join("NoteLiner"),
    });
    h.run_steps(2);
    for id in 1..=3 {
        h.state_mut().push_mcp_confirm(McpConfirm {
            id,
            tool: "add_attachment".into(),
            summary: format!("Attach file {id}"),
            args: json!({ "fileId": "a", "dataBase64": "x".repeat(200) }),
        });
    }
    h.run_steps(3);
    h.get_by_label("Attach file 1");
    h.get_by_label("Allow once").click();
    h.run_steps(3);
    // The second request: its raw arguments, with the payload cut.
    h.get_by_label("Attach file 2");
    h.get_by_label_contains("Show raw arguments").click();
    h.run_steps(2);
    assert!(h.query_by_label_contains("<200 bytes base64>").is_some());
    h.get_by_label("Allow for session").click();
    h.run_steps(3);
    h.get_by_label("Attach file 3");
    h.key_press(Key::Escape);
    h.run_steps(3);
    assert_eq!(
        h.state().mcp_decisions(),
        [(1, "allow"), (2, "session"), (3, "deny")]
    );
}

#[test]
fn pdf_export_is_a_pdf() {
    let tmp = tempfile::tempdir().unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let exporter = Arc::new(noteliner_export::LazyExporter::spawn(
        marina_egui::fonts::bundled_faces().iter().map(|(_, b)| *b),
    ));
    let raw = "---\nname: Alpha\n---\n# Heading\n\nSome **bold** text and a [[Wikilink]].\n\n- one\n- two\n\n```rust\nfn main() {}\n```\n";
    let pdf = rt
        .block_on(crate::backend::pdf_bytes(
            exporter,
            raw.into(),
            "Alpha".into(),
            tmp.path().to_path_buf(),
        ))
        .unwrap();
    assert!(pdf.starts_with(b"%PDF-"));
    let doc = hayro::hayro_syntax::Pdf::new(Arc::new(pdf)).expect("hayro reads it");
    assert_eq!(doc.pages().len(), 1);
}
