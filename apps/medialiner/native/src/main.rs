#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! MediaLiner: browse and organize the photos in your folders, where they
//! are. An egui UI over `medialiner-core`.
//!
//! `medialiner <folder>` adds the folder (if it isn't listed yet) and
//! selects it.

mod app;
mod backend;
mod commands;
mod config;
mod overlays;
mod textures;
mod views;

#[cfg(test)]
mod ui_tests;

use app::{Launch, MediaLinerApp};
use config::Paths;

/// The app id the window manager groups the windows by (the Wayland app id
/// and X11 WM class, matching `com.medialiner.app.desktop`).
const APP_ID: &str = "com.medialiner.app";
/// Where eframe keeps its own state (`app.ron`), and MediaLiner its merge
/// journals.
const STORAGE_ID: &str = "medialiner";

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!(
            "Usage: medialiner [folder]\n\n  folder  Add this folder to the list, and select it."
        );
        return Ok(());
    }
    let launch = Launch {
        open: args
            .iter()
            .find(|a| !a.starts_with('-'))
            .map(|a| std::path::absolute(a).unwrap_or_else(|_| a.into())),
        ..Default::default()
    };
    let paths = Paths::default_location(eframe::storage_dir(STORAGE_ID));
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("MediaLiner")
        .with_app_id(APP_ID)
        .with_inner_size([1360.0, 860.0])
        .with_min_inner_size([760.0, 480.0])
        .with_icon(app_icon());
    if paths.prefs().custom_titlebar {
        viewport = laankit_egui::window::undecorated(viewport);
    }
    let options = eframe::NativeOptions {
        viewport,
        persist_window: true,
        persistence_path: storage_file(),
        ..Default::default()
    };
    eframe::run_native(
        "MediaLiner",
        options,
        Box::new(move |cc| Ok(Box::new(MediaLinerApp::new(cc.storage, paths, launch)))),
    )
}

/// The window and taskbar icon (`apps/medialiner/assets/icon.png`).
fn app_icon() -> egui::IconData {
    const ICON: &[u8] = include_bytes!("../../assets/icon.png");
    match image::load_from_memory(ICON) {
        Ok(img) => {
            let rgba = img.into_rgba8();
            egui::IconData {
                width: rgba.width(),
                height: rgba.height(),
                rgba: rgba.into_raw(),
            }
        }
        Err(_) => egui::IconData::default(),
    }
}

/// `<data dir>/medialiner/app.ron` (`eframe::storage_dir`), created if missing.
fn storage_file() -> Option<std::path::PathBuf> {
    let dir = eframe::storage_dir(STORAGE_ID)?;
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("app.ron"))
}
