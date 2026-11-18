//! Folders, frontmatter in the editor, and changes made by other programs,
//! driven headlessly (egui_kittest) against a temp project:
//!
//! - a plain folder opens with its sub-folders, and no setup dialog;
//! - New Folder from the header, a folder's menu and the palette;
//! - a new note lands in the selected folder;
//! - renaming a folder keeps its notes' tabs open;
//! - deleting a folder asks, then sends it to the trash;
//! - a note opens with its frontmatter, and tags move both ways between the
//!   block and the Tags pane;
//! - a note changed on disk reloads, or asks when it has unsaved edits;
//! - a folder added on disk appears.

use std::path::{Path, PathBuf};

use egui::accesskit::Role;
use egui::{Key, Modifiers};
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use noteliner_core::project::Project;
use serde_json::json;

use crate::app::NoteLinerApp;
use crate::commands::Act;
use crate::config::Paths;
use crate::editor::SaveStatus;
use crate::ui_tests::{drag_row, file_by_name, harness, index, trashed, tree_rows, wait_for};

type H = Harness<'static, NoteLinerApp>;

/// Files written by hand, as another program leaves them: `Top` at the top,
/// `Work/Plan` and `Work/Meetings/Kickoff`, an empty `Archive`, and things
/// NoteLiner doesn't show.
fn plain_folder(dir: &Path) {
    let write = |path: &str, text: &str| {
        let p = dir.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    };
    write("top.md", "# Top\n\nplain text\n");
    write(
        "Work/plan.md",
        "---\ntags:\n  - work\n---\n# Plan\n\nfind the needle here\n",
    );
    write("Work/Meetings/kickoff.md", "# Kickoff\n");
    write("Work/map.mmd", "flowchart LR\n  a[\"Alpha\"]\n");
    write("Work/report.pdf", "x");
    write("_templates/t.md", "x");
    write(".git/HEAD", "ref: refs/heads/main\n");
    std::fs::create_dir_all(dir.join("Archive")).unwrap();
}

/// Alpha (tagged `work`) and Beta at the top, made by NoteLiner.
fn notes(dir: &Path) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        std::fs::create_dir_all(dir).unwrap();
        let mut p = Project::new();
        p.open(dir).await.unwrap();
        p.create_file(
            "Alpha",
            &json!(["work"]),
            Some("# Alpha\n\none two three\n".into()),
            None,
        )
        .await
        .unwrap();
        p.create_file("Beta", &json!([]), Some("beta\n".into()), None)
            .await
            .unwrap();
    });
}

/// One note with more tags than the New Item dialog shows at once, out of
/// order.
fn many_tags(dir: &Path) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        std::fs::create_dir_all(dir).unwrap();
        let mut p = Project::new();
        p.open(dir).await.unwrap();
        let tags = json!(["zeta", "Mid", "alpha", "b1", "b2", "b3", "b4", "b5", "b6"]);
        p.create_file("Alpha", &tags, Some("alpha\n".into()), None)
            .await
            .unwrap();
    });
}

fn open(tmp: &Path, make: fn(&Path), count: usize) -> (H, PathBuf, Paths) {
    let project = tmp.join("notes");
    std::fs::create_dir_all(&project).unwrap();
    make(&project);
    let paths = Paths {
        user_data: tmp.join("NoteLiner"),
    };
    let mut h = harness(paths.clone());
    h.run_steps(2);
    h.state_mut().open(project.clone());
    wait_for(&mut h, "the project", move |a| a.entries().len() == count);
    (h, project, paths)
}

fn id_of(h: &H, name: &str) -> String {
    h.state()
        .entries()
        .iter()
        .find(|e| e.name == name)
        .map(|e| e.id.clone())
        .unwrap_or_else(|| panic!("no note named {name}"))
}

fn select(h: &mut H, name: &str) {
    h.get_by_role_and_label(Role::TreeItem, name).click();
    let id = id_of(h, name);
    wait_for(h, name, move |a| {
        a.editor_file_id().as_deref() == Some(id.as_str())
    });
}

fn folder(h: &mut H, name: &str) {
    h.get_by_role_and_label(Role::TreeItem, &format!("Folder {name}"))
        .click();
    h.run_steps(2);
}

/// Opens the folder at `path` if it is closed.
fn ensure_open(h: &mut H, path: &str) {
    if !h.state().open_folders().contains(&path.to_string()) {
        let name = path.rsplit('/').next().unwrap().to_string();
        folder(h, &name);
    }
}

fn act(h: &mut H, act: Act) {
    let ctx = h.ctx.clone();
    h.state_mut().apply_for_test(&ctx, act);
    h.run_steps(3);
}

/// Types a name into the dialog that is open and confirms it.
fn name_and_ok(h: &mut H, name: &str) {
    h.run_steps(3);
    h.get_all_by_role(Role::TextInput)
        .next()
        .expect("the name field")
        .focus();
    h.step();
    h.get_all_by_role(Role::TextInput)
        .next()
        .expect("the name field")
        .type_text(name);
    h.step();
    h.get_by_label("OK").click();
}

fn close(mut h: H) {
    h.state_mut().backend().close_project(Vec::new());
    wait_for(&mut h, "the close", |a| a.project_index().is_none());
}

fn read(project: &Path, path: &str) -> String {
    std::fs::read_to_string(project.join(path)).unwrap()
}

/// Sets a file's time a little ahead, as a write by another program does on
/// a file system that keeps times to the second.
fn touch(path: &Path) {
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
    let f = std::fs::File::options().write(true).open(path).unwrap();
    f.set_modified(later).unwrap();
}

fn touch_dir(path: &Path) {
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
    let mut options = std::fs::File::options();
    options.read(true);
    // Windows opens a folder only with backup semantics, and sets its time
    // only through a handle that may write attributes.
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_WRITE_ATTRIBUTES: u32 = 0x0100;
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        options
            .access_mode(FILE_WRITE_ATTRIBUTES)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS);
    }
    options.open(path).unwrap().set_modified(later).unwrap();
}

