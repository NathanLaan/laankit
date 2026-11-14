//! Everything that touches the network, the keychain, or the disk. One task
//! on a tokio runtime owns the session: the UI sends [`Request`]s, and the
//! task sends back [`Event`]s, which the UI drains once per frame
//! ([`Backend::poll`]). Each event wakes the UI (`request_repaint`), so an
//! idle dashboard doesn't redraw on a timer.
//!
//! The session:
//!
//! - Signs in (with the email code step when the account asks for one),
//!   keeps the token in the keychain (or a private file, when agreed), and
//!   marks the account "sign-in needed" when Bambu Lab refuses the token.
//! - Lists the printers every 5 minutes, and runs one cloud MQTT session
//!   for all of them.
//! - Merges each printer's reports into a full state and sends its status
//!   at most once a second, and at once when its state changes.
//! - Samples each printer every 60 seconds for the trends view.
//! - Loads task history on connect and when a job ends, and fetches cover
//!   images into a disk cache.
//!
//! In demo mode the same events come from `printliner_core::demo`, with no
//! network at all.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use printliner_core::history::{History, Sample, KEEP_DAYS};
use printliner_core::mqtt::{self, MqttEvent};
use printliner_core::secrets::{StoredToken, TokenStore};
use printliner_core::status::merge_json;
use printliner_core::{demo, Client, CloudError, Device, Login, PrinterStatus, Region, Task};
use serde_json::Value;
use tokio::runtime::Runtime;
use tokio::sync::{mpsc, watch};

/// How often the printer list is fetched again.
const DEVICE_REFRESH: Duration = Duration::from_secs(300);
/// The most often a printer's status reaches the UI, state changes aside.
const STATUS_INTERVAL: Duration = Duration::from_secs(1);
/// How often each printer is sampled for the trends view.
const SAMPLE_INTERVAL: Duration = Duration::from_secs(60);
/// What a refused password sign-in says.
pub const WRONG_PASSWORD: &str = "Bambu Lab says the email or password is wrong. Check the region too: a China account signs in under China. If you sign in to Bambu Lab with Google or Apple, your account has no password: use Email Me a Code instead.";
/// Tasks loaded on connect, and when a job ends.
const TASKS_ON_CONNECT: u32 = 50;
const TASKS_ON_FINISH: u32 = 5;

/// What the backend needs to start.
#[derive(Clone, Debug)]
pub struct Config {
    pub region: Region,
    pub email: Option<String>,
    /// Where the token goes when it's kept in a file…
    pub token_path: PathBuf,
    /// …and whether it is (instead of the keychain).
    pub token_in_file: bool,
    pub history_dir: PathBuf,
    pub covers_dir: PathBuf,
    pub user_agent: String,
    pub demo: bool,
    /// Tests: a mock REST server instead of the region's.
    pub api_base: Option<String>,
    /// Tests: a plain-TCP MQTT broker (`host`, `port`) instead of the region's.
    pub mqtt_override: Option<(String, u16)>,
}

/// Something for the backend to do.
#[derive(Clone, Debug)]
pub enum Request {
    SignIn {
        region: Region,
        email: String,
        password: String,
    },
    /// Sign in with an emailed code alone, no password (also for accounts
    /// made with Google or Apple sign-in, which have none).
    EmailCodeOnly {
        region: Region,
        email: String,
    },
    SubmitCode(String),
    ResendCode,
    SignOut,
    /// The answer to [`Event::KeychainUnavailable`]: `true` keeps the token
    /// in a private file; `false` keeps it in memory for this run only.
    StoreTokenInFile(bool),
    Refresh,
    Tasks(String),
    Cover {
        task_id: String,
        url: String,
    },
    History {
        serial: String,
        since: DateTime<Utc>,
    },
}

/// The account, as the UI shows it.
#[derive(Clone, Debug, PartialEq)]
pub enum Account {
    /// Still reading the stored token.
    Starting,
    SignedOut,
    SignedIn {
        email: String,
        expires_at: Option<DateTime<Utc>>,
        /// The token is in a file (not the keychain); `None` while it's
        /// only in memory.
        token_in_file: Option<bool>,
        demo: bool,
    },
    /// Bambu Lab refused the token: sign in again.
    SignInNeeded {
        email: String,
    },
}

