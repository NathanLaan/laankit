#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! NoteLiner, natively: an egui UI over `noteliner-core`, with no webview.
//! Reads and writes the same projects and `~/.config/NoteLiner` (UI prefs,
//! recent projects, window state, the MCP runtime file) as the Electron and
//! Tauri builds. See `README.md`.
//!
//! `noteliner --mcp-bridge` runs the stdio ↔ socket bridge that MCP
//! clients spawn, instead of the app (as the Tauri binary's flag does).

mod app;
mod backend;
mod commands;
mod config;
mod deck;
mod editor;
mod math;
mod model;
mod overlays;
mod spell;
mod svg_colors;
mod views;

#[cfg(test)]
mod ui_tests;
#[cfg(test)]
mod ui_tests_editor;
#[cfg(test)]
mod ui_tests_panes;
#[cfg(test)]
mod ui_tests_present;

use config::Paths;

/// The app id the window manager groups the windows by (the Wayland app id
/// and X11 WM class, matching `com.nathanlaan.noteliner.desktop`): the Tauri
/// and Electron builds' `identifier` / `appId`.
const APP_ID: &str = "com.nathanlaan.noteliner";
/// Where eframe keeps its own state (`app.ron`): the folder named after the
/// pre-cutover app id, so saved preferences survive the id change.
const STORAGE_ID: &str = "noteliner";

fn main() -> eframe::Result {
    if std::env::args().any(|a| a == "--mcp-bridge") {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        let user_data = Paths::default_location().user_data;
        let code = rt.block_on(noteliner_core::mcp::run_bridge(&user_data));
        std::process::exit(code);
    }

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let paths = Paths::default_location();
    // `main_window` in the Tauri host: 1200 × 800, at least 800 × 600.
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("NoteLiner")
        .with_app_id(APP_ID)
        .with_inner_size([1200.0, 800.0])
        .with_min_inner_size([800.0, 600.0])
        .with_drag_and_drop(true)
        .with_icon(app_icon());
    if paths.prefs().custom_titlebar {
        viewport = marina_egui::window::undecorated(viewport);
    }
    let options = eframe::NativeOptions {
        viewport,
        // Window bounds are saved per project in `window-state.json`, as the
        // Tauri host does. eframe's own copy would also restore transient
        // states, e.g. reopening full screen after quitting while presenting.
        persist_window: false,
        persistence_path: storage_file(),
        ..Default::default()
    };
    eframe::run_native(
        "NoteLiner",
        options,
        Box::new(move |cc| Ok(Box::new(app::NoteLinerApp::new(cc.storage, paths)))),
    )
}

/// The window and taskbar icon (`apps/noteliner/assets/icon.png`).
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

/// `<data dir>/noteliner/app.ron` (`eframe::storage_dir`), created if missing.
fn storage_file() -> Option<std::path::PathBuf> {
    let dir = eframe::storage_dir(STORAGE_ID)?;
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("app.ron"))
}