#[test]
fn a_plain_folder_opens_with_its_sub_folders() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, project, _) = open(tmp.path(), plain_folder, 4);
    // No setup dialog, and nothing asked.
    assert!(!h.state().has_overlay());
    assert_eq!(h.state().folders(), ["Archive", "Work", "Work/Meetings"]);
    // Folders first and closed; the empty one shows too.
    assert_eq!(tree_rows(&h), ["Folder Archive", "Folder Work", "top"]);
    // Opening changed no note, and left `.git` alone.
    assert_eq!(read(&project, "top.md"), "# Top\n\nplain text\n");
    assert_eq!(read(&project, ".git/HEAD"), "ref: refs/heads/main\n");
    let i = index(&project);
    assert_eq!(file_by_name(&i, "plan")["filename"], "Work/plan.md");
    assert_eq!(file_by_name(&i, "plan")["tags"], json!(["work"]));

    // A folder opens and closes with a click, and is the selection.
    folder(&mut h, "Work");
    assert_eq!(h.state().selected_folder().as_deref(), Some("Work"));
    assert_eq!(
        tree_rows(&h),
        [
            "Folder Archive",
            "Folder Work",
            "Folder Meetings",
            "map",
            "plan",
            "top"
        ]
    );
    folder(&mut h, "Meetings");
    assert_eq!(tree_rows(&h)[3], "kickoff");
    folder(&mut h, "Work");
    assert_eq!(tree_rows(&h), ["Folder Archive", "Folder Work", "top"]);
    folder(&mut h, "Work");

    // The sort list has no User Order.
    h.get_by_label("Sort files").click();
    h.run_steps(2);
    assert!(h.query_by_label("User Order").is_none());
    h.get_by_label("Name Z-A").click();
    h.run_steps(2);
    // Folders stay first, in name order; the notes are reversed.
    assert_eq!(
        tree_rows(&h),
        [
            "Folder Archive",
            "Folder Work",
            "Folder Meetings",
            "kickoff",
            "plan",
            "map",
            "top"
        ]
    );

    // Which folders are open is remembered with the project.
    assert_eq!(h.state().open_folders(), ["Work", "Work/Meetings"]);
    act(&mut h, Act::GoHome);
    wait_for(&mut h, "the close", |a| a.project_index().is_none());
    h.state_mut().open(project.clone());
    wait_for(&mut h, "the project again", |a| a.entries().len() == 4);
    assert_eq!(h.state().open_folders(), ["Work", "Work/Meetings"]);
    close(h);
}

#[test]
fn new_folder_from_the_header_the_menu_and_the_palette() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, project, _) = open(tmp.path(), plain_folder, 4);

    // The header's button, with nothing selected: at the top.
    h.get_by_label("New Folder").click();
    name_and_ok(&mut h, "Inbox");
    wait_for(&mut h, "Inbox", |a| {
        a.folders().contains(&"Inbox".to_string())
    });
    assert!(project.join("Inbox").is_dir());
    assert_eq!(h.state().selected_folder().as_deref(), Some("Inbox"));

    // A folder's menu: inside that folder, which opens to show it.
    h.get_by_role_and_label(Role::TreeItem, "Folder Work")
        .click_secondary();
    h.run_steps(3);
    h.get_by_label_contains("New Folder...").click();
    h.run_steps(3);
    assert!(h.state().has_overlay(), "the New Folder dialog");
    name_and_ok(&mut h, "Drafts");
    wait_for(&mut h, "Work/Drafts", |a| {
        a.folders().contains(&"Work/Drafts".to_string())
    });
    assert!(project.join("Work/Drafts").is_dir());
    assert!(h.state().open_folders().contains(&"Work".to_string()));

    // The palette's command: in the selected folder.
    act(&mut h, Act::NewFolder);
    name_and_ok(&mut h, "2026");
    wait_for(&mut h, "Work/Drafts/2026", |a| {
        a.folders().contains(&"Work/Drafts/2026".to_string())
    });
    assert!(project.join("Work/Drafts/2026").is_dir());

    // The name rules: a name NoteLiner skips, and one already there.
    act(&mut h, Act::NewFolder);
    h.run_steps(3);
    h.get_all_by_role(Role::TextInput).next().unwrap().focus();
    h.step();
    h.get_all_by_role(Role::TextInput)
        .next()
        .unwrap()
        .type_text("_hidden");
    h.run_steps(2);
    assert!(h.query_by_label_contains("can't start with").is_some());
    assert!(h.get_by_label("OK").accesskit_node().is_disabled());
    h.get_by_label("Cancel").click();
    h.run_steps(2);

    // A new note lands in the selected folder.
    h.get_by_role_and_label(Role::TreeItem, "Folder Inbox")
        .click();
    h.run_steps(2);
    act(&mut h, Act::NewFile);
    name_and_ok(&mut h, "Shopping List");
    let p2 = project.clone();
    wait_for(&mut h, "the note in Inbox", move |a| {
        p2.join("Inbox/shopping-list.md").exists()
            && a.entries()
                .iter()
                .any(|e| e.filename == "Inbox/shopping-list.md")
            && a.editor_file_id() == a.selected()
            && a.selected().is_some()
    });
    // And a diagram, beside the note that is now selected.
    act(&mut h, Act::NewDiagram);
    name_and_ok(&mut h, "Flow");
    let p3 = project.clone();
    wait_for(&mut h, "the diagram in Inbox", move |_| {
        p3.join("Inbox/flow.mmd").exists()
    });
    // No Remote Sync or Toggle History in the palette.
    let commands: Vec<String> = crate::commands::registry()
        .commands()
        .iter()
        .map(|c| c.label.clone())
        .collect();
    assert!(commands.iter().any(|c| c == "New Folder..."));
    for gone in ["Remote Sync", "Toggle History", "Project Settings"] {
        assert!(!commands.iter().any(|c| c == gone), "{gone}");
    }
    close(h);
}