/// The sign-in dialog's progress.
#[derive(Clone, Debug, PartialEq)]
pub enum SignInStep {
    /// A code was emailed to this address.
    EmailCode,
    /// The account uses an authenticator app, which isn't supported yet.
    Authenticator,
    Failed(String),
    Done {
        region: Region,
        email: String,
    },
}

/// The MQTT connection.
#[derive(Clone, Debug, PartialEq)]
pub enum Connection {
    Idle,
    Connecting,
    Connected,
    /// Dropped or not made; it retries.
    Lost(String),
}

#[derive(Debug)]
pub enum Event {
    Account(Account),
    SignIn(SignInStep),
    /// The token couldn't be saved to the keychain: ask whether to use a file.
    KeychainUnavailable,
    Devices(Vec<Device>),
    Connection(Connection),
    Status {
        serial: String,
        status: Box<PrinterStatus>,
        /// When the printer last reported.
        at: DateTime<Utc>,
    },
    Tasks {
        serial: String,
        tasks: Vec<Task>,
    },
    Cover {
        task_id: String,
        bytes: Arc<[u8]>,
    },
    History {
        serial: String,
        samples: Vec<Sample>,
    },
    Error(String),
}

struct Shared {
    ctx: Mutex<Option<egui::Context>>,
    events: Mutex<Sender<Event>>,
}

impl Shared {
    fn send(&self, event: Event) {
        if let Ok(tx) = self.events.lock() {
            let _ = tx.send(event);
        }
        if let Ok(ctx) = self.ctx.lock() {
            if let Some(ctx) = ctx.as_ref() {
                ctx.request_repaint();
            }
        }
    }
}

pub struct Backend {
    shared: Arc<Shared>,
    requests: mpsc::UnboundedSender<Request>,
    events: Receiver<Event>,
    _runtime: Runtime,
}

impl Backend {
    pub fn new(config: Config) -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("printliner-backend")
            .build()
            .expect("tokio runtime");
        let (tx, events) = channel();
        let shared = Arc::new(Shared {
            ctx: Mutex::new(None),
            events: Mutex::new(tx),
        });
        let (requests, rx) = mpsc::unbounded_channel();
        let s = shared.clone();
        if config.demo {
            runtime.spawn(run_demo(s, rx));
        } else {
            runtime.spawn(async move { Session::new(config, s).run(rx).await });
        }
        Self {
            shared,
            requests,
            events,
            _runtime: runtime,
        }
    }

    /// Lets events wake the UI.
    pub fn attach(&self, ctx: &egui::Context) {
        if let Ok(mut c) = self.shared.ctx.lock() {
            *c = Some(ctx.clone());
        }
    }

    pub fn send(&self, request: Request) {
        let _ = self.requests.send(request);
    }

    /// The events since the last call.
    pub fn poll(&self) -> Vec<Event> {
        self.events.try_iter().collect()
    }
}

/// A printer's live state inside the session.
#[derive(Default)]
struct Live {
    state: Value,
    status: PrinterStatus,
    at: Option<DateTime<Utc>>,
    sent: Option<Instant>,
    /// Changed since it was last sent.
    dirty: bool,
}

/// The token and who it's for.
struct Auth {
    email: String,
    token: StoredToken,
}

struct Session {
    config: Config,
    shared: Arc<Shared>,
    client: Client,
    store: TokenStore,
    auth: Option<Auth>,
    /// Waiting for an emailed code for this address.
    pending_code: Option<(Region, String)>,
    /// A token the keychain refused, waiting for the file question.
    unsaved: Option<Auth>,
    devices: Vec<Device>,
    live: HashMap<String, Live>,
    tasks: HashMap<String, Vec<Task>>,
    history: History,
    mqtt: Option<watch::Sender<bool>>,
    mqtt_events: mpsc::UnboundedReceiver<MqttEvent>,
    mqtt_tx: mpsc::UnboundedSender<MqttEvent>,
}

