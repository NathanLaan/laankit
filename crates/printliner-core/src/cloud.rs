//! The Bambu cloud's REST API: signing in (with an email code when the
//! account asks for one), the printers bound to the account, task history,
//! and the cover images tasks point to.
//!
//! Parsing is lenient on purpose: numbers may arrive as strings, ids as
//! either, and unknown fields are ignored.

use std::time::Duration;

use chrono::{DateTime, Utc};
use reqwest::StatusCode;
use serde_json::{json, Value};

use crate::region::Region;
use crate::values::{f64_of, i64_of, string_of, time_of};

/// How long a request may take.
const TIMEOUT: Duration = Duration::from_secs(20);

/// Why a request failed.
#[derive(Clone, Debug, PartialEq)]
pub enum CloudError {
    /// 401 or 403: a wrong password or code, or an expired token.
    Unauthorized,
    /// Another 4xx, with the server's message when it sent one.
    Rejected(String),
    /// A 5xx.
    Server(u16),
    /// No answer: DNS, connection, TLS, or timeout.
    Network(String),
    /// An answer PrintLiner couldn't read.
    Parse(String),
}

impl std::fmt::Display for CloudError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CloudError::Unauthorized => write!(f, "Bambu Lab refused the sign-in."),
            CloudError::Rejected(m) => write!(f, "Bambu Lab refused the request: {m}"),
            CloudError::Server(code) => write!(f, "Bambu Lab's servers had a problem ({code})."),
            CloudError::Network(m) => write!(f, "Couldn't reach Bambu Lab: {m}"),
            CloudError::Parse(m) => write!(f, "Unexpected answer from Bambu Lab: {m}"),
        }
    }
}

impl std::error::Error for CloudError {}

/// A signed-in session's token.
#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub access_token: String,
    /// When it stops working, if the server said.
    pub expires_at: Option<DateTime<Utc>>,
}

/// What a sign-in request led to.
#[derive(Clone, Debug, PartialEq)]
pub enum Login {
    Token(Token),
    /// The account wants a code sent by email: call
    /// [`Client::send_email_code`], then [`Client::login_with_code`].
    EmailCode,
    /// The account uses an authenticator app, which PrintLiner doesn't
    /// support yet.
    Authenticator,
}

/// A printer bound to the account.
#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    /// The serial number (`dev_id`), which the MQTT topics use.
    pub serial: String,
    /// The name set in Bambu Studio or Handy.
    pub name: String,
    /// The model code (`dev_model_name`, e.g. `C12`).
    pub model: String,
    /// The product name (`dev_product_name`, e.g. `P1S`).
    pub product: String,
    pub online: bool,
}

impl Device {
    /// The product name, else the model code.
    pub fn model_label(&self) -> &str {
        if self.product.is_empty() {
            &self.model
        } else {
            &self.product
        }
    }
}

/// One entry of a printer's task history.
#[derive(Clone, Debug, PartialEq)]
pub struct Task {
    pub id: String,
    pub title: String,
    /// The plate image's URL.
    pub cover: Option<String>,
    /// As the cloud says it (`"completed"`, `"failed"`, …), lower case.
    pub status: String,
    pub weight_g: Option<f64>,
    pub duration_secs: Option<i64>,
    pub start: Option<DateTime<Utc>>,
    pub end: Option<DateTime<Utc>>,
    pub serial: String,
}

impl Task {
    /// Whether the task finished well, by the cloud's status words.
    pub fn succeeded(&self) -> bool {
        matches!(
            self.status.as_str(),
            "completed" | "finish" | "finished" | "success" | "2"
        )
    }

    pub fn failed(&self) -> bool {
        matches!(self.status.as_str(), "failed" | "fail" | "3")
    }

    pub fn cancelled(&self) -> bool {
        matches!(self.status.as_str(), "cancelled" | "canceled" | "cancel")
    }
}

/// The REST client for one region.
#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    base: String,
}

impl Client {
    /// `user_agent` is the app's, e.g. `PrintLiner/0.1.0`.
    pub fn new(region: Region, user_agent: &str) -> Self {
        Self::with_base(api_base(region), user_agent)
    }

    /// A client for any base URL (tests point it at a mock server).
    pub fn with_base(base: impl Into<String>, user_agent: &str) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(user_agent)
            .timeout(TIMEOUT)
            .build()
            .unwrap_or_default();
        Self {
            http,
            base: base.into().trim_end_matches('/').to_string(),
        }
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    /// Signs in with the email and password.
    pub async fn login(&self, email: &str, password: &str) -> Result<Login, CloudError> {
        let body = json!({ "account": email, "password": password });
        let v = self.post("/v1/user-service/user/login", &body).await?;
        parse_login(&v)
    }