#[test]
fn folders_rename_move_and_delete_with_their_notes() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, project, paths) = open(tmp.path(), plain_folder, 4);
    folder(&mut h, "Work");
    select(&mut h, "plan");
    let plan = id_of(&h, "plan");
    folder(&mut h, "Meetings");
    select(&mut h, "kickoff");
    assert_eq!(h.state().tab_ids().len(), 2);

    // Rename Work in place: its notes' tabs stay open on their files.
    folder(&mut h, "Work");
    h.key_press(Key::F2);
    h.run_steps(3);
    h.get_by_label("Rename").focus();
    h.step();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.step();
    h.get_by_label("Rename").type_text("Projects");
    h.step();
    h.key_press(Key::Enter);
    let p2 = project.clone();
    wait_for(&mut h, "the rename", move |a| {
        p2.join("Projects/plan.md").exists()
            && a.folders() == ["Archive", "Projects", "Projects/Meetings"]
    });
    assert!(!project.join("Work").exists());
    assert_eq!(h.state().tab_ids().len(), 2);
    assert_eq!(h.state().selected_folder().as_deref(), Some("Projects"));
    // The folder that was open is still open, under its new name.
    assert!(h
        .state()
        .open_folders()
        .contains(&"Projects/Meetings".to_string()));
    let i = index(&project);
    assert_eq!(file_by_name(&i, "plan")["id"], plan.as_str());
    assert_eq!(file_by_name(&i, "plan")["filename"], "Projects/plan.md");
    assert_eq!(file_by_name(&i, "plan")["tags"], json!(["work"]));

    // An edit in the open note goes to the file at its new path.
    ensure_open(&mut h, "Projects");
    select(&mut h, "plan");
    h.get_by_role(Role::MultilineTextInput).focus();
    h.step();
    h.get_by_role(Role::MultilineTextInput).type_text("EDITED ");
    h.step();
    let p3 = project.clone();
    wait_for(&mut h, "the save at the new path", move |_| {
        std::fs::read_to_string(p3.join("Projects/plan.md")).is_ok_and(|s| s.contains("EDITED"))
    });
    assert!(!project.join("Work").exists());

    // Drag Meetings onto Archive: the folder moves with its note.
    drag_row(&mut h, "Folder Meetings", "Folder Archive");
    let p4 = project.clone();
    wait_for(&mut h, "the folder move", move |a| {
        p4.join("Archive/Meetings/kickoff.md").exists()
            && a.folders().contains(&"Archive/Meetings".to_string())
    });
    assert!(!project.join("Projects/Meetings").exists());
    assert_eq!(h.state().tab_ids().len(), 2);

    // Move… from the menu: Archive/Meetings to the top.
    act(&mut h, Act::CloseTab);
    if !h.state().open_folders().contains(&"Archive".to_string()) {
        folder(&mut h, "Archive");
    }
    h.get_by_role_and_label(Role::TreeItem, "Folder Meetings")
        .click_secondary();
    h.run_steps(3);
    h.get_by_label_contains("Move...").click();
    h.run_steps(3);
    // The folder itself isn't offered; the top of the project is.
    h.get_by_role_and_label(Role::ListBoxOption, "(Top of the project)")
        .click();
    h.run_steps(2);
    h.get_by_label("OK").click();
    let p5 = project.clone();
    wait_for(&mut h, "the move to the top", move |a| {
        p5.join("Meetings/kickoff.md").exists() && a.folders().contains(&"Meetings".to_string())
    });

    // Delete Projects: it asks, with what is inside, then trashes it.
    folder(&mut h, "Projects");
    assert_eq!(h.state().selected_folder().as_deref(), Some("Projects"));
    h.key_press_modifiers(Modifiers::COMMAND, Key::D);
    h.run_steps(3);
    assert!(h.state().has_overlay());
    assert!(h
        .query_by_label_contains("1 note, 1 diagram and 1 other file")
        .is_some());
    assert!(
        project.join("Projects/plan.md").exists(),
        "not before the answer"
    );
    h.get_by_label("Delete").click();
    let p6 = project.clone();
    wait_for(&mut h, "the folder delete", move |a| {
        !p6.join("Projects").exists() && !a.folders().contains(&"Projects".to_string())
    });
    assert_eq!(trashed(&paths), ["Projects"]);
    assert!(paths.user_data.join("trash/Projects/plan.md").exists());
    // Its notes left the index, and their tab closed.
    let names: Vec<String> = h.state().entries().iter().map(|e| e.name.clone()).collect();
    assert!(!names.contains(&"plan".to_string()) && !names.contains(&"map".to_string()));
    assert!(!h.state().tab_ids().contains(&plan));
    assert_eq!(index(&project)["files"].as_array().unwrap().len(), 2);
    close(h);
}

#[test]
fn a_note_opens_with_its_frontmatter() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, project, _) = open(tmp.path(), notes, 2);
    h.state_mut().layout_mut().unwrap().show_tags = true;
    h.state_mut().layout_mut().unwrap().show_preview = true;
    select(&mut h, "Alpha");
    let raw = read(&project, "alpha.md");
    assert!(raw.starts_with("---\ntags:\n  - work\ncreated: '"), "{raw}");

    // The editor shows the file as it is on disk; the rest read the text.
    assert_eq!(h.state().editor_file_text(), raw);
    assert_eq!(h.state().editor_text(), "# Alpha\n\none two three\n");
    // The caret starts on the text, below the block.
    assert_eq!(h.state().editor_body_line(), 1);
    assert_eq!(h.state().editor_cursor().0, 6);
    // The word count leaves the block out: "# Alpha one two three".
    h.run_steps(2);
    assert!(h.query_by_label_contains("created").is_none());

    // Adding a tag in the Tags pane rewrites the line in the editor, and
    // the note isn't marked as edited.
    h.key_press_modifiers(Modifiers::COMMAND, Key::T);
    h.run_steps(3);
    h.get_by_label("New tag name").type_text("later");
    h.step();
    h.key_press(Key::Enter);
    let p2 = project.clone();
    wait_for(&mut h, "later on Alpha", move |a| {
        file_by_name(&index(&p2), "Alpha")["tags"] == json!(["work", "later"])
            && a.editor_file_text().contains("  - later\n")
    });
    assert_eq!(h.state().editor_status(), SaveStatus::Saved);
    assert_eq!(h.state().editor_text(), "# Alpha\n\none two three\n");
    // The file on disk and the editor agree, with no save of the note.
    let p3 = project.clone();
    wait_for(&mut h, "the block on disk", move |a| {
        std::fs::read_to_string(p3.join("alpha.md")).is_ok_and(|s| s == a.editor_file_text())
    });
    assert_eq!(
        h.state().editor_cursor().0,
        7,
        "the caret stayed on its line"
    );

    // A tag typed in the block shows in the Tags pane after the save.
    let typed = h
        .state()
        .editor_file_text()
        .replace("  - later\n", "  - later\n  - typed\n");
    h.state_mut().edit_file_for_test(&typed);
    let p4 = project.clone();
    wait_for(&mut h, "the typed tag", move |a| {
        file_by_name(&index(&p4), "Alpha")["tags"] == json!(["work", "later", "typed"])
            && a.entries()
                .iter()
                .any(|e| e.name == "Alpha" && e.tags.contains(&"typed".to_string()))
    });
    // The save wrote the block as typed.
    assert_eq!(read(&project, "alpha.md"), typed);
    assert!(h.query_by_role_and_label(Role::CheckBox, "typed").is_some());

    // A half-typed block is saved as typed; the tags keep their last good
    // values, and the status bar says so.
    let broken = typed.replace("  - typed\n", "  - typed\n  - [\n");
    h.state_mut().edit_file_for_test(&broken);
    let p5 = project.clone();
    let broken2 = broken.clone();
    wait_for(&mut h, "the broken block on disk", move |_| {
        std::fs::read_to_string(p5.join("alpha.md")).is_ok_and(|s| s == broken2)
    });
    h.run_steps(2);
    assert_eq!(
        file_by_name(&index(&project), "Alpha")["tags"],
        json!(["work", "later", "typed"])
    );
    assert_eq!(h.state().editor_text(), "# Alpha\n\none two three\n");

    // A deleted block comes back on the next save, with the note's tags.
    h.state_mut()
        .edit_file_for_test("# Alpha\n\nno block now\n");
    let p6 = project.clone();
    wait_for(&mut h, "the block back", move |a| {
        std::fs::read_to_string(p6.join("alpha.md")).is_ok_and(|s| {
            s.starts_with("---\ntags:\n  - work\n  - later\n  - typed\ncreated: '")
                && s.ends_with("---\n# Alpha\n\nno block now\n")
                && s == a.editor_file_text()
        })
    });
    assert_eq!(h.state().editor_text(), "# Alpha\n\nno block now\n");
    close(h);
}