impl Session {
    fn new(config: Config, shared: Arc<Shared>) -> Self {
        let client = make_client(&config, config.region);
        let store = if config.token_in_file {
            TokenStore::File(config.token_path.clone())
        } else {
            TokenStore::Keychain
        };
        let history = History::new(config.history_dir.clone());
        let (mqtt_tx, mqtt_events) = mpsc::unbounded_channel();
        Self {
            config,
            shared,
            client,
            store,
            auth: None,
            pending_code: None,
            unsaved: None,
            devices: Vec::new(),
            live: HashMap::new(),
            tasks: HashMap::new(),
            history,
            mqtt: None,
            mqtt_events,
            mqtt_tx,
        }
    }

    async fn run(mut self, mut requests: mpsc::UnboundedReceiver<Request>) {
        self.start().await;
        let mut tick = tokio::time::interval(STATUS_INTERVAL);
        let mut refresh =
            tokio::time::interval_at(tokio::time::Instant::now() + DEVICE_REFRESH, DEVICE_REFRESH);
        let mut sample = tokio::time::interval_at(
            tokio::time::Instant::now() + SAMPLE_INTERVAL,
            SAMPLE_INTERVAL,
        );
        loop {
            tokio::select! {
                req = requests.recv() => match req {
                    Some(r) => self.handle(r).await,
                    None => break,
                },
                Some(ev) = self.mqtt_events.recv() => self.on_mqtt(ev).await,
                _ = tick.tick() => self.flush(false),
                _ = refresh.tick() => {
                    if self.auth.is_some() {
                        self.load_devices().await;
                    }
                }
                _ = sample.tick() => self.sample(),
            }
        }
        self.stop_mqtt();
    }

    fn send(&self, event: Event) {
        self.shared.send(event);
    }

    /// Reads the stored token, if there's an account.
    async fn start(&mut self) {
        let history = self.history.clone();
        tokio::task::spawn_blocking(move || {
            if let Err(e) = history.prune(Utc::now(), KEEP_DAYS) {
                log::warn!("pruning the history: {e}");
            }
        });
        let Some(email) = self.config.email.clone() else {
            self.send(Event::Account(Account::SignedOut));
            return;
        };
        let store = self.store.clone();
        let e = email.clone();
        let loaded = tokio::task::spawn_blocking(move || store.load(&e))
            .await
            .unwrap_or_else(|e| Err(anyhow::anyhow!("{e}")));
        match loaded {
            Ok(Some(token)) if !expired(&token) => {
                self.auth = Some(Auth { email, token });
                self.signed_in(Some(matches!(self.store, TokenStore::File(_))));
                self.load_devices().await;
            }
            Ok(_) => self.send(Event::Account(Account::SignInNeeded { email })),
            Err(e) => {
                self.send(Event::Error(format!(
                    "Couldn't read the saved sign-in: {e:#}"
                )));
                self.send(Event::Account(Account::SignInNeeded { email }));
            }
        }
    }

    fn signed_in(&self, token_in_file: Option<bool>) {
        if let Some(auth) = &self.auth {
            self.send(Event::Account(Account::SignedIn {
                email: auth.email.clone(),
                expires_at: auth.token.expires_at,
                token_in_file,
                demo: false,
            }));
        }
    }

