//! The optional update notice (phase F of the program to 1.0.0): off by
//! default; when on, once a day, ask GitHub which version is the latest and
//! tell the person when it is newer. Nothing is downloaded or installed.
//!
//! - one fixed URL, over HTTPS (rustls), with no identifying data (the
//!   user agent is the app's name and version);
//! - the answer is only trusted as a version number `X.Y.Z`;
//! - the release page opened in the browser is built here from that number,
//!   never taken from the answer.
//!
//! Packagers can build without it (`--no-default-features`): the setting is
//! then hidden and no request is ever made.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Where releases are published.
pub const RELEASES_URL: &str = "https://github.com/lucastelso/ikebox/releases";
#[cfg(feature = "update-check")]
const LATEST_API: &str = "https://api.github.com/repos/lucastelso/ikebox/releases/latest";

/// How often the check runs.
pub const INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// Is the check built in?
#[must_use]
pub fn available() -> bool {
    cfg!(feature = "update-check")
}

/// Seconds since the epoch.
#[must_use]
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// `X.Y.Z` (an optional leading `v` removed), or `None`.
#[must_use]
pub fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let text = text.strip_prefix('v').unwrap_or(text);
    let mut parts = text.split('.');
    let mut next = || -> Option<u64> {
        let p = parts.next()?;
        if p.is_empty() || p.len() > 6 || !p.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        p.parse().ok()
    };
    let version = (next()?, next()?, next()?);
    parts.next().is_none().then_some(version)
}

/// `latest` is newer than this app.
#[must_use]
pub fn is_newer(latest: &str) -> bool {
    match (parse_version(latest), parse_version(ikebox_core::VERSION)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

/// The release page of `version` (already validated).
#[must_use]
pub fn release_page(version: &str) -> Option<String> {
    let (a, b, c) = parse_version(version)?;
    Some(format!("{RELEASES_URL}/tag/v{a}.{b}.{c}"))
}

/// Asks GitHub for the latest release. Blocking.
///
/// # Errors
/// Network, an HTTP error or an answer without a valid version.
#[cfg(feature = "update-check")]
pub fn latest_version() -> Result<String, String> {
    #[derive(serde::Deserialize)]
    struct Release {
        tag_name: String,
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .https_only(true)
        .user_agent(format!("ikebox/{}", ikebox_core::VERSION))
        .build()
        .into();
    let mut response = agent
        .get(LATEST_API)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| e.to_string())?;
    let body = response
        .body_mut()
        .with_config()
        .limit(256 * 1024)
        .read_to_string()
        .map_err(|e| e.to_string())?;
    let release: Release = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let (a, b, c) = parse_version(&release.tag_name).ok_or("not a version number")?;
    Ok(format!("{a}.{b}.{c}"))
}

/// Without the feature, nothing is ever asked.
///
/// # Errors
/// Always: the check is not built in.
#[cfg(not(feature = "update-check"))]
pub fn latest_version() -> Result<String, String> {
    Err("the update check is not built in".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_only_numbers() {
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("10.0.12"), Some((10, 0, 12)));
        for bad in [
            "",
            "1.2",
            "1.2.3.4",
            "1.2.x",
            "v1.2.3-rc.1",
            "1..3",
            "../1.2.3",
            "1.2.3 ",
        ] {
            assert_eq!(parse_version(bad), None, "{bad}");
        }
    }

    #[test]
    fn newer_and_release_page() {
        assert!(is_newer("999.0.0"));
        assert!(!is_newer("0.0.1"));
        assert!(!is_newer("junk"));
        assert_eq!(
            release_page("v1.4.2").as_deref(),
            Some("https://github.com/lucastelso/ikebox/releases/tag/v1.4.2")
        );
        assert_eq!(release_page("1.4.2/../../evil"), None);
    }
}