#[test]
fn a_search_result_opens_on_the_right_line() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, _, _) = open(tmp.path(), plain_folder, 4);
    h.key_press_modifiers(Modifiers::COMMAND, Key::F);
    h.run_steps(3);
    h.get_by_label("Search all files").type_text("needle");
    for _ in 0..200 {
        h.step();
        if h.query_by_label("Line 3: find the needle here").is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    // The result's line is its line in the note's text.
    h.get_by_label("Line 3: find the needle here").click();
    let plan = id_of(&h, "plan");
    wait_for(&mut h, "the jump", move |a| {
        a.editor_file_id().as_deref() == Some(plan.as_str()) && a.editor_body_line() == 3
    });
    // In the file that is line 7, after the four lines of frontmatter.
    assert_eq!(h.state().editor_cursor().0, 7);
    close(h);
}

#[test]
fn changes_made_by_other_programs() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, project, _) = open(tmp.path(), notes, 2);
    select(&mut h, "Alpha");
    let alpha = id_of(&h, "Alpha");
    let block = h
        .state()
        .editor_file_text()
        .replace("# Alpha\n\none two three\n", "");

    // A folder and a note added on disk appear.
    std::fs::create_dir_all(project.join("Inbox")).unwrap();
    std::fs::write(project.join("Inbox/from-phone.md"), "hello\n").unwrap();
    touch_dir(&project);
    h.state_mut().check_now();
    wait_for(&mut h, "the new folder", |a| {
        a.folders() == ["Inbox"] && a.entries().iter().any(|e| e.name == "from-phone")
    });
    assert!(tree_rows(&h).contains(&"Folder Inbox".to_string()));

    // The open note changed on disk, with no unsaved edits: it reloads.
    let outside = format!("{block}# Alpha\n\nchanged outside\n");
    std::fs::write(project.join("alpha.md"), &outside).unwrap();
    touch(&project.join("alpha.md"));
    h.state_mut().check_now();
    let outside2 = outside.clone();
    wait_for(&mut h, "the reload", move |a| {
        a.editor_file_text() == outside2
    });
    assert!(!h.state().has_overlay());
    assert_eq!(h.state().editor_status(), SaveStatus::Saved);

    // Changed on disk under unsaved edits: the edits stay, and it asks.
    let mine = format!("{block}# Alpha\n\nmy unsaved edit\n");
    h.state_mut().edit_file_for_test(&mine);
    let theirs = format!("{block}# Alpha\n\ntheirs\n");
    std::fs::write(project.join("alpha.md"), &theirs).unwrap();
    touch(&project.join("alpha.md"));
    h.state_mut().check_now();
    wait_for(&mut h, "the question", |a| a.has_overlay());
    h.run_steps(3);
    assert!(h
        .query_by_label_contains("was changed by another program")
        .is_some());
    // Nothing is written while it waits for an answer.
    for _ in 0..40 {
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(read(&project, "alpha.md"), theirs);
    assert_eq!(h.state().editor_file_text(), mine);
    // Keep Mine saves the edits over the file.
    h.get_by_label("Keep Mine").click();
    let (p2, mine2) = (project.clone(), mine.clone());
    wait_for(&mut h, "my edits on disk", move |_| {
        std::fs::read_to_string(p2.join("alpha.md")).is_ok_and(|s| s == mine2)
    });
    assert!(!h.state().has_overlay());

    // Again, and Reload takes the file from disk.
    wait_for(&mut h, "the save to settle", |a| {
        a.editor_status() == SaveStatus::Saved
    });
    h.state_mut()
        .edit_file_for_test(&format!("{block}# Alpha\n\nto be dropped\n"));
    std::fs::write(project.join("alpha.md"), &theirs).unwrap();
    touch(&project.join("alpha.md"));
    h.state_mut().check_now();
    wait_for(&mut h, "the question again", |a| a.has_overlay());
    h.run_steps(3);
    h.get_by_label("Reload").click();
    h.run_steps(3);
    assert_eq!(h.state().editor_file_text(), theirs);
    // If the autosave was already writing when the change was found, Reload
    // writes the file back.
    wait_for(&mut h, "the reload to settle", |a| {
        a.editor_status() == SaveStatus::Saved
    });
    assert_eq!(read(&project, "alpha.md"), theirs);

    // A note moved by another program keeps its tab, name and tags.
    std::fs::rename(project.join("alpha.md"), project.join("Inbox/alpha.md")).unwrap();
    touch_dir(&project);
    touch_dir(&project.join("Inbox"));
    h.state_mut().check_now();
    wait_for(&mut h, "the move", |a| {
        a.entries().iter().any(|e| e.filename == "Inbox/alpha.md")
    });
    assert_eq!(h.state().selected().as_deref(), Some(alpha.as_str()));
    let i = index(&project);
    assert_eq!(file_by_name(&i, "Alpha")["id"], alpha.as_str());
    assert_eq!(file_by_name(&i, "Alpha")["tags"], json!(["work"]));

    // A note removed by another program leaves the tree, and its tab closes.
    std::fs::remove_file(project.join("Inbox/alpha.md")).unwrap();
    touch_dir(&project.join("Inbox"));
    h.state_mut().check_now();
    wait_for(&mut h, "the removal", |a| {
        !a.entries().iter().any(|e| e.name == "Alpha")
    });
    assert!(!h.state().tab_ids().contains(&alpha));

    // One removed while it has unsaved edits is written again.
    select(&mut h, "Beta");
    let beta = h.state().editor_file_text();
    h.state_mut()
        .edit_file_for_test(&format!("{beta}not lost\n"));
    std::fs::remove_file(project.join("beta.md")).unwrap();
    touch_dir(&project);
    h.state_mut().check_now();
    let p3 = project.clone();
    wait_for(&mut h, "the edits written again", move |a| {
        std::fs::read_to_string(p3.join("beta.md")).is_ok_and(|s| s.ends_with("not lost\n"))
            && a.entries().iter().any(|e| e.name == "Beta")
    });
    assert_eq!(h.state().tab_ids().len(), 1);
    close(h);
}

