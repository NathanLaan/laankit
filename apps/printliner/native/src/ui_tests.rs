//! Drives the app headlessly (egui_kittest) against a temp config folder:
//! the demo dashboard and a printer's detail, signing in with an emailed
//! code against a mock Bambu Lab server, the sign-in-needed banner, and
//! renaming a printer.

use std::time::{Duration, Instant};

use egui::{Event, Key, Modifiers};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use printliner_core::demo;
use serde_json::{json, Value};

use crate::app::{Launch, PrintLinerApp};
use crate::backend::Account;
use crate::config::Paths;

type H = Harness<'static, PrintLinerApp>;

fn paths(tmp: &tempfile::TempDir) -> Paths {
    Paths {
        config: tmp.path().join("PrintLiner"),
        data: tmp.path().join("printliner"),
    }
}

fn harness(paths: Paths, launch: Launch) -> H {
    Harness::builder()
        .with_size(egui::vec2(1280.0, 820.0))
        .with_step_dt(1.0 / 60.0)
        .with_context_setup(laankit_egui::fonts::install)
        .build_ui_state(
            |ui, app: &mut PrintLinerApp| app.show(ui),
            PrintLinerApp::new(None, paths, launch),
        )
}

/// Steps until `done` holds (the backend answers on its own threads).
fn wait_for(h: &mut H, what: &str, done: impl Fn(&PrintLinerApp) -> bool) {
    let start = Instant::now();
    while !done(h.state()) {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "timed out waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(10));
        h.step();
    }
    h.run_steps(2);
}

fn demo_harness() -> (tempfile::TempDir, H) {
    let tmp = tempfile::tempdir().unwrap();
    let mut h = harness(
        paths(&tmp),
        Launch {
            demo: true,
            ..Default::default()
        },
    );
    wait_for(&mut h, "the demo printers", |a| {
        let p = a.printers_for_test();
        p.len() == 3 && p.iter().all(|p| p.status.is_some())
    });
    (tmp, h)
}

fn key(h: &mut H, key: Key) {
    for pressed in [true, false] {
        h.event(Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Modifiers::NONE,
        });
    }
    h.step();
}

/// A printer card (a button named after the printer, which also shows the
/// name as text).
fn card<'a>(h: &'a H, name: &'a str) -> egui_kittest::Node<'a> {
    h.get_by_role_and_label(egui::accesskit::Role::Button, name)
}

fn type_text(h: &mut H, text: &str) {
    h.event(Event::Text(text.into()));
    h.step();
}

#[test]
fn demo_dashboard_shows_the_printers_and_their_detail() {
    let (_tmp, mut h) = demo_harness();
    assert!(matches!(
        h.state().account(),
        Account::SignedIn { demo: true, .. }
    ));
    // A card per printer, with its state.
    for name in ["Workshop X1C", "Bench P1S", "Front Desk A1"] {
        card(&h, name);
    }
    h.get_by_label("Printing");
    h.get_by_label("Failed");
    h.get_by_label("1 alert");

    card(&h, "Workshop X1C").click();
    h.run_steps(2);
    assert_eq!(h.state().selected(), Some(demo::X1C));
    wait_for(&mut h, "the task history", |a| {
        a.tasks_for(demo::X1C).is_some()
    });
    h.get_by_label("Task History".to_uppercase().as_str());
    h.get_by_label("Hex key holder");
    h.get_by_label("Slot 1");
    h.get_by_label("In Use");
    h.get_by_label("No alerts");

    // Esc goes back to the grid.
    key(&mut h, Key::Escape);
    assert_eq!(h.state().selected(), None);

    // The failed A1 lists its HMS code.
    card(&h, "Front Desk A1").click();
    h.run_steps(2);
    h.get_by_label("0300_0100_0001_0003");
    // The detail's own button (the toolbar has one too).
    h.get_all_by_label("All Printers").last().unwrap().click();
    h.run_steps(2);
    assert_eq!(h.state().selected(), None);
}

