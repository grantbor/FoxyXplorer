//! Manual update checking through published stable GitHub releases.
//! The module deliberately does not download or install anything.

use std::time::Duration;

pub(crate) const RELEASES_URL: &str = "https://github.com/grantbor/FoxyXplorer/releases";
const LATEST_RELEASE_API: &str =
    "https://api.github.com/repos/grantbor/FoxyXplorer/releases/latest";

#[derive(Debug, Clone)]
pub(crate) enum Status {
    NotChecked,
    Checking,
    UpToDate,
    Available(String),
    NoReleases,
    Failed,
}

/// Fetch the latest stable published release tag; 404 means no published release.
/// A Git tag without an associated GitHub Release is not visible to this API.
pub(crate) async fn fetch_latest_release() -> Result<Option<String>, ()> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent(format!(
            "FoxyXplorer/{} (+https://github.com/grantbor/FoxyXplorer)",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .map_err(|_| ())?;

    let response = client
        .get(LATEST_RELEASE_API)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .map_err(|_| ())?;

    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let response = response.error_for_status().map_err(|_| ())?;
    let body = response.text().await.map_err(|_| ())?;
    let release: serde_json::Value = serde_json::from_str(&body).map_err(|_| ())?;
    let tag = release
        .get("tag_name")
        .and_then(|tag| tag.as_str())
        .filter(|tag| !tag.is_empty())
        .ok_or(())?;
    Ok(Some(tag.to_owned()))
}

/// Compare numeric version components, not version strings: 0.3.0 > 0.2.9.
/// Accepts GitHub tags like v0.3.0 and the Cargo version 0.3.0.
fn components(version: &str) -> Option<(u64, u64, u64)> {
    let version = version.trim().trim_start_matches(['v', 'V']);
    let core = version.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

pub(crate) fn is_newer(release_tag: &str, local_version: &str) -> Option<bool> {
    Some(components(release_tag)? > components(local_version)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_version_comparison() {
        assert_eq!(is_newer("v0.3.0", "0.2.9"), Some(true));
        assert_eq!(is_newer("v0.4.10", "0.4.9"), Some(true));
        assert_eq!(is_newer("v0.4.3", "0.4.3"), Some(false));
        assert_eq!(is_newer("v0.4.2", "0.4.3"), Some(false));
        assert_eq!(is_newer("v1.0.0", "0.99.99"), Some(true));
        assert_eq!(is_newer("not-a-version", "0.4.3"), None);
        assert_eq!(is_newer("v0.4", "0.4.3"), None);
    }
}