/// Picks `name` in the New Item dialog's Folder list.
fn pick_parent(h: &mut H, name: &str) {
    h.get_by_role_and_label(Role::ComboBox, "Folder").click();
    h.run_steps(3);
    h.get_all_by_role(Role::ListBoxOption)
        .find(|n| n.accesskit_node().label().is_some_and(|l| l.trim() == name))
        .expect("the folder in the list")
        .click();
    h.run_steps(3);
}

#[test]
fn the_four_types_are_on_one_line() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, _, _) = open(tmp.path(), plain_folder, 4);
    act(&mut h, Act::NewFile);
    h.run_steps(3);
    let top = |label: &str| h.get_by_role_and_label(Role::Button, label).rect().top();
    let note = top("Note");
    for label in ["Presentation", "Diagram", "Folder"] {
        assert_eq!(top(label), note, "{label} is beside Note");
    }
    close(h);
}

#[test]
fn the_tags_are_a_sorted_list_of_checkboxes() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, _, _) = open(tmp.path(), many_tags, 1);
    act(&mut h, Act::NewFile);
    h.run_steps(3);
    let mut boxes: Vec<(String, f32, f32)> = h
        .get_all_by_role(Role::CheckBox)
        .map(|n| {
            let label = n.accesskit_node().label().unwrap_or_default();
            (label.trim().to_string(), n.rect().left(), n.rect().top())
        })
        .collect();
    // The dialog is drawn last, over a pane that lists the tags too.
    let boxes = boxes.split_off(boxes.len() - 9);
    let names: Vec<&str> = boxes.iter().map(|b| b.0.as_str()).collect();
    let mut sorted = names.clone();
    sorted.sort_by_key(|t| t.to_lowercase());
    assert_eq!(names, sorted, "alphabetical, whatever the case");
    assert!(names.contains(&"zeta") && names.contains(&"Mid"));
    // One to a row: the same left edge, each below the one before.
    for pair in boxes.windows(2) {
        assert_eq!(pair[0].1, pair[1].1, "{} and {}", pair[0].0, pair[1].0);
        assert!(pair[0].2 < pair[1].2, "{} above {}", pair[0].0, pair[1].0);
    }
    // The list scrolls: its last tag lies past the buttons under it.
    let ok = h.get_by_label("OK").rect();
    assert!(boxes.last().unwrap().2 > ok.top(), "the list is cut short");
    close(h);
}

/// Makes a presentation called `name` through New Item, in slide theme
/// `theme` when one is given, and returns the theme in its file.
fn new_presentation(h: &mut H, project: &Path, name: &str, theme: Option<&str>) -> String {
    act(h, Act::NewFile);
    h.run_steps(3);
    // A note has no theme.
    assert!(h.query_by_role_and_label(Role::ComboBox, "Theme").is_none());
    h.get_by_role_and_label(Role::Button, "Presentation")
        .click();
    h.run_steps(3);
    if let Some(theme) = theme {
        h.get_by_role_and_label(Role::ComboBox, "Theme").click();
        h.run_steps(3);
        h.get_all_by_role(Role::ListBoxOption)
            .find(|n| {
                n.accesskit_node()
                    .label()
                    .is_some_and(|l| l.trim() == theme)
            })
            .expect("the theme in the list")
            .click();
        h.run_steps(3);
    }
    name_and_ok(h, name);
    let want = name.to_string();
    wait_for(h, "the presentation", move |a| {
        a.entries().iter().any(|e| e.name == want && e.deck)
    });
    let file = format!("{}.md", name.to_lowercase());
    let raw = read(project, &file);
    let line = raw
        .lines()
        .find_map(|l| l.trim().strip_prefix("theme: "))
        .unwrap_or_else(|| panic!("no theme in {raw}"));
    line.to_string()
}

#[test]
fn new_presentation_takes_the_theme_you_pick() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, project, _) = open(tmp.path(), plain_folder, 4);
    // Left alone, the theme is the app theme in use.
    let app_theme = h.state().theme_id().to_string();
    assert_eq!(new_presentation(&mut h, &project, "First", None), app_theme);
    assert_eq!(
        new_presentation(&mut h, &project, "Second", Some("Light-Mulberry")),
        "lightMulberry"
    );
    close(h);
}

#[test]
fn new_item_makes_a_folder_in_the_parent_you_pick() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, project, _) = open(tmp.path(), plain_folder, 4);

    // Ctrl+N's dialog, with Note chosen: a name typed for it is kept when
    // the type changes to Folder.
    act(&mut h, Act::NewFile);
    h.run_steps(3);
    assert!(h.state().has_overlay(), "the New Item dialog");
    h.get_all_by_role(Role::TextInput).next().unwrap().focus();
    h.step();
    h.get_all_by_role(Role::TextInput)
        .next()
        .unwrap()
        .type_text("Work");
    h.run_steps(2);
    assert!(!h.get_by_label("OK").accesskit_node().is_disabled());
    h.get_by_role_and_label(Role::Button, "Folder").click();
    h.run_steps(3);

    // At the top there is a Work already, so OK is off and says why.
    assert!(h.query_by_label_contains("already there").is_some());
    assert!(h.get_by_label("OK").accesskit_node().is_disabled());
    // A folder has no tags.
    assert!(h.query_by_label("Tags").is_none());

    // Inside Work the name is free: the check follows the parent.
    pick_parent(&mut h, "Work");
    assert!(h.query_by_label_contains("already there").is_none());
    h.get_by_label("OK").click();
    wait_for(&mut h, "Work/Work", |a| {
        a.folders().contains(&"Work/Work".to_string())
    });
    assert!(project.join("Work/Work").is_dir());
    assert_eq!(h.state().selected_folder().as_deref(), Some("Work/Work"));
    // No note was made of it.
    assert!(!h.state().entries().iter().any(|e| e.name == "Work"));
}

