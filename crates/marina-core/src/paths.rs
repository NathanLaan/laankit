//! Filesystem locations. Mirrors Electron's `app.getPath(...)` so the
//! Electron, Tauri and native builds share settings and data:
//!
//! | Electron    | Linux                         | Windows                 | macOS                                  |
//! |-------------|-------------------------------|-------------------------|----------------------------------------|
//! | `appData`   | `$XDG_CONFIG_HOME` or `~/.config` | `%APPDATA%`         | `~/Library/Application Support`        |
//! | `userData`  | `<appData>/<ProductName>`     | same                    | same                                   |
//! | `documents` | `xdg-user-dir DOCUMENTS`      | `%USERPROFILE%\Documents` | `~/Documents`                       |

use std::path::PathBuf;

pub fn home_dir() -> PathBuf {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var).filter(|p| !p.is_empty()).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

/// Electron's `app.getPath('appData')`: the per-user application data folder.
pub fn config_home() -> PathBuf {
    let var = if cfg!(windows) { "APPDATA" } else { "XDG_CONFIG_HOME" };
    match std::env::var_os(var) {
        // Electron honours XDG_CONFIG_HOME on Linux only.
        Some(p) if !p.is_empty() && !cfg!(target_os = "macos") => PathBuf::from(p),
        _ if cfg!(target_os = "macos") => home_dir().join("Library").join("Application Support"),
        _ if cfg!(windows) => home_dir().join("AppData").join("Roaming"),
        _ => home_dir().join(".config"),
    }
}

/// Electron's `userData`, e.g. `~/.config/NoteLiner` on Linux.
pub fn user_data_dir(product_name: &str) -> PathBuf {
    config_home().join(product_name)
}

/// Electron's `app.getPath('documents')`.
pub fn documents_dir() -> PathBuf {
    if cfg!(all(unix, not(target_os = "macos"))) {
        if let Ok(out) = std::process::Command::new("xdg-user-dir").arg("DOCUMENTS").output() {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if out.status.success() && !s.is_empty() {
                return PathBuf::from(s);
            }
        }
    }
    home_dir().join("Documents")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_data_dir_uses_product_name() {
        let p = user_data_dir("NoteLiner");
        assert!(p.ends_with("NoteLiner"));
    }
}
