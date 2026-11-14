#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! PrintLiner: a dashboard for the Bambu Lab 3D printers on your Bambu
//! account, through the Bambu Lab cloud. An egui UI over `printliner-core`.
//!
//! `printliner --demo` shows three made-up printers, with no account.

mod app;
mod backend;
mod commands;
mod config;
mod detail;
mod model;
mod overlays;
mod views;

#[cfg(test)]
mod ui_tests;

use app::{Launch, PrintLinerApp};
use config::Paths;

/// The app id the window manager groups the windows by (the Wayland app id
/// and X11 WM class, matching `com.printliner.app.desktop`).
const APP_ID: &str = "com.printliner.app";
/// Where eframe keeps its own state (`app.ron`), and PrintLiner its history
/// and cover cache.
const STORAGE_ID: &str = "printliner";

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: printliner [--demo]\n\n  --demo  Show three made-up printers, with no Bambu Lab account.");
        return Ok(());
    }
    let launch = Launch {
        demo: args.iter().any(|a| a == "--demo"),
        ..Default::default()
    };
    let paths = Paths::default_location(eframe::storage_dir(STORAGE_ID));
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(if launch.demo {
            "PrintLiner (Demo)"
        } else {
            "PrintLiner"
        })
        .with_app_id(APP_ID)
        .with_inner_size([1280.0, 820.0])
        .with_min_inner_size([720.0, 480.0])
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
        "PrintLiner",
        options,
        Box::new(move |cc| Ok(Box::new(PrintLinerApp::new(cc.storage, paths, launch)))),
    )
}

/// The window and taskbar icon (`apps/printliner/assets/icon.png`).
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

/// `<data dir>/printliner/app.ron` (`eframe::storage_dir`), created if missing.
fn storage_file() -> Option<std::path::PathBuf> {
    let dir = eframe::storage_dir(STORAGE_ID)?;
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("app.ron"))
}