    /// Asks Bambu Lab to email a sign-in code.
    pub async fn send_email_code(&self, email: &str) -> Result<(), CloudError> {
        let body = json!({ "email": email, "type": "codeLogin" });
        self.post("/v1/user-service/user/sendemail/code", &body)
            .await
            .map(|_| ())
    }

    /// Signs in with the emailed code.
    pub async fn login_with_code(&self, email: &str, code: &str) -> Result<Login, CloudError> {
        let body = json!({ "account": email, "code": code.trim() });
        let v = self.post("/v1/user-service/user/login", &body).await?;
        parse_login(&v)
    }

    /// The cloud MQTT username for the token's account: `u_<uid>`.
    pub async fn mqtt_username(&self, token: &str) -> Result<String, CloudError> {
        let v = self
            .get("/v1/design-user-service/my/preference", token)
            .await?;
        v.get("uid")
            .and_then(i64_of)
            .map(|uid| format!("u_{uid}"))
            .ok_or_else(|| CloudError::Parse("no uid in the account preferences".into()))
    }

    /// The printers bound to the account.
    pub async fn devices(&self, token: &str) -> Result<Vec<Device>, CloudError> {
        let v = self.get("/v1/iot-service/api/user/bind", token).await?;
        Ok(parse_devices(&v))
    }

    /// The printer's last `limit` tasks, newest first.
    pub async fn tasks(
        &self,
        token: &str,
        serial: &str,
        limit: u32,
    ) -> Result<Vec<Task>, CloudError> {
        let path = format!("/v1/user-service/my/tasks?deviceId={serial}&limit={limit}");
        let v = self.get(&path, token).await?;
        Ok(parse_tasks(&v))
    }

    /// An image a task points to. Cover URLs are usually signed and need no
    /// token; on a 403 it tries once more with it.
    pub async fn image(&self, url: &str, token: &str) -> Result<Vec<u8>, CloudError> {
        let first = self.http.get(url).send().await.map_err(network)?;
        let resp = if first.status() == StatusCode::FORBIDDEN {
            self.http
                .get(url)
                .bearer_auth(token)
                .send()
                .await
                .map_err(network)?
        } else {
            first
        };
        let status = resp.status();
        if !status.is_success() {
            return Err(status_error(status, &Value::Null));
        }
        resp.bytes().await.map(|b| b.to_vec()).map_err(network)
    }

    async fn post(&self, path: &str, body: &Value) -> Result<Value, CloudError> {
        let resp = self
            .http
            .post(format!("{}{path}", self.base))
            .json(body)
            .send()
            .await
            .map_err(network)?;
        read(resp).await
    }

    async fn get(&self, path: &str, token: &str) -> Result<Value, CloudError> {
        let resp = self
            .http
            .get(format!("{}{path}", self.base))
            .bearer_auth(token)
            .send()
            .await
            .map_err(network)?;
        read(resp).await
    }
}

/// The region's REST base; debug builds take `PRINTLINER_API_BASE` instead,
/// for a local mock server.
fn api_base(region: Region) -> String {
    if cfg!(debug_assertions) {
        if let Ok(base) = std::env::var("PRINTLINER_API_BASE") {
            return base;
        }
    }
    region.api_base().to_string()
}

fn network(e: reqwest::Error) -> CloudError {
    CloudError::Network(e.without_url().to_string())
}

async fn read(resp: reqwest::Response) -> Result<Value, CloudError> {
    let status = resp.status();
    let text = resp.text().await.map_err(network)?;
    let v: Value = if text.trim().is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&text).unwrap_or(Value::Null)
    };
    if status.is_success() {
        Ok(v)
    } else {
        Err(status_error(status, &v))
    }
}

fn status_error(status: StatusCode, body: &Value) -> CloudError {
    match status.as_u16() {
        401 | 403 => CloudError::Unauthorized,
        code @ 500.. => CloudError::Server(code),
        code => {
            let message = ["message", "error", "msg"]
                .iter()
                .find_map(|k| body.get(*k).and_then(string_of))
                .unwrap_or_else(|| format!("HTTP {code}"));
            CloudError::Rejected(message)
        }
    }
}