/// Forty folders, so the New Item dialog's Folder list is taller than the
/// window.
fn many_folders(dir: &Path) {
    for i in 1..=40 {
        std::fs::create_dir_all(dir.join(format!("Folder {i:02}"))).unwrap();
    }
}

#[test]
fn a_long_folder_list_scrolls_in_the_new_file_dialog() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, project, _) = open(tmp.path(), many_folders, 0);
    wait_for(&mut h, "the folders", |a| a.folders().len() == 40);
    act(&mut h, Act::NewFile);
    h.get_by_role_and_label(Role::ComboBox, "Folder").click();
    h.run_steps(3);

    // The list is no taller than ten rows, and stays on the screen.
    let screen = h.ctx.content_rect();
    let first = h
        .get_by_role_and_label(Role::ListBoxOption, "(Top of the project)")
        .rect();
    assert!(screen.contains_rect(first), "{first:?}");
    let last = |h: &H| {
        h.get_all_by_role(Role::ListBoxOption)
            .find(|n| {
                n.accesskit_node()
                    .label()
                    .is_some_and(|l| l.trim() == "Folder 40")
            })
            .expect("the last folder")
            .rect()
    };
    assert!(
        last(&h).top() > first.top() + 280.0,
        "the last row is out of view"
    );

    // The wheel brings the last folder into view, and the list stays open.
    h.hover_at(first.center());
    for _ in 0..12 {
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -200.0),
            source: egui::MouseWheelSource::Unknown,
            phase: egui::TouchPhase::Move,
            modifiers: Modifiers::NONE,
        });
        h.run_steps(2);
    }
    let rect = last(&h);
    assert!(
        rect.bottom() <= first.top() + 281.0,
        "{rect:?} from {first:?}"
    );
    h.get_all_by_role(Role::ListBoxOption)
        .find(|n| {
            n.accesskit_node()
                .label()
                .is_some_and(|l| l.trim() == "Folder 40")
        })
        .unwrap()
        .click();
    h.run_steps(3);
    // Picking a row closes the list.
    assert!(h.query_by_role(Role::ListBoxOption).is_none());

    name_and_ok(&mut h, "Deep");
    let p = project.clone();
    wait_for(&mut h, "the note in the last folder", move |_| {
        p.join("Folder 40/deep.md").exists()
    });
    close(h);
}

/// A save looks at the file first: a change another program made is not
/// written over, even when the save runs before the app has looked.
#[test]
fn a_save_does_not_write_over_another_programs_change() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, project, _) = open(tmp.path(), notes, 2);
    select(&mut h, "Alpha");
    let block = h
        .state()
        .editor_file_text()
        .replace("# Alpha\n\none two three\n", "");
    let text = |body: &str| format!("{block}# Alpha\n\n{body}\n");
    let on_disk = |project: &Path, want: String| {
        let project = project.to_path_buf();
        move |_: &mut NoteLinerApp| {
            std::fs::read_to_string(project.join("alpha.md")).is_ok_and(|s| s == want)
        }
    };
    let settled = |a: &mut NoteLinerApp| a.editor_status() == SaveStatus::Saved;

    // The file was only touched: the save goes ahead, and nothing is asked.
    touch(&project.join("alpha.md"));
    h.state_mut().edit_file_for_test(&text("mine"));
    wait_for(
        &mut h,
        "the save over a touched file",
        on_disk(&project, text("mine")),
    );
    assert!(!h.state().has_overlay());

    // The file changed, and the autosave runs with no check before it.
    wait_for(&mut h, "the save to settle", settled);
    std::fs::write(project.join("alpha.md"), text("theirs")).unwrap();
    touch(&project.join("alpha.md"));
    h.state_mut().edit_file_for_test(&text("mine again"));
    wait_for(&mut h, "the question", |a| a.has_overlay());
    h.run_steps(3);
    assert_eq!(read(&project, "alpha.md"), text("theirs"));
    assert_eq!(h.state().editor_file_text(), text("mine again"));
    assert_eq!(h.state().editor_status(), SaveStatus::Unsaved);
    h.get_by_label("Keep Mine").click();
    wait_for(
        &mut h,
        "my edits on disk",
        on_disk(&project, text("mine again")),
    );

    // The same as the note's tab is left: it asks when the tab is shown.
    select(&mut h, "Beta");
    select(&mut h, "Alpha");
    wait_for(&mut h, "the save to settle", settled);
    std::fs::write(project.join("alpha.md"), text("theirs")).unwrap();
    touch(&project.join("alpha.md"));
    h.state_mut().edit_file_for_test(&text("left behind"));
    let ctx = h.ctx.clone();
    h.state_mut().apply_for_test(&ctx, Act::NextTab);
    let beta = id_of(&h, "Beta");
    wait_for(&mut h, "Beta", move |a| {
        a.editor_file_id().as_deref() == Some(beta.as_str())
    });
    for _ in 0..40 {
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(!h.state().has_overlay());
    assert_eq!(read(&project, "alpha.md"), text("theirs"));
    select(&mut h, "Alpha");
    wait_for(&mut h, "the question on return", |a| a.has_overlay());
    h.run_steps(3);
    assert_eq!(h.state().editor_file_text(), text("left behind"));
    h.get_by_label("Reload").click();
    h.run_steps(3);
    assert_eq!(h.state().editor_file_text(), text("theirs"));
    assert_eq!(read(&project, "alpha.md"), text("theirs"));

    // The same as the tab is closed: there is nowhere to ask, so the edits
    // are written and the log says so.
    wait_for(&mut h, "the reload to settle", settled);
    std::fs::write(project.join("alpha.md"), text("theirs again")).unwrap();
    touch(&project.join("alpha.md"));
    h.state_mut().edit_file_for_test(&text("closed over"));
    h.state_mut().apply_for_test(&ctx, Act::CloseTab);
    wait_for(
        &mut h,
        "the closed tab's edits",
        on_disk(&project, text("closed over")),
    );
    assert!(h
        .state()
        .log_texts()
        .iter()
        .any(|l| l.contains("Your version was kept")));
}

