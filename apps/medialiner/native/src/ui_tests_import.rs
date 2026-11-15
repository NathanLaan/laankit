//! Drives importing from a device headlessly (egui_kittest): a folder made
//! up as a camera's card stands in for the device, plugged in as the
//! watcher would report it.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use egui::accesskit::Role;
use egui_kittest::kittest::Queryable;
use medialiner_core::devices::Volume;
use serde_json::json;

use crate::commands::Act;
use crate::config::Paths;
use crate::overlays::import::{Phase, Scan};
use crate::ui_tests::{harness, photo, wait_for, with_folder, H};

/// A card with three photos in two of the camera's folders, a video, and
/// the camera's own index file.
fn card(tmp: &Path) -> (PathBuf, Volume) {
    let root = tmp.join("card");
    photo(&root.join("DCIM/100MSDCF/DSC0001.JPG"), 16, 12);
    photo(&root.join("DCIM/100MSDCF/DSC0002.JPG"), 12, 12);
    photo(&root.join("DCIM/101MSDCF/DSC0001.JPG"), 8, 8);
    fs::create_dir_all(root.join("MP_ROOT")).unwrap();
    fs::write(root.join("MP_ROOT/MAH0001.MP4"), b"a video").unwrap();
    fs::write(root.join("MP_ROOT/MAH0001.THM"), b"index").unwrap();
    let volume = Volume::folder(&root, "CARD", "uuid:test-card");
    (root, volume)
}

/// The dialog's button named `label` (the toolbar's Import comes first).
fn click(h: &mut H, label: &str) {
    h.get_all_by(|n| n.role() == Role::Button && n.label().as_deref() == Some(label))
        .last()
        .unwrap_or_else(|| panic!("no {label} button"))
        .click();
    h.run_steps(2);
}

fn scanned(h: &mut H) {
    wait_for(h, "the device's files", |a| {
        a.import_dialog()
            .is_some_and(|s| matches!(s.scan, Scan::Ready { .. }))
    });
}

