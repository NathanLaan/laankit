//! The MCP dialogs: `McpConfirmModal.svelte` (an MCP client's write
//! request, one at a time from the app's queue) and
//! `McpWalkthroughModal.svelte` (the consent gate every time MCP is turned
//! on in Settings).

use egui::{RichText, Ui};
use marina_egui::dialog::{self, ButtonKind, Dialog};
use marina_egui::{fonts, icons, theme};
use serde_json::Value;

use crate::backend::{McpConfirm, McpStatus};
use crate::overlays::settings::mcp_config_snippet;

/// `dataBase64` longer than this shows as its length.
const BASE64_PREVIEW: usize = 80;
/// `body` is cut to this many characters.
const BODY_PREVIEW: usize = 400;

/// `prettyArgs`: the arguments as indented JSON, with an attachment's
/// base64 payload and a long note body cut so the dialog stays readable.
pub fn pretty_args(args: &Value) -> String {
    let mut args = args.clone();
    if let Some(map) = args.as_object_mut() {
        if let Some(data) = map.get("dataBase64").and_then(Value::as_str) {
            if data.len() > BASE64_PREVIEW {
                let n = data.len();
                map.insert(
                    "dataBase64".into(),
                    Value::String(format!("<{n} bytes base64>")),
                );
            }
        }
        if let Some(body) = map.get("body").and_then(Value::as_str) {
            let len = body.chars().count();
            if len > BODY_PREVIEW {
                let cut: String = body.chars().take(BODY_PREVIEW).collect();
                let more = len - BODY_PREVIEW;
                map.insert(
                    "body".into(),
                    Value::String(format!("{cut}… (+{more} more chars)")),
                );
            }
        }
    }
    if args.is_null() {
        return String::new();
    }
    serde_json::to_string_pretty(&args).unwrap_or_else(|_| args.to_string())
}

/// `McpConfirmModal`: returns "allow", "session" or "deny". Enter allows
/// once; Escape (or the X) denies. `show_details` is the "raw arguments"
/// disclosure, kept by the app per request.
pub fn confirm(
    ctx: &egui::Context,
    req: &McpConfirm,
    show_details: &mut bool,
) -> Option<&'static str> {
    let title = format!("{}  MCP write request", icons::SHIELD_HALVED);
    let resp = Dialog::compact(("mcp-confirm", req.id), &title).show(ctx, |ui| {
        let p = theme::palette(ui.ctx());
        ui.label(
            RichText::new("An MCP client wants to run a tool that will modify this project.")
                .color(p.text_primary),
        );
        ui.add_space(8.0);
        kv(ui, "Tool", |ui| {
            ui.label(RichText::new(&req.tool).font(fonts::mono(13.0)).color(p.accent));
        });
        kv(ui, "Action", |ui| {
            ui.label(RichText::new(&req.summary).color(p.text_primary));
        });
        ui.add_space(4.0);
        let toggle = ui.add(
            egui::Button::new(
                RichText::new(format!(
                    "{}  {} raw arguments",
                    if *show_details { icons::CHEVRON_DOWN } else { icons::CHEVRON_RIGHT },
                    if *show_details { "Hide" } else { "Show" }
                ))
                .size(12.0)
                .color(p.text_secondary),
            )
            .frame(false),
        );
        if toggle.clicked() {
            *show_details = !*show_details;
        }
        if *show_details {
            egui::Frame::NONE
                .fill(p.pre_bg)
                .stroke(egui::Stroke::new(1.0, p.border))
                .corner_radius(theme::RADIUS)
                .inner_margin(egui::Margin::same(8))
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt(("mcp-args", req.id))
                        .max_height(220.0)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.add(
                                egui::Label::new(
                                    RichText::new(pretty_args(&req.args))
                                        .font(fonts::mono(12.0))
                                        .color(p.text_primary),
                                )
                                .selectable(true),
                            );
                        });
                });
        }
        ui.add_space(6.0);
        ui.label(
            RichText::new(format!(
                "\"Allow for session\" trusts every {} call until you close the project or quit NoteLiner. Allow once approves just this call.",
                req.tool
            ))
            .size(12.0)
            .color(p.text_muted),
        );
        let mut out = None;
        dialog::actions(ui, |ui| {
            if dialog::button(ui, "Allow once", ButtonKind::Primary, true).clicked() {
                out = Some("allow");
            }
            if dialog::button(ui, "Allow for session", ButtonKind::Secondary, true).clicked() {
                out = Some("session");
            }
            if dialog::button(ui, "Deny", ButtonKind::Danger, true).clicked() {
                out = Some("deny");
            }
        });
        if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            out = Some("allow");
        }
        out
    });
    if resp.close {
        return Some("deny");
    }
    resp.inner
}

fn kv(ui: &mut Ui, key: &str, value: impl FnOnce(&mut Ui)) {
    let p = theme::palette(ui.ctx());
    ui.horizontal(|ui| {
        ui.add_sized(
            [64.0, 18.0],
            egui::Label::new(RichText::new(key).size(12.0).color(p.text_muted)),
        );
        value(ui);
    });
}

/// What the walkthrough asks the Settings dialog to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Walkthrough {
    Enable,
    Cancel,
}

