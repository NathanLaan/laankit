//! The cloud MQTT session: one connection for the account, subscribed to
//! every printer's `device/<serial>/report` topic. On every connect it asks
//! each printer for a full report (`pushall`), since P1 and A1 printers
//! otherwise send only changes.
//!
//! [`run`] reconnects with backoff (5 s, doubling to 60 s) until it's
//! stopped, or until the broker refuses the credentials, which it reports
//! as [`MqttEvent::AuthFailed`] and then returns: a new token is needed.

use std::sync::Arc;
use std::time::Duration;

use rumqttc::{
    AsyncClient, ConnectReturnCode, ConnectionError, Event, MqttOptions, Packet, QoS,
    TlsConfiguration, Transport,
};
use serde_json::Value;
use tokio::sync::{mpsc, watch};

use crate::region::{Region, MQTT_PORT};
use crate::status::pushall;

const KEEP_ALIVE: Duration = Duration::from_secs(60);
const BACKOFF_MIN: Duration = Duration::from_secs(5);
const BACKOFF_MAX: Duration = Duration::from_secs(60);
/// Full reports with AMS data run to tens of kilobytes.
const MAX_INCOMING: usize = 1024 * 1024;

/// Where and how to connect.
#[derive(Clone, Debug)]
pub struct Config {
    pub host: String,
    pub port: u16,
    /// `u_<uid>`.
    pub username: String,
    /// The access token.
    pub password: String,
    pub client_id: String,
    /// TLS with Mozilla's roots. Off only for a local test broker.
    pub tls: bool,
}

impl Config {
    /// The region's broker. Debug builds take `PRINTLINER_MQTT_HOST`
    /// (`host:port`, plain TCP) instead, for a local test broker.
    pub fn for_region(region: Region, username: String, token: String) -> Config {
        let client_id = format!("printliner-{}", uuid::Uuid::new_v4().simple());
        if cfg!(debug_assertions) {
            if let Ok(over) = std::env::var("PRINTLINER_MQTT_HOST") {
                let (host, port) = over
                    .rsplit_once(':')
                    .and_then(|(h, p)| Some((h.to_string(), p.parse().ok()?)))
                    .unwrap_or((over.clone(), 1883));
                return Config {
                    host,
                    port,
                    username,
                    password: token,
                    client_id,
                    tls: false,
                };
            }
        }
        Config {
            host: region.mqtt_host().to_string(),
            port: MQTT_PORT,
            username,
            password: token,
            client_id,
            tls: true,
        }
    }
}

/// What the session reports.
#[derive(Clone, Debug, PartialEq)]
pub enum MqttEvent {
    /// Connected and subscribed; `pushall` sent to every printer.
    Connected,
    /// A report from a printer, as sent (merge it into the last state).
    Report { serial: String, payload: Value },
    /// The connection dropped or couldn't be made; it retries.
    Disconnected(String),
    /// The broker refused the username or token. The session has ended.
    AuthFailed,
}

/// Runs the session until `stop` turns true or the credentials are refused.
pub async fn run(
    config: Config,
    serials: Vec<String>,
    events: mpsc::UnboundedSender<MqttEvent>,
    mut stop: watch::Receiver<bool>,
) {
    let mut options = MqttOptions::new(&config.client_id, &config.host, config.port);
    options
        .set_credentials(&config.username, &config.password)
        .set_keep_alive(KEEP_ALIVE)
        .set_max_packet_size(MAX_INCOMING, 64 * 1024)
        .set_clean_session(true);
    if config.tls {
        options.set_transport(Transport::Tls(tls_config()));
    }
    let (client, mut eventloop) = AsyncClient::new(options, 64);
    let mut sequence: u64 = 0;
    let mut backoff = BACKOFF_MIN;
    loop {
        if *stop.borrow() {
            break;
        }
        let polled = tokio::select! {
            ev = eventloop.poll() => ev,
            _ = stop.changed() => break,
        };
        match polled {
            Ok(Event::Incoming(Packet::ConnAck(ack))) if ack.code == ConnectReturnCode::Success => {
                backoff = BACKOFF_MIN;
                for serial in &serials {
                    let _ = client
                        .subscribe(report_topic(serial), QoS::AtMostOnce)
                        .await;
                    sequence += 1;
                    let _ = client
                        .publish(
                            request_topic(serial),
                            QoS::AtMostOnce,
                            false,
                            pushall(sequence),
                        )
                        .await;
                }
                let _ = events.send(MqttEvent::Connected);
            }
            Ok(Event::Incoming(Packet::Publish(p))) => {
                let Some(serial) = serial_of(&p.topic) else {
                    continue;
                };
                match serde_json::from_slice::<Value>(&p.payload) {
                    Ok(payload) => {
                        let _ = events.send(MqttEvent::Report {
                            serial: serial.to_string(),
                            payload,
                        });
                    }
                    Err(e) => log::debug!("unreadable report from {serial}: {e}"),
                }
            }
            Ok(_) => {}
            Err(ConnectionError::ConnectionRefused(
                ConnectReturnCode::BadUserNamePassword | ConnectReturnCode::NotAuthorized,
            )) => {
                let _ = events.send(MqttEvent::AuthFailed);
                return;
            }
            Err(e) => {
                let _ = events.send(MqttEvent::Disconnected(e.to_string()));
                tokio::select! {
                    _ = tokio::time::sleep(backoff) => {}
                    _ = stop.changed() => break,
                }
                backoff = (backoff * 2).min(BACKOFF_MAX);
            }
        }
    }
    let _ = client.try_disconnect();
}

pub fn report_topic(serial: &str) -> String {
    format!("device/{serial}/report")
}

pub fn request_topic(serial: &str) -> String {
    format!("device/{serial}/request")
}

/// The serial in `device/<serial>/report`.
fn serial_of(topic: &str) -> Option<&str> {
    topic
        .strip_prefix("device/")?
        .strip_suffix("/report")
        .filter(|s| !s.is_empty() && !s.contains('/'))
}

/// rustls on the ring provider with Mozilla's roots: the same trust for
/// every platform, with no dependence on the OS store.
fn tls_config() -> TlsConfiguration {
    let roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("ring supports the default TLS versions")
        .with_root_certificates(roots)
        .with_no_client_auth();
    TlsConfiguration::Rustls(Arc::new(config))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topics() {
        assert_eq!(report_topic("ABC"), "device/ABC/report");
        assert_eq!(request_topic("ABC"), "device/ABC/request");
        assert_eq!(serial_of("device/ABC/report"), Some("ABC"));
        assert_eq!(serial_of("device/ABC/request"), None);
        assert_eq!(serial_of("device//report"), None);
        assert_eq!(serial_of("device/a/b/report"), None);
    }

    #[test]
    fn tls_config_builds() {
        assert!(matches!(tls_config(), TlsConfiguration::Rustls(_)));
    }

    #[test]
    fn region_config() {
        let c = Config::for_region(Region::Europe, "u_1".into(), "t".into());
        if std::env::var("PRINTLINER_MQTT_HOST").is_err() {
            assert_eq!(c.host, "eu.mqtt.bambulab.com");
            assert_eq!(c.port, 8883);
            assert!(c.tls);
        }
        assert!(c.client_id.starts_with("printliner-"));
    }
}
