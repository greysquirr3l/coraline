//! Update checking for Coraline.
//!
//! Checks crates.io for the latest published version and compares it against the current binary.

use std::fmt;
use std::time::Duration;

use serde::Deserialize;

/// Crates.io API response structure.
#[derive(Debug, Deserialize)]
struct CrateResponse {
    #[serde(rename = "crate")]
    krate: CrateInfo,
}

#[derive(Debug, Deserialize)]
struct CrateInfo {
    max_stable_version: String,
}

/// Errors that can occur during update checking.
#[derive(Debug)]
pub enum UpdateError {
    /// Network request failed.
    Network(String),
    /// Failed to parse response.
    Parse(String),
    /// Version comparison failed.
    Version(String),
}

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Network(msg) => write!(f, "network error: {msg}"),
            Self::Parse(msg) => write!(f, "parse error: {msg}"),
            Self::Version(msg) => write!(f, "version error: {msg}"),
        }
    }
}

impl std::error::Error for UpdateError {}

/// Result of an update check.
#[derive(Debug)]
pub enum UpdateStatus {
    /// Current version is up to date.
    UpToDate { current: String },
    /// A newer version is available.
    UpdateAvailable { current: String, latest: String },
    /// Current version is newer than latest (pre-release or development build).
    Ahead { current: String, latest: String },
}

impl UpdateStatus {
    /// Returns `true` if an update is available.
    #[must_use]
    pub const fn has_update(&self) -> bool {
        matches!(self, Self::UpdateAvailable { .. })
    }
}

/// Fetches the latest version from crates.io.
///
/// # Errors
///
/// Returns an error if the network request fails or the response cannot be parsed.
pub fn fetch_latest_version() -> Result<String, UpdateError> {
    const CRATES_IO_API: &str = "https://crates.io/api/v1/crates/coraline";
    const TIMEOUT_SECS: u64 = 10;

    let agent = ureq::Agent::new_with_config(
        ureq::config::Config::builder()
            .timeout_global(Some(Duration::from_secs(TIMEOUT_SECS)))
            .user_agent("coraline-update-checker")
            .build(),
    );

    let mut response = agent
        .get(CRATES_IO_API)
        .call()
        .map_err(|e| UpdateError::Network(e.to_string()))?;

    let body: CrateResponse = response
        .body_mut()
        .read_json()
        .map_err(|e| UpdateError::Parse(e.to_string()))?;

    Ok(body.krate.max_stable_version)
}

/// Compares two semver version strings.
///
/// Returns:
/// - `Ok(Ordering::Less)` if `current < latest`
/// - `Ok(Ordering::Equal)` if `current == latest`
/// - `Ok(Ordering::Greater)` if `current > latest`
///
/// # Errors
///
/// Returns an error if either version string cannot be parsed.
fn compare_versions(current: &str, latest: &str) -> Result<std::cmp::Ordering, UpdateError> {
    let parse = |v: &str| -> Result<(u32, u32, u32), UpdateError> {
        let parts: Vec<&str> = v.trim_start_matches('v').split('.').collect();
        if parts.len() < 3 {
            return Err(UpdateError::Version(format!("invalid version format: {v}")));
        }
        let major_str = parts
            .first()
            .ok_or_else(|| UpdateError::Version(format!("missing major version in: {v}")))?;
        let minor_str = parts
            .get(1)
            .ok_or_else(|| UpdateError::Version(format!("missing minor version in: {v}")))?;
        let patch_part = parts
            .get(2)
            .ok_or_else(|| UpdateError::Version(format!("missing patch version in: {v}")))?;

        let major = major_str
            .parse()
            .map_err(|_| UpdateError::Version(format!("invalid major version in: {v}")))?;
        let minor = minor_str
            .parse()
            .map_err(|_| UpdateError::Version(format!("invalid minor version in: {v}")))?;
        // Handle pre-release suffixes like "0.6.0-alpha"
        let patch_str = patch_part.split('-').next().unwrap_or(patch_part);
        let patch = patch_str
            .parse()
            .map_err(|_| UpdateError::Version(format!("invalid patch version in: {v}")))?;
        Ok((major, minor, patch))
    };

    let current = parse(current)?;
    let latest = parse(latest)?;

    Ok(current.cmp(&latest))
}

/// Checks if there's a newer version of Coraline available on crates.io.
///
/// # Errors
///
/// Returns an error if the update check fails.
pub fn check_for_update() -> Result<UpdateStatus, UpdateError> {
    let current = env!("CARGO_PKG_VERSION").to_string();
    let latest = fetch_latest_version()?;

    match compare_versions(&current, &latest)? {
        std::cmp::Ordering::Less => Ok(UpdateStatus::UpdateAvailable { current, latest }),
        std::cmp::Ordering::Equal => Ok(UpdateStatus::UpToDate { current }),
        std::cmp::Ordering::Greater => Ok(UpdateStatus::Ahead { current, latest }),
    }
}

/// Prints update status to stdout.
pub fn print_update_status(status: &UpdateStatus) {
    match status {
        UpdateStatus::UpToDate { current } => {
            println!("✓ Coraline v{current} is up to date.");
        }
        UpdateStatus::UpdateAvailable { current, latest } => {
            println!("⬆ Update available: v{current} → v{latest}");
            println!();
            println!("  To update, run:");
            println!("    cargo install coraline --force");
            println!();
            println!("  Or visit: https://crates.io/crates/coraline");
        }
        UpdateStatus::Ahead { current, latest } => {
            println!("✓ Coraline v{current} (ahead of latest release v{latest})");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering;

    #[test]
    fn test_compare_versions_equal() -> Result<(), UpdateError> {
        assert_eq!(compare_versions("0.6.0", "0.6.0")?, Ordering::Equal);
        Ok(())
    }

    #[test]
    fn test_compare_versions_less() -> Result<(), UpdateError> {
        assert_eq!(compare_versions("0.5.0", "0.6.0")?, Ordering::Less);
        assert_eq!(compare_versions("0.5.9", "0.6.0")?, Ordering::Less);
        assert_eq!(compare_versions("0.6.0", "1.0.0")?, Ordering::Less);
        Ok(())
    }

    #[test]
    fn test_compare_versions_greater() -> Result<(), UpdateError> {
        assert_eq!(compare_versions("0.7.0", "0.6.0")?, Ordering::Greater);
        assert_eq!(compare_versions("1.0.0", "0.6.0")?, Ordering::Greater);
        Ok(())
    }

    #[test]
    fn test_compare_versions_with_v_prefix() -> Result<(), UpdateError> {
        assert_eq!(compare_versions("v0.6.0", "0.6.0")?, Ordering::Equal);
        Ok(())
    }

    #[test]
    fn test_compare_versions_with_prerelease() -> Result<(), UpdateError> {
        // Pre-release suffixes are stripped for comparison
        assert_eq!(compare_versions("0.6.0-alpha", "0.6.0")?, Ordering::Equal);
        Ok(())
    }
}