    async fn handle(&mut self, request: Request) {
        match request {
            Request::SignIn {
                region,
                email,
                password,
            } => {
                self.client = make_client(&self.config, region);
                self.config.region = region;
                let email = email.trim().to_string();
                let result = self.client.login(&email, &password).await;
                self.after_login(region, email, result, false).await;
            }
            Request::EmailCodeOnly { region, email } => {
                self.client = make_client(&self.config, region);
                self.config.region = region;
                let email = email.trim().to_string();
                match self.client.send_email_code(&email).await {
                    Ok(()) => {
                        self.pending_code = Some((region, email));
                        self.send(Event::SignIn(SignInStep::EmailCode));
                    }
                    Err(e) => self.send(Event::SignIn(SignInStep::Failed(e.to_string()))),
                }
            }
            Request::SubmitCode(code) => {
                let Some((region, email)) = self.pending_code.clone() else {
                    return;
                };
                let result = self.client.login_with_code(&email, &code).await;
                self.after_login(region, email, result, true).await;
            }
            Request::ResendCode => {
                if let Some((_, email)) = self.pending_code.clone() {
                    if let Err(e) = self.client.send_email_code(&email).await {
                        self.send(Event::SignIn(SignInStep::Failed(e.to_string())));
                    }
                }
            }
            Request::SignOut => self.sign_out().await,
            Request::StoreTokenInFile(yes) => {
                let Some(auth) = self.unsaved.take() else {
                    return;
                };
                if !yes {
                    return;
                }
                let store = TokenStore::File(self.config.token_path.clone());
                let s = store.clone();
                let saved = tokio::task::spawn_blocking(move || s.save(&auth.email, &auth.token))
                    .await
                    .unwrap_or_else(|e| Err(anyhow::anyhow!("{e}")));
                match saved {
                    Ok(()) => {
                        self.store = store;
                        self.signed_in(Some(true));
                    }
                    Err(e) => self.send(Event::Error(format!("Couldn't save the token: {e:#}"))),
                }
            }
            Request::Refresh => {
                if self.auth.is_some() {
                    self.load_devices().await;
                    for d in self.devices.clone() {
                        self.load_tasks(&d.serial, TASKS_ON_CONNECT).await;
                    }
                }
            }
            Request::Tasks(serial) => self.load_tasks(&serial, TASKS_ON_CONNECT).await,
            Request::Cover { task_id, url } => self.cover(task_id, url),
            Request::History { serial, since } => {
                let history = self.history.clone();
                let shared = self.shared.clone();
                tokio::task::spawn_blocking(move || {
                    let samples = history.read(&serial, since);
                    shared.send(Event::History { serial, samples });
                });
            }
        }
    }

    async fn after_login(
        &mut self,
        region: Region,
        email: String,
        result: Result<Login, CloudError>,
        with_code: bool,
    ) {
        match result {
            Ok(Login::Token(token)) => {
                self.pending_code = None;
                self.finish_login(region, email, token).await;
            }
            Ok(Login::EmailCode) if with_code => {
                self.send(Event::SignIn(SignInStep::Failed(
                    "That code has expired. Send a new one.".into(),
                )));
            }
            Ok(Login::EmailCode) => match self.client.send_email_code(&email).await {
                Ok(()) => {
                    self.pending_code = Some((region, email));
                    self.send(Event::SignIn(SignInStep::EmailCode));
                }
                Err(e) => self.send(Event::SignIn(SignInStep::Failed(e.to_string()))),
            },
            Ok(Login::Authenticator) => self.send(Event::SignIn(SignInStep::Authenticator)),
            Err(CloudError::Unauthorized) if with_code => {
                self.send(Event::SignIn(SignInStep::Failed(
                    "That code didn't work. Check it, or send a new one.".into(),
                )));
            }
            // Bambu Lab answers a wrong password with a 400 and its own words
            // ("Incorrect account or password"), not a 401.
            Err(CloudError::Unauthorized | CloudError::Rejected(_)) if !with_code => {
                self.send(Event::SignIn(SignInStep::Failed(WRONG_PASSWORD.into())));
            }
            Err(CloudError::Rejected(m)) => {
                self.send(Event::SignIn(SignInStep::Failed(format!(
                    "Bambu Lab didn't accept the code: {m}"
                ))));
            }
            Err(e) => self.send(Event::SignIn(SignInStep::Failed(e.to_string()))),
        }
    }

    async fn finish_login(&mut self, region: Region, email: String, token: printliner_core::Token) {
        let username = match self.client.mqtt_username(&token.access_token).await {
            Ok(u) => u,
            Err(e) => {
                self.send(Event::SignIn(SignInStep::Failed(format!(
                    "Signed in, but couldn't read the account: {e}"
                ))));
                return;
            }
        };
        self.stop_mqtt();
        self.clear_printers();
        let stored = StoredToken {
            access_token: token.access_token,
            expires_at: token.expires_at,
            username,
        };
        let store = self.store.clone();
        let (e, t) = (email.clone(), stored.clone());
        let saved = tokio::task::spawn_blocking(move || store.save(&e, &t))
            .await
            .unwrap_or_else(|e| Err(anyhow::anyhow!("{e}")));
        self.auth = Some(Auth {
            email: email.clone(),
            token: stored.clone(),
        });
        self.send(Event::SignIn(SignInStep::Done {
            region,
            email: email.clone(),
        }));
        match saved {
            Ok(()) => self.signed_in(Some(matches!(self.store, TokenStore::File(_)))),
            Err(e) => {
                log::warn!("keychain: {e:#}");
                self.unsaved = Some(Auth {
                    email,
                    token: stored,
                });
                self.signed_in(None);
                self.send(Event::KeychainUnavailable);
            }
        }
        self.load_devices().await;
    }

