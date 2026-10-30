#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! BlockLiner: a Visio-style editor for software architecture diagrams,
//! saved as Mermaid flowcharts. An egui UI over `blockliner-core`. See
//! `docs/plans/plan-blockliner.md`.

mod app;
mod backend;
mod canvas;
mod commands;
mod config;
mod export;
mod overlays;
mod properties;
mod views;

#[cfg(test)]
mod ui_tests;
#[cfg(test)]
mod ui_tests_clipboard;
#[cfg(test)]
mod ui_tests_export;
#[cfg(test)]
mod ui_tests_history;
#[cfg(test)]
mod ui_tests_import;
#[cfg(test)]
mod ui_tests_lines;
#[cfg(test)]
mod ui_tests_more_shapes;
#[cfg(test)]
mod ui_tests_project;
#[cfg(test)]
mod ui_tests_properties;
#[cfg(test)]
mod ui_tests_routes;
#[cfg(test)]
mod ui_tests_shapes;
#[cfg(test)]
mod ui_tests_styles;

use config::Paths;

/// The app id the window manager groups the windows by (the Wayland app id
/// and X11 WM class).
const APP_ID: &str = "com.blockliner.app";
/// Where eframe keeps its own state (`app.ron`).
const STORAGE_ID: &str = "blockliner";

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let paths = Paths::default_location();
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("BlockLiner")
        .with_app_id(APP_ID)
        .with_inner_size([1280.0, 800.0])
        // Tall enough for every toolbar button (the tools and the bottom group).
        .with_min_inner_size([800.0, 640.0])
        .with_icon(app_icon());
    if paths.prefs().custom_titlebar {
        viewport = marina_egui::window::undecorated(viewport);
    }
    let options = eframe::NativeOptions {
        viewport,
        persist_window: true,
        persistence_path: storage_file(),
        ..Default::default()
    };
    eframe::run_native(
        "BlockLiner",
        options,
        Box::new(move |cc| {
            let mut app = app::BlockLinerApp::new(cc.storage, paths);
            if let Some(path) = launch_path() {
                app.open_path(&path);
            }
            Ok(Box::new(app))
        }),
    )
}

/// `blockliner <file.mmd | folder>`: what to open instead of the last
/// diagram (the desktop entry passes the file it was asked to open).
/// Options (`-…`) are skipped.
fn launch_path() -> Option<std::path::PathBuf> {
    std::env::args_os()
        .skip(1)
        .find(|a| !a.to_string_lossy().starts_with('-'))
        .map(Into::into)
}

/// The window and taskbar icon (`apps/blockliner/assets/icon.png`).
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

/// `<data dir>/blockliner/app.ron` (`eframe::storage_dir`), created if missing.
fn storage_file() -> Option<std::path::PathBuf> {
    let dir = eframe::storage_dir(STORAGE_ID)?;
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("app.ron"))
}
