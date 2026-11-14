//! Where the Bambu access token is kept. The password is never stored.
//!
//! - [`TokenStore::Keychain`]: the OS keychain (Secret Service on Linux,
//!   Credential Manager on Windows, Keychain on macOS), under the service
//!   [`SERVICE`] and the account email.
//! - [`TokenStore::File`]: a JSON file readable only by you (`0600` on
//!   Unix), for machines with no keychain. Used only when you agree to it.
//!
//! The calls block (the keychain talks to a system service), so run them
//! off the UI thread and outside async code.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// The keychain service name.
pub const SERVICE: &str = "com.printliner.app";

/// What's stored: the token, its expiry, and the MQTT username, so a
/// restart needs no request before connecting.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredToken {
    pub access_token: String,
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
    /// `u_<uid>`.
    pub username: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TokenStore {
    Keychain,
    File(PathBuf),
}

impl TokenStore {
    pub fn load(&self, email: &str) -> anyhow::Result<Option<StoredToken>> {
        let text = match self {
            TokenStore::Keychain => match keyring::Entry::new(SERVICE, email)?.get_password() {
                Ok(t) => t,
                Err(keyring::Error::NoEntry) => return Ok(None),
                Err(e) => return Err(e.into()),
            },
            TokenStore::File(path) => match std::fs::read_to_string(path) {
                Ok(t) => t,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(e) => return Err(e.into()),
            },
        };
        Ok(serde_json::from_str(&text).ok())
    }

    pub fn save(&self, email: &str, token: &StoredToken) -> anyhow::Result<()> {
        let text = serde_json::to_string(token)?;
        match self {
            TokenStore::Keychain => keyring::Entry::new(SERVICE, email)?.set_password(&text)?,
            TokenStore::File(path) => write_private(path, &text)?,
        }
        Ok(())
    }

    pub fn clear(&self, email: &str) -> anyhow::Result<()> {
        match self {
            TokenStore::Keychain => {
                match keyring::Entry::new(SERVICE, email)?.delete_credential() {
                    Ok(()) | Err(keyring::Error::NoEntry) => {}
                    Err(e) => return Err(e.into()),
                }
            }
            TokenStore::File(path) => match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            },
        }
        Ok(())
    }
}

/// Writes `text` to a temp file only you can read, then renames it over
/// `path`, so the token is never briefly readable by others.
fn write_private(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut f = options.open(&tmp)?;
    f.write_all(text.as_bytes())?;
    f.sync_all()?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_store_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let store = TokenStore::File(tmp.path().join("cfg").join("token.json"));
        assert_eq!(store.load("a@b.c").unwrap(), None);
        let token = StoredToken {
            access_token: "secret".into(),
            expires_at: None,
            username: "u_1".into(),
        };
        store.save("a@b.c", &token).unwrap();
        assert_eq!(store.load("a@b.c").unwrap(), Some(token));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let path = tmp.path().join("cfg").join("token.json");
            let mode = std::fs::metadata(path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        store.clear("a@b.c").unwrap();
        store.clear("a@b.c").unwrap();
        assert_eq!(store.load("a@b.c").unwrap(), None);
    }
}