    async fn sign_out(&mut self) {
        self.stop_mqtt();
        if let Some(auth) = self.auth.take() {
            let store = self.store.clone();
            let cleared = tokio::task::spawn_blocking(move || store.clear(&auth.email))
                .await
                .unwrap_or_else(|e| Err(anyhow::anyhow!("{e}")));
            if let Err(e) = cleared {
                self.send(Event::Error(format!(
                    "Couldn't remove the saved token: {e:#}"
                )));
            }
        }
        self.unsaved = None;
        self.pending_code = None;
        self.clear_printers();
        self.send(Event::Account(Account::SignedOut));
    }

    /// Bambu Lab refused the token.
    fn sign_in_needed(&mut self) {
        self.stop_mqtt();
        if let Some(auth) = self.auth.take() {
            self.send(Event::Account(Account::SignInNeeded { email: auth.email }));
        }
        self.send(Event::Connection(Connection::Idle));
    }

    fn clear_printers(&mut self) {
        self.devices.clear();
        self.live.clear();
        self.tasks.clear();
        self.send(Event::Devices(Vec::new()));
        self.send(Event::Connection(Connection::Idle));
    }

    async fn load_devices(&mut self) {
        let Some(auth) = &self.auth else {
            return;
        };
        match self.client.devices(&auth.token.access_token).await {
            Ok(devices) => {
                let before: HashSet<&str> =
                    self.devices.iter().map(|d| d.serial.as_str()).collect();
                let after: HashSet<&str> = devices.iter().map(|d| d.serial.as_str()).collect();
                let changed = before != after;
                self.devices = devices.clone();
                self.send(Event::Devices(devices));
                if changed || self.mqtt.is_none() {
                    self.start_mqtt();
                }
            }
            Err(CloudError::Unauthorized) => self.sign_in_needed(),
            Err(e) => self.send(Event::Error(e.to_string())),
        }
    }

    fn start_mqtt(&mut self) {
        self.stop_mqtt();
        let Some(auth) = &self.auth else {
            return;
        };
        if self.devices.is_empty() {
            return;
        }
        let mut config = mqtt::Config::for_region(
            self.config.region,
            auth.token.username.clone(),
            auth.token.access_token.clone(),
        );
        if let Some((host, port)) = &self.config.mqtt_override {
            config.host = host.clone();
            config.port = *port;
            config.tls = false;
        }
        let serials = self.devices.iter().map(|d| d.serial.clone()).collect();
        let (stop, stop_rx) = watch::channel(false);
        tokio::spawn(mqtt::run(config, serials, self.mqtt_tx.clone(), stop_rx));
        self.mqtt = Some(stop);
        self.send(Event::Connection(Connection::Connecting));
    }

    fn stop_mqtt(&mut self) {
        if let Some(stop) = self.mqtt.take() {
            let _ = stop.send(true);
        }
    }

    async fn on_mqtt(&mut self, event: MqttEvent) {
        if self.mqtt.is_none() {
            // A stopped session's last words.
            return;
        }
        match event {
            MqttEvent::Connected => {
                self.send(Event::Connection(Connection::Connected));
                for d in self.devices.clone() {
                    self.load_tasks(&d.serial, TASKS_ON_CONNECT).await;
                }
            }
            MqttEvent::Disconnected(why) => self.send(Event::Connection(Connection::Lost(why))),
            MqttEvent::AuthFailed => self.sign_in_needed(),
            MqttEvent::Report { serial, payload } => {
                if !self.devices.iter().any(|d| d.serial == serial) {
                    return;
                }
                let live = self.live.entry(serial.clone()).or_default();
                if live.state.is_null() {
                    live.state = serde_json::json!({});
                }
                merge_json(&mut live.state, &payload);
                let status = PrinterStatus::from_state(&live.state);
                let was = live.status.state;
                let ended = was.is_active() && !status.state.is_active();
                let state_changed = was != status.state;
                live.status = status;
                live.at = Some(Utc::now());
                live.dirty = true;
                if state_changed {
                    self.flush_one(&serial);
                }
                if ended {
                    self.load_tasks(&serial, TASKS_ON_FINISH).await;
                }
            }
        }
    }

