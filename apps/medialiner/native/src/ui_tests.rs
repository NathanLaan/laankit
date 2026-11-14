//! Drives the app headlessly (egui_kittest) against temp folders: adding a
//! folder and browsing its tree, files, and grid; the preview; the
//! right-click menu and the Info pane; Include Subfolders; outside changes;
//! merging a folder and undoing it; removing a folder from the list; and
//! the settings coming back after a restart.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use egui::accesskit::Role;
use egui::{Key, Modifiers};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use egui_kittest::Node;

use crate::app::{Launch, MediaLinerApp};
use crate::commands::{Act, Focus};
use crate::config::Paths;

type H = Harness<'static, MediaLinerApp>;

fn harness(paths: Paths) -> H {
    Harness::builder()
        .with_size(egui::vec2(1360.0, 860.0))
        .with_step_dt(1.0 / 60.0)
        .with_context_setup(laankit_egui::fonts::install)
        .build_ui_state(
            |ui, app: &mut MediaLinerApp| app.show(ui),
            MediaLinerApp::new(
                None,
                paths,
                Launch {
                    force_copy: false,
                    ..Default::default()
                },
            ),
        )
}

/// Steps until `done` holds (the backend answers on its own threads).
fn wait_for(h: &mut H, what: &str, done: impl Fn(&mut MediaLinerApp) -> bool) {
    let start = Instant::now();
    while !done(h.state_mut()) {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "timed out waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(5));
        h.step();
    }
    h.run_steps(2);
}

/// A PNG of `w` × `h`.
fn photo(path: &Path, w: u32, h: u32) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    image::RgbImage::from_pixel(w, h, image::Rgb([40, 120, 200]))
        .save(path)
        .unwrap();
}

/// Photos/a.png, b.png, notes.txt, and 2025/Trip/c.png.
fn library(tmp: &Path) -> PathBuf {
    let root = tmp.join("Photos");
    photo(&root.join("a.png"), 64, 48);
    photo(&root.join("b.png"), 32, 32);
    fs::write(root.join("notes.txt"), "hello").unwrap();
    photo(&root.join("2025/Trip/c.png"), 20, 10);
    root
}

fn with_folder() -> (tempfile::TempDir, PathBuf, H) {
    let tmp = tempfile::tempdir().unwrap();
    let root = library(tmp.path());
    let mut h = harness(Paths::under(tmp.path()));
    h.run_steps(2);
    h.state_mut().add_folder_for_test(root.clone());
    wait_for(&mut h, "the listing", |a| {
        a.listing_done() && a.photo_count() == 2
    });
    (tmp, root, h)
}

/// The grid's cell for `name` (the Files pane's row comes first).
fn cell<'a>(h: &'a H, name: &'a str) -> Node<'a> {
    h.get_all_by(|n| n.role() == Role::ListItem && n.label().as_deref() == Some(name))
        .last()
        .expect("a grid cell")
}

#[test]
fn adding_a_folder_shows_its_tree_files_and_grid() {
    let (tmp, root, mut h) = with_folder();
    assert_eq!(h.state().roots(), std::slice::from_ref(&root));
    assert_eq!(h.state().folder(), Some(root.as_path()));
    assert_eq!(h.state().file_names(), ["a.png", "b.png", "notes.txt"]);
    let a = root.join("a.png");
    wait_for(&mut h, "a thumbnail", |app| app.thumbnail_ready(&a));
    // The thumbnail is cached on disk.
    assert!(tmp.path().join("cache/thumbs").is_dir());

    // The tree: expand Photos, then select 2025.
    h.get_by_label("Expand Photos").click();
    h.run_steps(2);
    h.get_by_role_and_label(Role::TreeItem, "2025").click();
    h.run_steps(2);
    assert_eq!(h.state().folder(), Some(root.join("2025").as_path()));
    assert_eq!(h.state().focus(), Focus::Tree);
    wait_for(&mut h, "the empty listing", |a| a.listing_done());
    assert_eq!(h.state().photo_count(), 0);
    h.get_by_label("No photos in this folder.");

    // Down goes to the next row (none yet), Right opens 2025, Down selects Trip.
    h.key_press(Key::ArrowRight);
    h.run_steps(2);
    assert!(h.state().expanded(&root.join("2025")));
    h.key_press(Key::ArrowDown);
    h.run_steps(2);
    assert_eq!(h.state().folder(), Some(root.join("2025/Trip").as_path()));
    h.key_press(Key::ArrowLeft);
    h.run_steps(2);
    assert_eq!(h.state().folder(), Some(root.join("2025").as_path()));
}

#[test]
fn the_preview_steps_through_photos_and_goes_back() {
    let (_tmp, root, mut h) = with_folder();
    cell(&h, "a.png").click();
    h.run_steps(2);
    assert_eq!(h.state().selected(), Some(root.join("a.png").as_path()));
    h.key_press(Key::Enter);
    h.run_steps(2);
    assert_eq!(h.state().preview_path(), Some(root.join("a.png").as_path()));
    wait_for(&mut h, "the preview", |a| a.preview_ready());
    h.get_by_label("1 of 2");

    h.key_press(Key::ArrowRight);
    h.run_steps(2);
    assert_eq!(h.state().preview_path(), Some(root.join("b.png").as_path()));
    // The last photo: Right stays.
    h.key_press(Key::ArrowRight);
    h.run_steps(2);
    assert_eq!(h.state().preview_path(), Some(root.join("b.png").as_path()));

    h.key_press(Key::Escape);
    h.run_steps(2);
    assert_eq!(h.state().preview_path(), None);
    assert_eq!(h.state().selected(), Some(root.join("b.png").as_path()));
    // In the grid, Left moves the selection.
    h.key_press(Key::ArrowLeft);
    h.run_steps(2);
    assert_eq!(h.state().selected(), Some(root.join("a.png").as_path()));
}

