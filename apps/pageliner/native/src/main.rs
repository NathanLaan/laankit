#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! PageLiner, natively: an egui UI over `pageliner-core`, with a hayro PDF
//! reader and no webview. Reads and writes the same `~/.config/PageLiner`
//! (UI prefs and the library) as the Electron and Tauri builds. See
//! `README.md`.

mod app;
mod backend;
mod commands;
mod config;
mod model;
mod overlays;
mod reader;
mod views;

#[cfg(test)]
mod ui_tests;

use config::Paths;

/// The app id the window manager groups the windows by (the Wayland app id
/// and X11 WM class, matching `com.pageliner.app.desktop`): the Tauri and
/// Electron builds' `identifier` / `appId`.
const APP_ID: &str = "com.pageliner.app";
/// Where eframe keeps its own state (`app.ron`): the folder named after the
/// pre-cutover app id, so saved preferences survive the id change.
const STORAGE_ID: &str = "pageliner";

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let paths = Paths::default_location();
    // `main_window` in the Tauri host: 1200 × 800, at least 800 × 500.
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("PageLiner")
        .with_app_id(APP_ID)
        .with_inner_size([1200.0, 800.0])
        .with_min_inner_size([800.0, 500.0])
        .with_drag_and_drop(true)
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
        "PageLiner",
        options,
        Box::new(move |cc| Ok(Box::new(app::PageLinerApp::new(cc.storage, paths)))),
    )
}

/// The window and taskbar icon (`apps/pageliner/assets/icon.png`).
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

/// `<data dir>/pageliner/app.ron` (`eframe::storage_dir`), created if missing.
fn storage_file() -> Option<std::path::PathBuf> {
    let dir = eframe::storage_dir(STORAGE_ID)?;
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("app.ron"))
}