/// A note with no unsaved edits that changes on disk reloads, and says so.
#[test]
fn a_reload_from_disk_shows_a_toast() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, project, _) = open(tmp.path(), notes, 2);
    select(&mut h, "Alpha");
    std::fs::write(project.join("alpha.md"), "changed outside\n").unwrap();
    touch(&project.join("alpha.md"));
    h.state_mut().check_now();
    wait_for(&mut h, "the reload", |a| {
        a.editor_file_text() == "changed outside\n"
    });
    assert_eq!(
        h.state().toast_texts(),
        ["“Alpha” changed on disk and was reloaded."]
    );
}

/// The whole of Alpha's file with `body` as its text, under the block the
/// editor shows now.
fn alpha_with(h: &H, body: &str) -> String {
    let file = h.state().editor_file_text();
    let block = &file[..noteliner_core::frontmatter::body_start(&file)];
    format!("{block}# Alpha\n\n{body}\n")
}

/// Another program writes `text` over a file. A check that the app has
/// already started is left to finish first, so that it does not find the
/// change before the step under test does.
fn write_outside(h: &mut H, path: &Path, text: &str) {
    for _ in 0..5 {
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    std::fs::write(path, text).unwrap();
    touch(path);
}

/// The copies of your edits in the project folder.
fn copies(project: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(project)
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.contains("(your edits "))
        .collect();
    names.sort();
    names
}

/// A tag added from the Tags pane goes into the file as it is on disk. If
/// another program changed the note's text first, the editor takes it: a
/// later edit does not write the old text back.
#[test]
fn a_tag_change_shows_another_programs_change() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, project, _) = open(tmp.path(), notes, 2);
    let alpha = project.join("alpha.md");
    select(&mut h, "Alpha");

    // No unsaved edits: the note reloads with their text and your tag.
    let theirs = alpha_with(&h, "theirs");
    write_outside(&mut h, &alpha, &theirs);
    h.state_mut().add_tag_for_test("later");
    wait_for(&mut h, "their text in the editor", |a| {
        let text = a.editor_file_text();
        text.contains("theirs") && text.contains("  - later\n")
    });
    assert_eq!(h.state().editor_file_text(), read(&project, "alpha.md"));
    assert_eq!(
        h.state().toast_texts(),
        ["“Alpha” changed on disk and was reloaded."]
    );
    let next = alpha_with(&h, "theirs\n\nand more");
    h.state_mut().edit_file_for_test(&next);
    let want = next.clone();
    let file = alpha.clone();
    wait_for(&mut h, "the later edit on disk", move |_| {
        std::fs::read_to_string(&file).is_ok_and(|s| s == want)
    });
    wait_for(&mut h, "the save to settle", |a| {
        a.editor_status() == SaveStatus::Saved
    });

    // Unsaved edits: the tag is written, and the dialog asks about the rest.
    let theirs = alpha_with(&h, "theirs again");
    write_outside(&mut h, &alpha, &theirs);
    let mine = alpha_with(&h, "mine");
    h.state_mut().edit_file_for_test(&mine);
    h.state_mut().add_tag_for_test("third");
    wait_for(&mut h, "the question", |a| a.has_overlay());
    h.run_steps(3);
    let disk = read(&project, "alpha.md");
    assert!(disk.contains("theirs again") && disk.contains("  - third\n"));
    assert!(h.state().editor_file_text().contains("mine"));
    h.get_by_label("Reload").click();
    h.run_steps(3);
    assert_eq!(h.state().editor_file_text(), disk);
    assert_eq!(read(&project, "alpha.md"), disk);
}

/// The same for a change to a note's `presentation:` block.
#[test]
fn a_presentation_change_shows_another_programs_change() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, project, _) = open(tmp.path(), notes, 2);
    let alpha = project.join("alpha.md");
    select(&mut h, "Alpha");
    let theirs = alpha_with(&h, "theirs");
    write_outside(&mut h, &alpha, &theirs);
    let ctx = h.ctx.clone();
    h.state_mut()
        .apply_for_test(&ctx, Act::ConvertToPresentation);
    wait_for(&mut h, "their text in the editor", |a| {
        let text = a.editor_file_text();
        text.contains("theirs") && text.contains("presentation:")
    });
    assert_eq!(h.state().editor_file_text(), read(&project, "alpha.md"));
}

/// Quitting does not write over another program's change: the file is left
/// as it is, and your edits are written beside it. The log names the copy
/// when the project is next opened.
#[test]
fn quitting_over_another_programs_change_keeps_both() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, project, paths) = open(tmp.path(), notes, 2);
    let alpha = project.join("alpha.md");
    let reopen = |count: usize| {
        let mut h = harness(paths.clone());
        h.run_steps(2);
        h.state_mut().open(project.clone());
        wait_for(&mut h, "the project", move |a| a.entries().len() == count);
        h
    };
    let said = |h: &H| {
        let log = h.state().log_texts();
        log.iter().any(|l| l.contains("Your edits are in"))
    };

    // The file was only touched: the edit is written, with no copy.
    select(&mut h, "Alpha");
    touch(&alpha);
    let mine = alpha_with(&h, "mine");
    h.state_mut().edit_file_for_test(&mine);
    h.state_mut().shutdown();
    drop(h);
    assert_eq!(read(&project, "alpha.md"), mine);
    assert!(copies(&project).is_empty());

    // The file changed: both versions are on disk.
    let mut h = reopen(2);
    assert!(!said(&h));
    select(&mut h, "Alpha");
    let theirs = alpha_with(&h, "theirs");
    write_outside(&mut h, &alpha, &theirs);
    let mine = alpha_with(&h, "mine again");
    h.state_mut().edit_file_for_test(&mine);
    h.state_mut().shutdown();
    drop(h);
    assert_eq!(read(&project, "alpha.md"), theirs);
    let made = copies(&project);
    assert_eq!(made.len(), 1, "{made:?}");
    assert!(made[0].starts_with("alpha (your edits ") && made[0].ends_with(").md"));
    assert_eq!(read(&project, &made[0]), mine);

    // The copy is a note of its own, and the log names it once.
    let mut h = reopen(3);
    assert!(said(&h));
    assert!(h.state().log_texts().iter().any(|l| l.contains(&made[0])));
    h.state_mut().shutdown();
    drop(h);
    let h = reopen(3);
    assert!(!said(&h));
}