#[test]
fn show_info_from_the_right_click_menu() {
    let (_tmp, root, mut h) = with_folder();
    assert!(!h.state().info_visible());
    cell(&h, "a.png").click_secondary();
    h.run_steps(2);
    h.get_by_label("Show Info").click();
    h.run_steps(2);
    assert!(h.state().info_visible());
    wait_for(&mut h, "the info", |a| a.info().is_some());
    let info = h.state().info().unwrap();
    assert_eq!(info.path, root.join("a.png"));
    assert_eq!(info.dimensions, Some((64, 48)));
    h.get_by_label("64 × 48");
    h.get_by_label("PNG");

    // The Info pane follows the selection.
    cell(&h, "b.png").click();
    wait_for(&mut h, "b's info", |a| {
        a.info().is_some_and(|i| i.dimensions == Some((32, 32)))
    });
    h.get_by_label("Close Info").click();
    h.run_steps(2);
    assert!(!h.state().info_visible());
}

#[test]
fn include_subfolders_and_outside_changes() {
    let (_tmp, root, mut h) = with_folder();
    h.get_by_label("Include Subfolders (Off)").click();
    wait_for(&mut h, "the deep listing", |a| a.photo_count() == 3);
    assert_eq!(
        h.state().file_names(),
        ["a.png", "b.png", "notes.txt", "c.png"]
    );
    let ctx = h.ctx.clone();
    h.state_mut().run(&ctx, Act::ToggleSubfolders);
    wait_for(&mut h, "the shallow listing", |a| a.photo_count() == 2);

    // A photo added outside MediaLiner shows up.
    photo(&root.join("d.png"), 8, 8);
    wait_for(&mut h, "the new photo", |a| a.photo_count() == 3);
    fs::remove_file(root.join("a.png")).unwrap();
    wait_for(&mut h, "the removed photo", |a| a.photo_count() == 2);
}

#[test]
fn merging_a_folder_moves_its_files_and_undoes() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("Photos");
    let trip = root.join("Trip");
    let best = root.join("Best");
    photo(&trip.join("x.png"), 8, 8);
    photo(&trip.join("clash.png"), 9, 9);
    photo(&best.join("clash.png"), 10, 10);
    let mut h = harness(Paths::under(tmp.path()));
    h.run_steps(2);
    h.state_mut().add_folder_for_test(trip.clone());
    // Trip alone is listed; adding Photos replaces it.
    h.state_mut().add_folder_for_test(root.clone());
    assert_eq!(h.state().roots(), std::slice::from_ref(&root));
    h.run_steps(2);
    h.get_by_label("Expand Photos").click();
    h.run_steps(2);
    h.get_by_role_and_label(Role::TreeItem, "Trip").click();
    h.run_steps(2);

    let ctx = h.ctx.clone();
    h.state_mut().run(&ctx, Act::MergeInto);
    h.run_steps(2);
    assert!(h.state().has_overlay());
    // The dialog's tree is drawn after the sidebar's.
    h.get_all_by_label("Best").last().unwrap().click();
    wait_for(&mut h, "the plan", |a| a.merge_plan_ready());
    h.get_by_label_contains("Moves 2 photos");
    h.get_by_label_contains("1 file has the same name");
    h.get_by_label("Merge").click();
    wait_for(&mut h, "the merge", |a| !a.merging() && !a.has_overlay());

    assert!(!trip.exists());
    assert!(best.join("x.png").exists());
    assert!(best.join("clash (2).png").exists());
    assert_eq!(
        image::image_dimensions(best.join("clash.png")).unwrap(),
        (10, 10)
    );
    assert_eq!(h.state().folder(), Some(best.as_path()));
    assert!(h.state().can_undo_merge());
    assert_eq!(h.state().journals().len(), 1);

    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run_steps(2);
    wait_for(&mut h, "the undo", |a| !a.merging());
    assert!(trip.join("x.png").exists());
    assert_eq!(
        image::image_dimensions(trip.join("clash.png")).unwrap(),
        (9, 9)
    );
    assert!(!best.join("x.png").exists());
    assert!(!h.state().can_undo_merge());
    assert!(h.state().journals()[0].undone);
}

#[test]
fn removing_a_folder_from_the_list_leaves_it_on_disk() {
    let (_tmp, root, mut h) = with_folder();
    h.get_by_role_and_label(Role::TreeItem, "Photos")
        .click_secondary();
    h.run_steps(2);
    h.get_by_label("Remove from List").click();
    h.run_steps(2);
    assert!(h.state().roots().is_empty());
    assert_eq!(h.state().folder(), None);
    assert!(root.join("a.png").exists());
    h.get_by_label("No folders yet");
}

#[test]
fn folders_and_preferences_come_back_after_a_restart() {
    let (tmp, root, mut h) = with_folder();
    h.get_by_label("Expand Photos").click();
    h.run_steps(2);
    h.get_by_role_and_label(Role::TreeItem, "2025").click();
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::COMMAND, Key::Equals);
    h.key_press_modifiers(Modifiers::COMMAND, Key::I);
    h.run_steps(2);
    drop(h);

    let paths = Paths::under(tmp.path());
    assert_eq!(paths.prefs().thumb_size, 192.0);
    assert!(paths.prefs().info_visible);
    let mut h = harness(paths);
    h.run_steps(2);
    assert_eq!(h.state().roots(), std::slice::from_ref(&root));
    assert_eq!(h.state().folder(), Some(root.join("2025").as_path()));
    assert!(h.state().info_visible());
    h.get_by_role_and_label(Role::TreeItem, "2025");
}
