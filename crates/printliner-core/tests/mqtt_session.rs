//! The MQTT session against an in-process broker (plain TCP, with a
//! username and password): it subscribes, sends `pushall`, and passes
//! reports on; and it stops for good when the credentials are refused.

use std::net::TcpListener;
use std::time::Duration;

use printliner_core::mqtt::{self, Config, MqttEvent};
use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS};
use serde_json::json;
use tokio::sync::{mpsc, watch};

const SERIAL: &str = "00M00A000000001";

/// Starts a broker on a free port that accepts `u_1` / `token`.
fn broker() -> u16 {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let config: rumqttd::Config = serde_json::from_value(json!({
        "id": 0,
        "router": {
            "max_connections": 10,
            "max_outgoing_packet_count": 200,
            "max_segment_size": 104857600,
            "max_segment_count": 10
        },
        "v4": { "1": {
            "name": "v4-1",
            "listen": format!("127.0.0.1:{port}"),
            "next_connection_delay_ms": 1,
            "connections": {
                "connection_timeout_ms": 5000,
                "max_payload_size": 1048576,
                "max_inflight_count": 100,
                "auth": { "u_1": "token", "tester": "pw" }
            }
        }}
    }))
    .unwrap();
    std::thread::spawn(move || {
        let _ = rumqttd::Broker::new(config).start();
    });
    // Wait for the listener.
    for _ in 0..100 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    port
}

fn config(port: u16, password: &str) -> Config {
    Config {
        host: "127.0.0.1".into(),
        port,
        username: "u_1".into(),
        password: password.into(),
        client_id: format!("printliner-test-{password}"),
        tls: false,
    }
}

async fn next(rx: &mut mpsc::UnboundedReceiver<MqttEvent>) -> MqttEvent {
    tokio::time::timeout(Duration::from_secs(10), rx.recv())
        .await
        .expect("an event in time")
        .expect("the session is still running")
}

#[tokio::test(flavor = "multi_thread")]
async fn subscribes_sends_pushall_and_passes_reports_on() {
    let port = broker();

    // A stand-in for the printer: hears the request topic, sends reports.
    let mut options = MqttOptions::new("printer", "127.0.0.1", port);
    options.set_credentials("tester", "pw");
    let (printer, mut printer_loop) = AsyncClient::new(options, 16);
    printer
        .subscribe(mqtt::request_topic(SERIAL), QoS::AtMostOnce)
        .await
        .unwrap();
    let (req_tx, mut req_rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Ok(ev) = printer_loop.poll().await {
            if let Event::Incoming(Packet::Publish(p)) = ev {
                let _ = req_tx.send(String::from_utf8_lossy(&p.payload).to_string());
            }
        }
    });
    tokio::time::sleep(Duration::from_millis(200)).await;

    let (tx, mut rx) = mpsc::unbounded_channel();
    let (stop_tx, stop_rx) = watch::channel(false);
    let session = tokio::spawn(mqtt::run(
        config(port, "token"),
        vec![SERIAL.into()],
        tx,
        stop_rx,
    ));
    assert_eq!(next(&mut rx).await, MqttEvent::Connected);

    let request = tokio::time::timeout(Duration::from_secs(10), req_rx.recv())
        .await
        .unwrap()
        .unwrap();
    let request: serde_json::Value = serde_json::from_str(&request).unwrap();
    assert_eq!(request["pushing"]["command"], "pushall");

    let report = json!({ "print": { "gcode_state": "RUNNING", "mc_percent": 5 } });
    printer
        .publish(
            mqtt::report_topic(SERIAL),
            QoS::AtMostOnce,
            false,
            report.to_string(),
        )
        .await
        .unwrap();
    assert_eq!(
        next(&mut rx).await,
        MqttEvent::Report {
            serial: SERIAL.into(),
            payload: report
        }
    );

    stop_tx.send(true).unwrap();
    tokio::time::timeout(Duration::from_secs(5), session)
        .await
        .expect("the session stops")
        .unwrap();
}

/// A broker that answers every CONNECT with "not authorised" (CONNACK
/// return code 5), as Bambu's broker does for an expired token. (rumqttd
/// drops the connection instead, so it can't stand in here.)
async fn refusing_broker() -> u16 {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut sock, _)) = listener.accept().await {
            let mut buf = [0u8; 512];
            let _ = sock.read(&mut buf).await;
            let _ = sock.write_all(&[0x20, 0x02, 0x00, 0x05]).await;
        }
    });
    port
}

#[tokio::test(flavor = "multi_thread")]
async fn a_refused_token_ends_the_session() {
    let port = refusing_broker().await;
    let (tx, mut rx) = mpsc::unbounded_channel();
    let (_stop_tx, stop_rx) = watch::channel(false);
    let session = tokio::spawn(mqtt::run(
        config(port, "expired"),
        vec![SERIAL.into()],
        tx,
        stop_rx,
    ));
    assert_eq!(next(&mut rx).await, MqttEvent::AuthFailed);
    tokio::time::timeout(Duration::from_secs(5), session)
        .await
        .expect("the session ends rather than retrying")
        .unwrap();
}