/// Reads a sign-in answer: a token, or which second step it wants.
pub fn parse_login(v: &Value) -> Result<Login, CloudError> {
    let text = |k: &str| v.get(k).and_then(string_of);
    if let Some(access_token) = text("accessToken") {
        let expires_at = v
            .get("expiresIn")
            .and_then(i64_of)
            .filter(|s| *s > 0)
            .map(|s| Utc::now() + chrono::Duration::seconds(s));
        return Ok(Login::Token(Token {
            access_token,
            expires_at,
        }));
    }
    if text("tfaKey").is_some() {
        return Ok(Login::Authenticator);
    }
    if text("loginType").is_some_and(|t| t.eq_ignore_ascii_case("verifyCode")) {
        return Ok(Login::EmailCode);
    }
    Err(match text("error").or_else(|| text("message")) {
        Some(m) => CloudError::Rejected(m),
        None => CloudError::Parse("the sign-in answer had no token".into()),
    })
}

/// Reads `{devices: [...]}`.
pub fn parse_devices(v: &Value) -> Vec<Device> {
    let Some(list) = v.get("devices").and_then(Value::as_array) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|d| {
            let text = |k: &str| d.get(k).and_then(string_of).unwrap_or_default();
            let serial = text("dev_id");
            if serial.is_empty() {
                return None;
            }
            let name = Some(text("name"))
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| serial.clone());
            Some(Device {
                name,
                model: text("dev_model_name"),
                product: text("dev_product_name"),
                online: d.get("online").is_some_and(|o| match o {
                    Value::Bool(b) => *b,
                    other => i64_of(other).is_some_and(|n| n != 0),
                }),
                serial,
            })
        })
        .collect()
}

/// Reads `{hits: [...]}`.
pub fn parse_tasks(v: &Value) -> Vec<Task> {
    let Some(list) = v.get("hits").and_then(Value::as_array) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|t| {
            let text = |k: &str| t.get(k).and_then(string_of);
            Some(Task {
                id: text("id")?,
                title: text("title").unwrap_or_default(),
                cover: text("cover"),
                status: text("status").unwrap_or_default().to_lowercase(),
                weight_g: t.get("weight").and_then(f64_of),
                duration_secs: t.get("costTime").and_then(i64_of),
                start: t.get("startTime").and_then(time_of),
                end: t.get("endTime").and_then(time_of),
                serial: text("deviceId").unwrap_or_default(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_token() {
        let v = json!({ "accessToken": "abc", "refreshToken": "r", "expiresIn": 7776000, "loginType": "" });
        let Ok(Login::Token(t)) = parse_login(&v) else {
            panic!("expected a token");
        };
        assert_eq!(t.access_token, "abc");
        let days = (t.expires_at.unwrap() - Utc::now()).num_days();
        assert!((89..=90).contains(&days), "{days}");
    }

    #[test]
    fn login_second_steps() {
        let code = json!({ "accessToken": "", "loginType": "verifyCode" });
        assert_eq!(parse_login(&code), Ok(Login::EmailCode));
        let tfa = json!({ "accessToken": "", "loginType": "tfa", "tfaKey": "k1" });
        assert_eq!(parse_login(&tfa), Ok(Login::Authenticator));
        let bad = json!({ "error": "Incorrect password" });
        assert_eq!(
            parse_login(&bad),
            Err(CloudError::Rejected("Incorrect password".into()))
        );
    }

    #[test]
    fn devices() {
        let v: Value = serde_json::from_str(include_str!("../tests/fixtures/bind.json")).unwrap();
        let d = parse_devices(&v);
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].serial, "00M00A000000001");
        assert_eq!(d[0].model_label(), "X1 Carbon");
        assert!(d[0].online);
        // A missing name falls back to the serial; a 0 means offline.
        assert_eq!(d[1].name, "01P00A000000002");
        assert!(!d[1].online);
    }

    #[test]
    fn tasks() {
        let v: Value = serde_json::from_str(include_str!("../tests/fixtures/tasks.json")).unwrap();
        let t = parse_tasks(&v);
        assert_eq!(t.len(), 3);
        // A number id, a string status, RFC 3339 times.
        assert_eq!(t[0].id, "123456789");
        assert!(t[0].succeeded());
        assert_eq!(t[0].weight_g, Some(12.5));
        assert_eq!(t[0].duration_secs, Some(3600));
        assert!(t[0].start.is_some() && t[0].end.is_some());
        // A string id, epoch-millisecond times, a failure.
        assert_eq!(t[1].id, "abc-2");
        assert!(t[1].failed());
        assert!(t[1].start.is_some());
        // An entry with no id is skipped.
        assert_eq!(t[2].title, "No times");
        assert_eq!(t[2].start, None);
    }
}