    /// Sends what changed: everything dirty when `all`, else only printers
    /// not sent within the last second.
    fn flush(&mut self, all: bool) {
        let due: Vec<String> = self
            .live
            .iter()
            .filter(|(_, l)| {
                l.dirty && (all || l.sent.is_none_or(|t| t.elapsed() >= STATUS_INTERVAL))
            })
            .map(|(s, _)| s.clone())
            .collect();
        for serial in due {
            self.flush_one(&serial);
        }
    }

    fn flush_one(&mut self, serial: &str) {
        let Some(live) = self.live.get_mut(serial) else {
            return;
        };
        live.dirty = false;
        live.sent = Some(Instant::now());
        let event = Event::Status {
            serial: serial.to_string(),
            status: Box::new(live.status.clone()),
            at: live.at.unwrap_or_else(Utc::now),
        };
        self.shared.send(event);
    }

    /// One sample per printer that reported in the last minute.
    fn sample(&self) {
        let now = Utc::now();
        let samples: Vec<(String, Sample)> = self
            .live
            .iter()
            .filter(|(_, l)| {
                l.at.is_some_and(|t| now - t < chrono::Duration::seconds(60))
            })
            .map(|(s, l)| (s.clone(), Sample::of(&l.status, now)))
            .collect();
        if samples.is_empty() {
            return;
        }
        let history = self.history.clone();
        tokio::task::spawn_blocking(move || {
            for (serial, s) in samples {
                if let Err(e) = history.append(&serial, &s) {
                    log::warn!("history for {serial}: {e}");
                }
            }
        });
    }

    async fn load_tasks(&mut self, serial: &str, limit: u32) {
        let Some(auth) = &self.auth else {
            return;
        };
        match self
            .client
            .tasks(&auth.token.access_token, serial, limit)
            .await
        {
            Ok(fresh) => {
                let list = self.tasks.entry(serial.to_string()).or_default();
                *list = merge_tasks(std::mem::take(list), fresh, limit);
                let tasks = list.clone();
                self.send(Event::Tasks {
                    serial: serial.to_string(),
                    tasks,
                });
            }
            Err(CloudError::Unauthorized) => self.sign_in_needed(),
            Err(e) => log::warn!("tasks for {serial}: {e}"),
        }
    }

    /// A task's cover, from the disk cache or the cloud.
    fn cover(&self, task_id: String, url: String) {
        let Some(auth) = &self.auth else {
            return;
        };
        let token = auth.token.access_token.clone();
        let client = self.client.clone();
        let shared = self.shared.clone();
        let dir = self.config.covers_dir.clone();
        tokio::spawn(async move {
            let file = dir.join(cover_file_name(&task_id));
            if let Ok(bytes) = tokio::fs::read(&file).await {
                shared.send(Event::Cover {
                    task_id,
                    bytes: bytes.into(),
                });
                return;
            }
            match client.image(&url, &token).await {
                Ok(bytes) => {
                    let _ = tokio::fs::create_dir_all(&dir).await;
                    let _ = tokio::fs::write(&file, &bytes).await;
                    shared.send(Event::Cover {
                        task_id,
                        bytes: bytes.into(),
                    });
                }
                Err(e) => log::debug!("cover for {task_id}: {e}"),
            }
        });
    }
}

fn make_client(config: &Config, region: Region) -> Client {
    match &config.api_base {
        Some(base) => Client::with_base(base.clone(), &config.user_agent),
        None => Client::new(region, &config.user_agent),
    }
}

fn expired(token: &StoredToken) -> bool {
    token.expires_at.is_some_and(|t| t <= Utc::now())
}

/// A file name for a task's cover that can't leave the cache folder.
fn cover_file_name(task_id: &str) -> String {
    let safe: String = task_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    format!("{safe}.img")
}

