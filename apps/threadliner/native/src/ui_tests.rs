//! Drives the app headlessly (egui_kittest) through the main flows, against
//! a temp config folder, a temp git data folder and a local HTTP feed (no
//! network): setup → add a feed → read an entry → mark it unread → create a
//! tag and assign it. Also checks that settings land in the data folder in
//! the shape the Tauri build writes. Needs `git` on the PATH, like the app.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::time::Duration;

use egui::accesskit::Role;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use serde_json::{json, Value};

use crate::app::ThreadLinerApp;
use crate::backend::{Backend, Event, Op};
use crate::config::Paths;

const FEED_XML: &str = r#"<?xml version="1.0"?>
<rss version="2.0"><channel>
  <title>Fixture Feed</title><link>http://localhost/</link><description>Test</description>
  <item><guid>a</guid><title>First post</title><link>http://localhost/a</link>
    <description>&lt;p&gt;Hello &lt;b&gt;world&lt;/b&gt;&lt;/p&gt;</description>
    <pubDate>Sat, 17 Oct 2026 12:00:00 GMT</pubDate></item>
  <item><guid>b</guid><title>Second post</title><link>http://localhost/b</link>
    <description>&lt;p&gt;Again&lt;/p&gt;</description>
    <pubDate>Fri, 16 Oct 2026 12:00:00 GMT</pubDate></item>
</channel></rss>"#;

/// Serves `FEED_XML` to every request; returns its URL.
fn feed_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/feed.xml", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut stream = stream;
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/rss+xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                FEED_XML.len(),
                FEED_XML
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });
    url
}

fn harness(paths: Paths) -> Harness<'static, ThreadLinerApp> {
    Harness::builder()
        .with_size(egui::vec2(1400.0, 900.0))
        .with_context_setup(marina_egui::fonts::install)
        .build_ui_state(
            |ui, app: &mut ThreadLinerApp| app.show(ui),
            ThreadLinerApp::new(None, paths),
        )
}

/// Steps the UI until `cond` holds (the backend works on other threads).
fn wait_for(
    h: &mut Harness<'static, ThreadLinerApp>,
    what: &str,
    cond: impl Fn(&ThreadLinerApp) -> bool,
) {
    for _ in 0..750 {
        h.step();
        if cond(h.state()) {
            h.step();
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("timed out waiting for {what}");
}

/// Focuses the `n`th text field and types into it.
fn type_into(h: &mut Harness<'static, ThreadLinerApp>, n: usize, text: &str) {
    // A dialog opened last frame is laid out invisibly first.
    h.run_steps(3);
    h.get_all_by_role(Role::TextInput)
        .nth(n)
        .expect("text field")
        .focus();
    h.step();
    h.get_all_by_role(Role::TextInput)
        .nth(n)
        .expect("text field")
        .type_text(text);
    h.step();
}

fn read_json(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn setup_add_read_mark_and_tag() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = Paths {
        user_data: tmp.path().join("ThreadLiner"),
    };
    let data = tmp.path().join("data");
    let url = feed_server();
    let mut h = harness(paths.clone());

    // First run: the setup dialog.
    wait_for(&mut h, "start-up", |a| a.is_ready());
    assert!(!h.state().is_setup_complete());
    type_into(&mut h, 0, &data.display().to_string());
    h.get_by_label("Get Started").click();
    wait_for(&mut h, "setup", |a| a.is_setup_complete());
    let config = read_json(&paths.config());
    assert_eq!(config["dataDir"], json!(data));
    assert!(data.join(".git").exists(), "the data folder is a git repo");

    // Add a feed from the local server.
    h.get_by_label("Add Feed").click();
    h.step();
    type_into(&mut h, 0, &url);
    h.get_all_by_label("Add Feed").last().unwrap().click();
    wait_for(&mut h, "the feed", |a| {
        a.feeds().len() == 1 && !a.has_overlay()
    });
    assert_eq!(h.state().feeds()[0].title, "Fixture Feed");
    assert_eq!(h.state().feeds()[0].unread_count, 2);

    // Open it and read the first entry: it's marked read.
    h.get_by_label_contains("Fixture Feed").click();
    wait_for(&mut h, "entries", |a| a.entries().len() == 2);
    h.get_by_role_and_label(Role::ListItem, "First post")
        .click();
    wait_for(&mut h, "the unread count", |a| {
        a.feeds()[0].unread_count == 1
    });
    assert!(h.state().selected_entry().is_some());
    let feed_id = h.state().feeds()[0].id.clone();
    let entries = read_json(&data.join("data/feeds").join(&feed_id).join("entries.json"));
    let first = entries["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["guid"] == "a")
        .unwrap();
    assert_eq!(first["isRead"], json!(true));

    // Mark it unread from the toolbar.
    h.get_by_label("Mark as Unread").click();
    wait_for(&mut h, "mark unread", |a| a.feeds()[0].unread_count == 2);

    // Create a tag, then assign it from the sidebar.
    h.get_by_label("Tags").click();
    h.run_steps(3);
    h.get_by_label("Add Tag").click();
    type_into(&mut h, 0, "Tech");
    h.get_by_label("Add").click();
    wait_for(&mut h, "the tag", |a| a.tags().len() == 1);
    h.get_by_label("Close").click();
    wait_for(&mut h, "the dialog to close", |a| !a.has_overlay());
    h.get_by_role_and_label(Role::CheckBox, "Tech").click();
    wait_for(&mut h, "the assignment", |a| {
        a.feeds()[0].tag_ids.len() == 1
    });
    let feeds = read_json(&data.join("data/feeds.json"));
    assert_eq!(feeds["feeds"][0]["tagIds"], json!([h.state().tags()[0].id]));
    assert!(
        h.state().toasts().messages().is_empty(),
        "{:?}",
        h.state().toasts().messages()
    );
}

#[test]
fn settings_are_written_like_the_tauri_build() {
    let tmp = tempfile::tempdir().unwrap();
    let paths = Paths {
        user_data: tmp.path().join("ThreadLiner"),
    };
    let data = tmp.path().join("data");
    let backend = Backend::new(paths.clone());
    backend.setup(data.clone(), None);
    let mut done = Vec::new();
    for _ in 0..500 {
        for e in backend.poll() {
            if let Event::Done { op, result } = e {
                done.push((op, result));
            }
        }
        if done.iter().any(|d| d.0 == Op::Setup) {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(done.iter().any(|d| d == &(Op::Setup, Ok(()))), "{done:?}");

    // The Settings selects store strings, the toggle a boolean.
    backend.set_setting("pollInterval", json!("5"));
    backend.set_setting("pollNotificationsEnabled", json!(false));
    let mut settings = None;
    for _ in 0..500 {
        for e in backend.poll() {
            if let Event::Settings(s) = e {
                settings = Some(s);
            }
        }
        if settings.as_ref().is_some_and(|s| !s.notifications) {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let settings = settings.expect("settings event");
    assert_eq!(settings.poll_interval, "5");
    assert!(!settings.notifications);
    let raw = std::fs::read_to_string(data.join("configuration/settings.json")).unwrap();
    assert_eq!(
        raw,
        "{\n  \"pollInterval\": \"5\",\n  \"pollNotificationsEnabled\": false\n}"
    );
}
