//! `update:*` channels. Electron used electron-updater; native Linux
//! packages (deb/rpm/Flatpak) are updated by the package manager, so this
//! only *checks* GitHub Releases (`marina_core::updates`) and sends the
//! user to the release page.
//! The renderer's state machine (idle → checking → available/unavailable/
//! error) is unchanged.

use std::sync::Mutex;

use marina_core::updates::{self, UpdateCheck};
use serde_json::{json as j, Value};
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

use crate::host::Host;
use crate::ipc::{json, Args, IpcResult};

pub struct Updater {
    /// GitHub `owner/repo` that publishes this app's releases, if any.
    repo: Option<&'static str>,
    state: Mutex<Value>,
    release_url: Mutex<Option<String>>,
}

impl Updater {
    pub fn new(repo: Option<&'static str>) -> Self {
        Self { repo, state: Mutex::new(j!({ "state": "idle" })), release_url: Mutex::new(None) }
    }

    fn set(&self, host: &Host, app: &AppHandle, state: Value) {
        *self.state.lock().unwrap() = state.clone();
        host.send(app, "update:state", state);
    }

    pub async fn check(&self, host: &Host, app: &AppHandle) -> bool {
        if cfg!(debug_assertions) {
            self.set(host, app, j!({ "state": "unavailable", "reason": "dev" }));
            return false;
        }
        let Some(repo) = self.repo else {
            self.set(host, app, j!({ "state": "unavailable" }));
            return false;
        };
        self.set(host, app, j!({ "state": "checking" }));
        let current = app.package_info().version.to_string();
        match updates::check(repo, &current).await {
            Ok(UpdateCheck::Available(rel)) => {
                *self.release_url.lock().unwrap() = Some(rel.url);
                self.set(host, app, j!({ "state": "available", "version": rel.version, "notes": rel.notes }));
                true
            }
            Ok(UpdateCheck::UpToDate { latest }) => {
                self.set(host, app, j!({ "state": "unavailable", "version": latest }));
                true
            }
            Err(e) => {
                self.set(host, app, j!({ "state": "error", "error": e.to_string() }));
                false
            }
        }
    }

    pub async fn route(&self, host: &Host, app: &AppHandle, channel: &str, _args: &Args) -> Option<IpcResult> {
        Some(match channel {
            "update:getState" => json(self.state.lock().unwrap().clone()),
            "update:checkNow" => json(self.check(host, app).await),
            // No in-place install for system packages: open the release page.
            "update:downloadNow" | "update:installNow" => {
                let url = self.release_url.lock().unwrap().clone();
                match url {
                    Some(u) => json(app.opener().open_url(u, None::<&str>).is_ok()),
                    None => json(false),
                }
            }
            _ => return None,
        })
    }
}