/// `fresh` (newest first) over `old`, by id, newest first, at most
/// [`TASKS_ON_CONNECT`] (or `limit`, if more).
fn merge_tasks(old: Vec<Task>, fresh: Vec<Task>, limit: u32) -> Vec<Task> {
    let ids: HashSet<String> = fresh.iter().map(|t| t.id.clone()).collect();
    let mut out = fresh;
    out.extend(old.into_iter().filter(|t| !ids.contains(&t.id)));
    out.sort_by_key(|t| std::cmp::Reverse(t.start));
    out.truncate(limit.max(TASKS_ON_CONNECT) as usize);
    out
}

/// Demo mode: the made-up printers, a status a second each.
async fn run_demo(shared: Arc<Shared>, mut requests: mpsc::UnboundedReceiver<Request>) {
    let started = Instant::now();
    let devices = demo::devices();
    shared.send(Event::Account(Account::SignedIn {
        email: "demo@printliner.invalid".into(),
        expires_at: None,
        token_in_file: Some(false),
        demo: true,
    }));
    shared.send(Event::Devices(devices.clone()));
    shared.send(Event::Connection(Connection::Connected));
    let send_tasks = |serial: &str| {
        shared.send(Event::Tasks {
            serial: serial.to_string(),
            tasks: demo::tasks(serial, Utc::now()),
        });
    };
    let send_status = |secs: f64| {
        for d in &devices {
            shared.send(Event::Status {
                serial: d.serial.clone(),
                status: Box::new(PrinterStatus::from_state(&demo::report(&d.serial, secs))),
                at: Utc::now(),
            });
        }
    };
    send_status(0.0);
    for d in &devices {
        send_tasks(&d.serial);
    }
    let mut tick = tokio::time::interval(STATUS_INTERVAL);
    loop {
        tokio::select! {
            req = requests.recv() => match req {
                None => break,
                Some(Request::Tasks(serial)) => send_tasks(&serial),
                Some(Request::Refresh) => {
                    for d in &devices {
                        send_tasks(&d.serial);
                    }
                }
                Some(Request::History { serial, since }) => {
                    shared.send(Event::History {
                        samples: demo_history(&serial, since, started.elapsed().as_secs_f64()),
                        serial,
                    });
                }
                Some(Request::SignOut) => {
                    shared.send(Event::Error("Demo mode has no account to sign out of.".into()));
                }
                Some(_) => {}
            },
            _ = tick.tick() => send_status(started.elapsed().as_secs_f64()),
        }
    }
}

/// Five-minute samples from `since` to now, from the demo's reports.
fn demo_history(serial: &str, since: DateTime<Utc>, elapsed: f64) -> Vec<Sample> {
    let now = Utc::now();
    let step = chrono::Duration::minutes(5);
    let mut at = since;
    let mut out = Vec::new();
    while at <= now {
        let secs = elapsed - (now - at).num_seconds() as f64;
        let status = PrinterStatus::from_state(&demo::report(serial, secs.rem_euclid(1e9)));
        out.push(Sample::of(&status, at));
        at += step;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: &str, mins_ago: i64) -> Task {
        Task {
            id: id.into(),
            title: id.into(),
            cover: None,
            status: "completed".into(),
            weight_g: None,
            duration_secs: None,
            start: Some(Utc::now() - chrono::Duration::minutes(mins_ago)),
            end: None,
            serial: "S".into(),
        }
    }

    #[test]
    fn merging_tasks_replaces_by_id_and_keeps_order() {
        let old = vec![task("b", 20), task("a", 30)];
        let mut b_done = task("b", 20);
        b_done.status = "failed".into();
        let merged = merge_tasks(old, vec![task("c", 5), b_done], TASKS_ON_FINISH);
        let ids: Vec<&str> = merged.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["c", "b", "a"]);
        assert_eq!(merged[1].status, "failed");
    }

    #[test]
    fn cover_names_stay_in_the_folder() {
        assert_eq!(cover_file_name("../../etc/x"), "etcx.img");
        assert_eq!(cover_file_name("123-ab_c"), "123-ab_c.img");
    }

    #[test]
    fn demo_history_covers_the_range() {
        let since = Utc::now() - chrono::Duration::hours(1);
        let s = demo_history(demo::X1C, since, 100.0);
        assert_eq!(s.len(), 13);
        assert!(s.iter().all(|x| x.nozzle.is_some()));
    }
}