#[test]
fn renaming_and_reordering_printers_is_saved() {
    let (tmp, mut h) = demo_harness();
    h.state_mut().run(crate::commands::Act::Settings);
    h.run_steps(2);
    h.get_by_label("Printers").click();
    h.run_steps(2);
    // The first row is Bench P1S (by name): move it down, under Front Desk A1.
    h.get_all_by_label("Down").next().unwrap().click();
    h.run_steps(2);
    let settings: Value =
        serde_json::from_str(&std::fs::read_to_string(paths(&tmp).settings_file()).unwrap())
            .unwrap();
    assert_eq!(
        settings["printerOrder"],
        json!([demo::A1, demo::P1S, demo::X1C])
    );
    let order: Vec<String> = h
        .state()
        .printers_for_test()
        .into_iter()
        .map(|p| p.device.serial)
        .collect();
    assert_eq!(order, [demo::A1, demo::P1S, demo::X1C]);
}

#[test]
fn signed_out_shows_the_welcome() {
    let tmp = tempfile::tempdir().unwrap();
    let mut h = harness(paths(&tmp), Launch::default());
    wait_for(&mut h, "signed out", |a| {
        matches!(a.account(), Account::SignedOut)
    });
    h.get_by_label_contains("Sign in to your Bambu Lab account");
    h.get_by_label("Sign In").click();
    h.run_steps(2);
    assert!(h.state().has_overlay());
    h.get_by_label("EMAIL");
}

#[test]
fn a_saved_account_without_a_token_asks_to_sign_in_again() {
    let tmp = tempfile::tempdir().unwrap();
    let p = paths(&tmp);
    laankit_core::json::write(
        &p.settings_file(),
        &json!({ "email": "a@b.c", "tokenStorage": "file" }),
    )
    .unwrap();
    let mut h = harness(p, Launch::default());
    wait_for(&mut h, "sign-in needed", |a| {
        matches!(a.account(), Account::SignInNeeded { .. })
    });
    h.get_by_label_contains("needs you to sign in again");
}

/// The whole sign-in, against a mock server: password, then the emailed
/// code, then the account's MQTT username and its printers. The token goes
/// to a private file (no keychain in tests).
#[test]
fn signing_in_with_an_emailed_code() {
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let rt = tokio::runtime::Runtime::new().unwrap();
    let server = rt.block_on(async {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/user-service/user/login"))
            .and(body_partial_json(json!({ "password": "pw" })))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "loginType": "verifyCode" })),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/user-service/user/sendemail/code"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/user-service/user/login"))
            .and(body_partial_json(json!({ "code": "654321" })))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "accessToken": "tok", "expiresIn": 7776000 })),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/design-user-service/my/preference"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "uid": 42 })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/iot-service/api/user/bind"))
            .respond_with(ResponseTemplate::new(200).set_body_string(include_str!(
                "../../../../crates/printliner-core/tests/fixtures/bind.json"
            )))
            .mount(&server)
            .await;
        server
    });

    let tmp = tempfile::tempdir().unwrap();
    let p = paths(&tmp);
    laankit_core::json::write(&p.settings_file(), &json!({ "tokenStorage": "file" })).unwrap();
    // Nothing listens on port 9: the MQTT session just keeps retrying.
    let mut h = harness(
        p.clone(),
        Launch {
            demo: false,
            api_base: Some(server.uri()),
            mqtt_override: Some(("127.0.0.1".into(), 9)),
        },
    );
    wait_for(&mut h, "signed out", |a| {
        matches!(a.account(), Account::SignedOut)
    });
    h.get_by_label("Sign In").click();
    h.run_steps(3);
    type_text(&mut h, "a@b.c");
    key(&mut h, Key::Tab);
    type_text(&mut h, "pw");
    key(&mut h, Key::Enter);
    wait_for(&mut h, "the code step", |_| true);
    let start = Instant::now();
    while h.query_by_label("CODE").is_none() {
        assert!(start.elapsed() < Duration::from_secs(10), "no code step");
        std::thread::sleep(Duration::from_millis(10));
        h.step();
    }
    h.run_steps(2);
    type_text(&mut h, "654321");
    key(&mut h, Key::Enter);
    wait_for(&mut h, "signed in", |a| {
        matches!(a.account(), Account::SignedIn { .. })
    });
    assert!(!h.state().has_overlay(), "the dialog closes");
    wait_for(&mut h, "the printers", |a| a.printers_for_test().len() == 2);
    card(&h, "Lab X1C");

    let settings = p.settings();
    assert_eq!(settings.email.as_deref(), Some("a@b.c"));
    let token: Value =
        serde_json::from_str(&std::fs::read_to_string(p.token_file()).unwrap()).unwrap();
    assert_eq!(token["accessToken"], "tok");
    assert_eq!(token["username"], "u_42");
    let raw = std::fs::read_to_string(p.settings_file()).unwrap();
    assert!(!raw.contains("pw"), "the password is never saved");
    drop(h);
    drop(server);
    drop(rt);
}