/// `McpWalkthroughModal`: how the server works, the client config snippet
/// and the safety notes, before MCP is turned on. Enter enables, Escape
/// (or the X, or a backdrop click) cancels. `copied_until` is when the
/// Copy button stops saying "Copied".
pub fn walkthrough(
    ctx: &egui::Context,
    status: Option<&McpStatus>,
    copied_until: &mut f64,
) -> Option<Walkthrough> {
    let title = format!("{}  Enable the MCP server", icons::PLUG);
    let resp = Dialog::compact("mcp-walkthrough", &title)
        .size(dialog::Size::Compact { width: 620.0 })
        .show(ctx, |ui| {
            let p = theme::palette(ui.ctx());
            let now = ui.input(|i| i.time);
            ui.label(RichText::new(
                "NoteLiner can expose the currently-open project to external AI assistants (Claude Code, Claude Desktop, Cursor, ...) so they can list, read, search, and write notes. Every write goes through the normal save path and is committed to git automatically.",
            ).color(p.text_primary));
            heading(ui, "How it works");
            for (n, text) in [
                "NoteLiner listens on a private local socket while a project is open.",
                "Your AI client launches `noteliner --mcp-bridge`, which pipes its stdio into that socket.",
                "The client calls tools (list_notes, create_note, ...) — you see every operation in the Sync Log.",
            ]
            .iter()
            .enumerate()
            {
                ui.horizontal_top(|ui| {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());
                    ui.painter().circle_filled(rect.center(), 9.0, p.bg_selected);
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        (n + 1).to_string(),
                        egui::FontId::proportional(11.0),
                        p.accent,
                    );
                    ui.add(egui::Label::new(RichText::new(*text).size(13.0).color(p.text_secondary)).wrap());
                });
            }
            heading(ui, "Add this to your MCP client config");
            ui.label(
                RichText::new("For Claude Code: .mcp.json in your project. For Claude Desktop: claude_desktop_config.json. The bridge command below is the one on this machine.")
                    .size(12.0)
                    .color(p.text_muted),
            );
            let snippet = status.map(mcp_config_snippet).unwrap_or_default();
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = if now < *copied_until { "Copied" } else { "Copy" };
                    if dialog::button(ui, label, ButtonKind::Secondary, !snippet.is_empty()).clicked() {
                        ui.ctx().copy_text(snippet.clone());
                        *copied_until = now + 1.5;
                    }
                });
            });
            egui::Frame::NONE
                .fill(p.pre_bg)
                .stroke(egui::Stroke::new(1.0, p.border))
                .corner_radius(theme::RADIUS)
                .inner_margin(egui::Margin::same(10))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    // Wrapped, so a long bridge path can't widen the dialog.
                    ui.add(
                        egui::Label::new(
                            RichText::new(if snippet.is_empty() { "Bridge path unavailable." } else { &snippet })
                                .font(fonts::mono(12.0))
                                .color(p.text_primary),
                        )
                        .wrap()
                        .selectable(true),
                    );
                });
            ui.add_space(6.0);
            for (glyph, text) in [
                (icons::SHIELD_HALVED, "Local-only — no TCP port is opened. The socket file is user-readable only."),
                (icons::CIRCLE_CHECK, "You can require confirmation for every write in Settings → MCP → Safety."),
                (icons::FOLDER_OPEN, "The server only ever exposes the currently-open project, not your other repos."),
            ] {
                ui.horizontal_top(|ui| {
                    ui.label(icons::icon_sized(glyph, 12.0).color(p.accent));
                    ui.add(egui::Label::new(RichText::new(text).size(12.5).color(p.text_secondary)).wrap());
                });
            }
            if now < *copied_until {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
            }
            let mut out = None;
            dialog::actions(ui, |ui| {
                if dialog::button(ui, "Enable MCP server", ButtonKind::Primary, true).clicked() {
                    out = Some(Walkthrough::Enable);
                }
                if dialog::button(ui, "Cancel", ButtonKind::Secondary, true).clicked() {
                    out = Some(Walkthrough::Cancel);
                }
            });
            if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                out = Some(Walkthrough::Enable);
            }
            out
        });
    if resp.close {
        return Some(Walkthrough::Cancel);
    }
    resp.inner
}

fn heading(ui: &mut Ui, text: &str) {
    let p = theme::palette(ui.ctx());
    ui.add_space(8.0);
    ui.label(fonts::weight(
        RichText::new(text).size(13.5).color(p.text_primary),
        fonts::SEMIBOLD,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn args_are_trimmed_for_display() {
        let args = json!({
            "fileId": "a",
            "dataBase64": "x".repeat(200),
            "body": "y".repeat(450),
        });
        let s = pretty_args(&args);
        assert!(s.contains("<200 bytes base64>"), "{s}");
        assert!(s.contains("… (+50 more chars)"), "{s}");
        assert!(s.contains("\"fileId\": \"a\""));
        let short = json!({ "dataBase64": "abc", "body": "hi" });
        assert!(pretty_args(&short).contains("\"abc\""));
        assert_eq!(pretty_args(&Value::Null), "");
    }
}
