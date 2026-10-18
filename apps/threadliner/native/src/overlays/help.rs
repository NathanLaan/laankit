//! Help (`HelpApp.svelte`). The Svelte builds open it in a second window;
//! here it's a full-size dialog with the same sections.

use egui::RichText;
use marina_egui::dialog::{Dialog, Size};
use marina_egui::{fonts, icons, theme};

/// (title, [(name, body)]). `{icon}` placeholders become Font Awesome glyphs.
const SECTIONS: [(&str, &[(&str, &str)]); 4] = [
    (
        "Getting started",
        &[
            ("Add a feed", "Click {plus} in the toolbar (or in the titlebar if the custom titlebar is enabled). Paste an RSS, Atom, or JSON Feed URL."),
            ("Refresh a feed", "Select a feed in the sidebar, then click {rotate} to pull its latest entries. The auto-sync engine commits and pushes changes in the background."),
            ("Mark entries read or unread", "Select an entry and click {check} to mark all read, or {undo} to mark all unread. With an entry selected, those buttons act on the single entry."),
        ],
    ),
    (
        "Tags",
        &[
            ("Create tags", "Open the Tags dialog from the toolbar ({tags}). Add new tags from the action column on the left."),
            ("Assign tags", "Select a feed in the sidebar, then click a tag in the Tags section below the feed list to toggle its assignment."),
        ],
    ),
    (
        "Sync",
        &[
            ("Auto-sync", "Every local change auto-commits and pushes after a debounce window. The Sync toolbar button tints accent when a push is queued."),
            ("Manual sync", "Click {cloud} to open the Sync drawer. From there you can Pull, Pull & Rebase, Push, or Reset from Remote."),
            ("Sync activity log", "Open Settings → Sync to see the most recent commit/push/pull operations."),
        ],
    ),
    (
        "Settings",
        &[
            ("Theme", "Settings → UI. Three themes: Light, Dark, Midnight."),
            ("UI scale", "Settings → UI. Or use Ctrl+= / Ctrl+- / Ctrl+0 from anywhere in the app."),
            ("Custom titlebar", "Settings → UI → Window. Replaces the OS frame with an in-app titlebar. Requires a restart to take effect."),
            ("Sync wait time", "Settings → Sync. How long the auto-sync engine waits after a change before pushing."),
        ],
    ),
];

fn expand(body: &str) -> String {
    body.replace("{plus}", icons::PLUS)
        .replace("{rotate}", icons::ARROWS_ROTATE)
        .replace("{check}", icons::CHECK_DOUBLE)
        .replace("{undo}", icons::ROTATE_LEFT)
        .replace("{tags}", icons::TAGS)
        .replace("{cloud}", icons::CLOUD_ARROW_UP)
}

/// Draws Help; returns `true` when it should close.
pub fn show(ctx: &egui::Context, top_inset: f32) -> bool {
    let p = theme::palette(ctx);
    let resp = Dialog::full("help", "ThreadLiner Help")
        .size(Size::Full { top_inset })
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_max_width(760.0);
                    ui.label(
                        RichText::new("RSS reader with Git-synced JSON storage.")
                            .color(p.text_muted),
                    );
                    for (title, items) in SECTIONS {
                        ui.add_space(20.0);
                        ui.label(fonts::styled(
                            RichText::new(title).size(17.0).color(p.text_primary),
                            fonts::Style::BOLD,
                        ));
                        ui.add_space(6.0);
                        for (name, body) in items {
                            ui.label(fonts::weight(
                                RichText::new(*name).color(p.text_primary),
                                fonts::SEMIBOLD,
                            ));
                            egui::Frame::new()
                                .inner_margin(egui::Margin {
                                    left: 16,
                                    ..Default::default()
                                })
                                .show(ui, |ui| {
                                    ui.label(RichText::new(expand(body)).color(p.text_secondary));
                                });
                            ui.add_space(6.0);
                        }
                    }
                });
        });
    resp.close
}