fn done(h: &mut H) {
    wait_for(h, "the import", |a| {
        a.import_dialog()
            .is_some_and(|s| matches!(s.phase, Phase::Done))
    });
}

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .flatten()
        .filter(|e| e.path().is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

#[test]
fn plugging_in_a_card_offers_the_dialog_and_imports_into_a_new_folder() {
    let (tmp, root, mut h) = with_folder();
    let (cardp, volume) = card(tmp.path());
    h.state_mut().plug_in(volume);
    scanned(&mut h);
    h.get_by_label_contains("3 photos and 1 video (");
    // The files, each with its date; the camera's index file is left out.
    let listed = h
        .get_all_by(|n| {
            n.role() == Role::ListItem && n.label().is_some_and(|l| l.starts_with("DSC0002.JPG 20"))
        })
        .count();
    assert_eq!(listed, 1);
    let state = h.state_mut().import_dialog().unwrap();
    assert_eq!(state.chosen().len(), 4);
    // The folder selected in the app is where it will go.
    assert_eq!(state.dest.as_deref(), Some(root.as_path()));

    // A new folder inside it: a name that's taken is refused.
    click(&mut h, "New Folder…");
    h.state_mut().import_dialog().unwrap().new_folder = Some("2025".into());
    click(&mut h, "Create");
    assert!(h
        .state_mut()
        .import_dialog()
        .unwrap()
        .new_folder_error
        .is_some());
    h.state_mut().import_dialog().unwrap().new_folder = Some("From Card".into());
    click(&mut h, "Create");
    let dest = root.join("From Card");
    assert!(dest.is_dir());
    assert_eq!(
        h.state_mut().import_dialog().unwrap().dest.as_deref(),
        Some(dest.as_path())
    );

    // The dialog settles to its new height over a few frames.
    h.run_steps(8);
    click(&mut h, "Import");
    done(&mut h);
    h.get_by_label("Copied 4 files into From Card.");
    assert_eq!(
        names(&dest),
        [
            "DSC0001 (2).JPG",
            "DSC0001.JPG",
            "DSC0002.JPG",
            "MAH0001.MP4"
        ]
    );
    // The card is untouched.
    assert!(cardp.join("DCIM/100MSDCF/DSC0001.JPG").is_file());
    assert!(cardp.join("MP_ROOT/MAH0001.THM").is_file());

    click(&mut h, "Done");
    assert!(!h.state().has_overlay());
    assert_eq!(h.state().folder(), Some(dest.as_path()));
    wait_for(&mut h, "the imported photos", |a| a.photo_count() == 3);

    // Undo Last Import removes the copies.
    assert!(h.state().can_undo_import());
    let ctx = h.ctx.clone();
    h.state_mut().run(&ctx, Act::UndoImport);
    wait_for(&mut h, "the undo", |a| !a.can_undo_import());
    assert!(names(&dest).is_empty());
    assert!(h
        .state()
        .toast_texts()
        .iter()
        .any(|t| t.starts_with("Import undone: removed 4 files")));
}

#[test]
fn the_same_card_again_offers_only_the_new_photos_and_asks_before_deleting() {
    let (tmp, root, mut h) = with_folder();
    let (cardp, volume) = card(tmp.path());
    h.state_mut().plug_in(volume.clone());
    scanned(&mut h);
    click(&mut h, "Import");
    done(&mut h);
    click(&mut h, "Done");
    h.state_mut().unplug(&volume);

    photo(&cardp.join("DCIM/101MSDCF/DSC0009.JPG"), 10, 10);
    h.state_mut().plug_in(volume);
    scanned(&mut h);
    let state = h.state_mut().import_dialog().unwrap();
    assert!(state.new_only);
    let chosen: Vec<String> = state.chosen().iter().map(|i| i.name.clone()).collect();
    assert_eq!(chosen, ["DSC0009.JPG"]);
    h.get_by_label("New since the last import (1)");
    h.get_by_label("All (5)");

    // Delete from device asks once the copy is made.
    h.state_mut().import_dialog().unwrap().delete_after = true;
    click(&mut h, "Import");
    wait_for(&mut h, "the question", |a| {
        a.import_dialog()
            .is_some_and(|s| matches!(s.phase, Phase::AskDelete))
    });
    h.get_by_label("Delete the copied file from CARD?");
    assert!(cardp.join("DCIM/101MSDCF/DSC0009.JPG").is_file());
    assert!(root.join("DSC0009.JPG").is_file());
    click(&mut h, "Delete from Device");
    done(&mut h);
    h.get_by_label("Deleted 1 file from the device.");
    assert!(!cardp.join("DCIM/101MSDCF/DSC0009.JPG").exists());
    // Only the file this import copied went.
    assert!(cardp.join("DCIM/100MSDCF/DSC0001.JPG").is_file());
    assert!(root.join("DSC0009.JPG").is_file());
    click(&mut h, "Done");
    // An import that deleted from the device can't be undone.
    assert!(!h.state().can_undo_import());
}

#[test]
fn keeping_the_photos_on_the_device_leaves_it_alone() {
    let (tmp, _root, mut h) = with_folder();
    let (cardp, volume) = card(tmp.path());
    h.state_mut().plug_in(volume);
    scanned(&mut h);
    h.state_mut().import_dialog().unwrap().delete_after = true;
    click(&mut h, "Import");
    wait_for(&mut h, "the question", |a| {
        a.import_dialog()
            .is_some_and(|s| matches!(s.phase, Phase::AskDelete))
    });
    click(&mut h, "Keep on Device");
    assert!(matches!(
        h.state_mut().import_dialog().unwrap().phase,
        Phase::Done
    ));
    assert!(cardp.join("DCIM/100MSDCF/DSC0001.JPG").is_file());
    click(&mut h, "Done");
    assert!(h.state().can_undo_import());
}

/// Long enough for a scan that offers nothing to have finished.
fn settle(h: &mut H) {
    for _ in 0..40 {
        std::thread::sleep(Duration::from_millis(5));
        h.step();
    }
}

#[test]
fn nothing_is_offered_for_documents_a_listed_folder_or_with_the_setting_off() {
    let (tmp, root, mut h) = with_folder();
    // A stick of documents.
    let docs = tmp.path().join("stick");
    fs::create_dir_all(&docs).unwrap();
    fs::write(docs.join("report.docx"), b"x").unwrap();
    h.state_mut().plug_in(Volume::folder(&docs, "STICK", "k"));
    settle(&mut h);
    assert!(!h.state().has_overlay());
    // A folder that's already in the list.
    h.state_mut().plug_in(Volume::folder(&root, "Photos", "k2"));
    settle(&mut h);
    assert!(!h.state().has_overlay());

    // The setting off: a card opens nothing, but the command still works.
    let tmp2 = tempfile::tempdir().unwrap();
    let paths = Paths::under(tmp2.path());
    laankit_core::json::write(&paths.settings_file(), &json!({ "offerImport": false })).unwrap();
    let mut h = harness(paths);
    h.run_steps(2);
    let (_, volume) = card(tmp2.path());
    h.state_mut().plug_in(volume.clone());
    settle(&mut h);
    assert!(!h.state().has_overlay());
    let ctx = h.ctx.clone();
    h.state_mut().run(&ctx, Act::ImportFromDevice);
    wait_for(&mut h, "the device list", |a| {
        a.import_dialog().is_some_and(|s| !s.looking)
    });
    h.get_by_label(
        "No device found. Plug in a memory card, a USB drive, or a camera, and it will show here.",
    );
    // A device plugged in while it waits shows up in it.
    h.state_mut().plug_in(volume);
    scanned(&mut h);
    // With no folders in the list, there's nowhere to import to yet.
    h.get_by_label("Add a folder to your list to import into.");
}

#[test]
fn a_device_waits_for_an_open_dialog_and_unplugging_closes_the_offer() {
    let (tmp, _root, mut h) = with_folder();
    let (_, volume) = card(tmp.path());
    let ctx = h.ctx.clone();
    h.state_mut().run(&ctx, Act::About);
    h.run_steps(2);
    h.state_mut().plug_in(volume.clone());
    wait_for(&mut h, "the notice", |a| {
        a.toast_texts()
            .iter()
            .any(|t| t.starts_with("CARD: 3 photos"))
    });
    assert!(h.state_mut().import_dialog().is_none());
    // Closing About brings the Import dialog up.
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    scanned(&mut h);

    h.state_mut().unplug(&volume);
    h.run_steps(2);
    assert!(!h.state().has_overlay());
    assert!(h
        .state()
        .toast_texts()
        .iter()
        .any(|t| t == "CARD was removed."));
}