/// A note in a background tab that is waiting to ask is not lost when the
/// project closes: its edits are written as a copy.
#[test]
fn closing_the_project_keeps_a_waiting_tabs_edits() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, project, _) = open(tmp.path(), notes, 2);
    let alpha = project.join("alpha.md");
    select(&mut h, "Beta");
    select(&mut h, "Alpha");
    let theirs = alpha_with(&h, "theirs");
    let mine = alpha_with(&h, "left behind");
    write_outside(&mut h, &alpha, &theirs);
    h.state_mut().edit_file_for_test(&mine);
    let ctx = h.ctx.clone();
    h.state_mut().apply_for_test(&ctx, Act::NextTab);
    let beta = id_of(&h, "Beta");
    wait_for(&mut h, "Beta", move |a| {
        a.editor_file_id().as_deref() == Some(beta.as_str())
    });
    // The save is refused, and waits for Alpha's tab to be shown.
    for _ in 0..40 {
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(read(&project, "alpha.md"), theirs);
    h.state_mut().apply_for_test(&ctx, Act::GoHome);
    wait_for(&mut h, "the close", |a| a.project_index().is_none());
    assert_eq!(read(&project, "alpha.md"), theirs);
    let made = copies(&project);
    assert_eq!(made.len(), 1, "{made:?}");
    assert_eq!(read(&project, &made[0]), mine);
}

/// The backend has found a change, and the app has not heard of it yet. A
/// save sent in that gap is not written over the change.
#[test]
fn a_save_sent_before_the_app_hears_of_a_change_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, project, _) = open(tmp.path(), notes, 2);
    let alpha = project.join("alpha.md");
    select(&mut h, "Beta");
    select(&mut h, "Alpha");

    // In the tab shown: the dialog asks, and the file is theirs.
    let theirs = alpha_with(&h, "theirs");
    let mine = alpha_with(&h, "mine");
    write_outside(&mut h, &alpha, &theirs);
    h.state_mut().check_unheard_for_test();
    h.state_mut().edit_file_for_test(&mine);
    h.state_mut().flush_for_test();
    wait_for(&mut h, "the question", |a| a.has_overlay());
    for _ in 0..10 {
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(read(&project, "alpha.md"), theirs);
    assert_eq!(h.state().editor_file_text(), mine);
    h.get_by_label("Reload").click();
    h.run_steps(3);
    assert_eq!(h.state().editor_file_text(), theirs);
    wait_for(&mut h, "the reload to settle", |a| {
        a.editor_status() == SaveStatus::Saved
    });
    assert_eq!(read(&project, "alpha.md"), theirs);

    // As the tab is left: it asks on return.
    let theirs = alpha_with(&h, "theirs again");
    let mine = alpha_with(&h, "left behind");
    write_outside(&mut h, &alpha, &theirs);
    h.state_mut().check_unheard_for_test();
    h.state_mut().edit_file_for_test(&mine);
    let ctx = h.ctx.clone();
    h.state_mut().apply_for_test(&ctx, Act::NextTab);
    let beta = id_of(&h, "Beta");
    wait_for(&mut h, "Beta", move |a| {
        a.editor_file_id().as_deref() == Some(beta.as_str())
    });
    for _ in 0..20 {
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(read(&project, "alpha.md"), theirs);
    select(&mut h, "Alpha");
    wait_for(&mut h, "the question on return", |a| a.has_overlay());
    h.run_steps(3);
    assert_eq!(h.state().editor_file_text(), mine);
    h.get_by_label("Keep Mine").click();
    let (file, want) = (alpha.clone(), mine.clone());
    wait_for(&mut h, "my edits on disk", move |_| {
        std::fs::read_to_string(&file).is_ok_and(|s| s == want)
    });
    wait_for(&mut h, "the save to settle", |a| {
        a.editor_status() == SaveStatus::Saved
    });

    // On quit: both versions are kept.
    let theirs = alpha_with(&h, "theirs once more");
    let mine = alpha_with(&h, "mine on quit");
    write_outside(&mut h, &alpha, &theirs);
    h.state_mut().check_unheard_for_test();
    h.state_mut().edit_file_for_test(&mine);
    h.state_mut().shutdown();
    drop(h);
    assert_eq!(read(&project, "alpha.md"), theirs);
    let made = copies(&project);
    assert_eq!(made.len(), 1, "{made:?}");
    assert_eq!(read(&project, &made[0]), mine);
}

/// A save that has no answer yet when the app quits is not lost: it is
/// written, or kept as a copy if another program changed the file.
#[test]
fn quitting_with_a_save_in_flight_keeps_the_edit() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, project, paths) = open(tmp.path(), notes, 2);
    let alpha = project.join("alpha.md");
    let leave_and_quit = |mut h: H, mine: &str| {
        h.state_mut().edit_file_for_test(mine);
        let ctx = h.ctx.clone();
        h.state_mut().apply_for_test(&ctx, Act::NextTab);
        h.state_mut().shutdown();
    };

    // The file has not changed: the edit is written, with no copy.
    select(&mut h, "Beta");
    select(&mut h, "Alpha");
    let mine = alpha_with(&h, "mine");
    leave_and_quit(h, &mine);
    assert_eq!(read(&project, "alpha.md"), mine);
    assert!(copies(&project).is_empty());

    // Another program changed it: both versions are kept.
    let mut h = harness(paths.clone());
    h.run_steps(2);
    h.state_mut().open(project.clone());
    wait_for(&mut h, "the project", |a| a.entries().len() == 2);
    select(&mut h, "Beta");
    select(&mut h, "Alpha");
    let theirs = alpha_with(&h, "theirs");
    let mine = alpha_with(&h, "mine again");
    write_outside(&mut h, &alpha, &theirs);
    leave_and_quit(h, &mine);
    assert_eq!(read(&project, "alpha.md"), theirs);
    let made = copies(&project);
    assert_eq!(made.len(), 1, "{made:?}");
    assert_eq!(read(&project, &made[0]), mine);
}

/// The backend has found a change to a note, and the app has not heard of
/// it yet. A tag added in that gap is still in the note once it reloads.
#[test]
fn a_tag_added_before_the_app_hears_of_a_change_stays() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut h, project, _) = open(tmp.path(), notes, 2);
    let alpha = project.join("alpha.md");
    select(&mut h, "Alpha");
    let theirs = alpha_with(&h, "theirs");
    write_outside(&mut h, &alpha, &theirs);
    h.state_mut().check_unheard_for_test();
    h.state_mut().add_tag_for_test("later");
    wait_for(&mut h, "their text and the tag in the editor", |a| {
        let text = a.editor_file_text();
        text.contains("theirs") && text.contains("  - later\n")
    });
    let file = alpha.clone();
    wait_for(&mut h, "the same on disk", move |a| {
        std::fs::read_to_string(&file).is_ok_and(|s| s == a.editor_file_text())
    });
}