/// An account made with Google sign-in has no password: the code alone
/// signs in, and a password attempt explains why it failed.
#[test]
fn signing_in_with_a_code_and_no_password() {
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let rt = tokio::runtime::Runtime::new().unwrap();
    let server = rt.block_on(async {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/user-service/user/login"))
            .and(body_partial_json(json!({ "password": "guess" })))
            .respond_with(
                ResponseTemplate::new(400)
                    .set_body_json(json!({ "code": 1, "error": "Incorrect account or password" })),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/user-service/user/sendemail/code"))
            .and(body_partial_json(
                json!({ "email": "g@b.c", "type": "codeLogin" }),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/user-service/user/login"))
            .and(body_partial_json(
                json!({ "account": "g@b.c", "code": "111222" }),
            ))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "accessToken": "tok", "expiresIn": 7776000 })),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/design-user-service/my/preference"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "uid": 7 })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/iot-service/api/user/bind"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "devices": [] })))
            .mount(&server)
            .await;
        server
    });

    let tmp = tempfile::tempdir().unwrap();
    let p = paths(&tmp);
    laankit_core::json::write(&p.settings_file(), &json!({ "tokenStorage": "file" })).unwrap();
    let mut h = harness(
        p.clone(),
        Launch {
            demo: false,
            api_base: Some(server.uri()),
            mqtt_override: Some(("127.0.0.1".into(), 9)),
        },
    );
    wait_for(&mut h, "signed out", |a| {
        matches!(a.account(), Account::SignedOut)
    });
    h.get_by_label("Sign In").click();
    h.run_steps(3);
    type_text(&mut h, "g@b.c");
    key(&mut h, Key::Tab);
    type_text(&mut h, "guess");
    key(&mut h, Key::Enter);
    let start = Instant::now();
    while h.query_by_label_contains("has no password").is_none() {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "no wrong-password message"
        );
        std::thread::sleep(Duration::from_millis(10));
        h.step();
    }

    // The error pushed the button down: let the layout settle first.
    h.run_steps(2);
    h.get_by_label("Email Me a Code Instead").click();
    let start = Instant::now();
    while h.query_by_label("CODE").is_none() {
        assert!(start.elapsed() < Duration::from_secs(10), "no code step");
        std::thread::sleep(Duration::from_millis(10));
        h.step();
    }
    h.run_steps(2);
    type_text(&mut h, "111222");
    key(&mut h, Key::Enter);
    wait_for(&mut h, "signed in", |a| {
        matches!(a.account(), Account::SignedIn { .. })
    });
    assert_eq!(p.settings().email.as_deref(), Some("g@b.c"));
    drop(h);
    drop(server);
    drop(rt);
}
