//! Update check against GitHub Releases, shared by the Tauri and native
//! builds. Native packages (deb/rpm/Flatpak, MSI, dmg) are updated by their
//! package manager or by the user, so this only *checks* and reports the
//! release page to open; there is no in-place install.

use anyhow::{anyhow, Result};
use serde::Deserialize;

/// A published release newer than the running version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    /// The version without a leading `v`, e.g. `2.1.0`.
    pub version: String,
    /// The release page.
    pub url: String,
    pub notes: Option<String>,
}

/// The outcome of [`check`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateCheck {
    Available(Release),
    /// Nothing newer; `latest` is the newest published version.
    UpToDate {
        latest: String,
    },
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    draft: bool,
}

/// Compares the newest release of `repo` (`owner/name`) with `current`.
pub async fn check(repo: &str, current: &str) -> Result<UpdateCheck> {
    let rel = latest(repo).await?;
    let version = rel.tag_name.trim_start_matches('v').to_string();
    Ok(if is_newer(current, &rel.tag_name) {
        UpdateCheck::Available(Release {
            version,
            url: rel.html_url,
            notes: rel.body,
        })
    } else {
        UpdateCheck::UpToDate { latest: version }
    })
}

/// Whether `latest` is a newer semver than `current` (either may start with `v`).
/// Unparseable versions never count as newer.
pub fn is_newer(current: &str, latest: &str) -> bool {
    matches!((parse_version(current), parse_version(latest)), (Some(c), Some(l)) if l > c)
}

fn parse_version(v: &str) -> Option<semver::Version> {
    semver::Version::parse(v.trim().trim_start_matches('v')).ok()
}

async fn latest(repo: &str) -> Result<GhRelease> {
    let client = reqwest::Client::builder()
        .user_agent(concat!("laankit-updater/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(15))
        .build()?;
    // Includes pre-releases (continuous builds), like electron-updater's allowPrerelease.
    let releases: Vec<GhRelease> = client
        .get(format!(
            "https://api.github.com/repos/{repo}/releases?per_page=10"
        ))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    releases
        .into_iter()
        .find(|r| !r.draft)
        .ok_or_else(|| anyhow!("No releases published"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_versions() {
        assert!(is_newer("2.0.1", "v2.1.0"));
        assert!(is_newer("v2.0.0", "2.0.1-rc.1"));
        assert!(!is_newer("2.1.0", "v2.1.0"));
        assert!(!is_newer("2.1.0", "2.0.9"));
        assert!(!is_newer("2.1.0-rc.2", "2.1.0-rc.1"));
        assert!(!is_newer("2.0.0", "nightly"));
        assert!(!is_newer("garbage", "3.0.0"));
    }
}
