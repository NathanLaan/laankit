//! Update check against GitHub Releases, shared by the Tauri and native
//! builds. Native packages (deb/rpm/Flatpak, MSI, dmg) are updated by their
//! package manager or by the user, so this only *checks* and reports the
//! release page to open; there is no in-place install.

use anyhow::{anyhow, Result};
use serde::Deserialize;

/// The tag prefix of the native apps' releases: `release-native.yml` runs on
/// `native-v*` tags, so `native-v2.3.1` is version `2.3.1`.
pub const NATIVE_TAG_PREFIX: &str = "native-v";

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

/// Compares the newest release of `repo` (`owner/name`) whose tag starts with
/// `tag_prefix` (e.g. [`NATIVE_TAG_PREFIX`], or `v`) with `current`. A
/// repository can publish several release lines; the others are ignored.
pub async fn check(repo: &str, tag_prefix: &str, current: &str) -> Result<UpdateCheck> {
    let rel = latest(repo, tag_prefix).await?;
    let version = tag_version(&rel.tag_name, tag_prefix)
        .unwrap_or_default()
        .to_string();
    Ok(if is_newer(current, &version) {
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

/// The version in `tag`: what follows `prefix`, or `None` for another
/// release line's tag.
fn tag_version<'a>(tag: &'a str, prefix: &str) -> Option<&'a str> {
    tag.strip_prefix(prefix)
}

fn parse_version(v: &str) -> Option<semver::Version> {
    semver::Version::parse(v.trim().trim_start_matches('v')).ok()
}

async fn latest(repo: &str, tag_prefix: &str) -> Result<GhRelease> {
    let client = reqwest::Client::builder()
        .user_agent(concat!("laankit-updater/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(15))
        .build()?;
    // Includes pre-releases (continuous builds), like electron-updater's allowPrerelease.
    let releases: Vec<GhRelease> = client
        .get(format!(
            "https://api.github.com/repos/{repo}/releases?per_page=100"
        ))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    releases
        .into_iter()
        .find(|r| !r.draft && tag_version(&r.tag_name, tag_prefix).is_some())
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

    #[test]
    fn tag_prefixes() {
        assert_eq!(
            tag_version("native-v2.3.1", NATIVE_TAG_PREFIX),
            Some("2.3.1")
        );
        assert_eq!(tag_version("v2.3.1", "v"), Some("2.3.1"));
        assert_eq!(tag_version("desktop-ui-v1.0.1", NATIVE_TAG_PREFIX), None);
        assert!(is_newer(
            "2.3.0",
            tag_version("native-v2.3.1", NATIVE_TAG_PREFIX).unwrap()
        ));
    }
}
