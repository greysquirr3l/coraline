#![forbid(unsafe_code)]

//! Structured logging setup for Coraline.
//!
//! Initializes `tracing` with:
//! - File output to `.coraline/logs/coraline.log` (daily rotation)
//! - Stderr fallback when no project root is available
//! - Log level controlled by `CORALINE_LOG` env var (default: `coraline=info`)

use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

use crate::config;

/// Opaque guard that must be kept alive for the duration of the program.
/// When dropped, the file appender worker thread flushes and exits.
pub struct LogGuard {
    _guard: Option<WorkerGuard>,
}

/// Initialize structured logging.
///
/// Returns a [`LogGuard`] that must be held for the duration of the program.
/// Dropping it before exit will stop log flushing.
///
/// Log level is read from `CORALINE_LOG` environment variable (e.g. `debug`,
/// `coraline=trace`). Defaults to `coraline=info`.
///
/// If `project_root` is provided and `.coraline/logs/` can be created, logs
/// are written to a daily-rotating file there. Otherwise logs go to stderr.
pub fn init(project_root: Option<&Path>) -> LogGuard {
    let env_filter =
        EnvFilter::try_from_env("CORALINE_LOG").unwrap_or_else(|_| EnvFilter::new("coraline=info"));

    // When no project root is available (e.g. fresh `init` before `.coraline`
    // exists), avoid stderr logging so progress output remains stable.
    if project_root.is_none() {
        return LogGuard { _guard: None };
    }

    // Attempt to set up file logging, but only into an already-initialized
    // project. Creating `.coraline/logs/` on a project that hasn't run
    // `coraline init` yet would make directory-existence-based init checks
    // see a partially-initialized project (logs only, no config/db) and
    // block a real `coraline init` from running cleanly.
    if let Some(root) = project_root
        && config::toml_config_path(root).is_file()
    {
        let log_dir = root.join(".coraline").join("logs");
        if std::fs::create_dir_all(&log_dir).is_ok() {
            let file_appender = tracing_appender::rolling::daily(&log_dir, "coraline.log");
            let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

            let result = tracing_subscriber::registry()
                .with(env_filter.clone())
                .with(fmt::layer().with_writer(non_blocking).with_ansi(false))
                .try_init();

            if result.is_ok() {
                return LogGuard {
                    _guard: Some(guard),
                };
            }
        }
    }

    // Fallback: stderr logging (swallow error if already initialized)
    let _ = tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt::layer().with_writer(std::io::stderr))
        .try_init();

    LogGuard { _guard: None }
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    /// Regression test for: the MCP server (and any other non-`init`
    /// command) starting logging against an uninitialized project used to
    /// create `.coraline/logs/` on its own, which made `is_initialized()`
    /// checks see a partially-initialized project and forced users to
    /// delete `.coraline/` by hand before `coraline init` would work.
    #[test]
    fn init_does_not_create_coraline_dir_for_uninitialized_project() -> TestResult {
        let temp_dir = tempfile::TempDir::new()?;
        let root = temp_dir.path();
        assert!(!root.join(".coraline").exists());

        let _guard = init(Some(root));

        assert!(
            !root.join(".coraline").exists(),
            ".coraline/ must not be created for a project without config.toml"
        );
        Ok(())
    }

    #[test]
    fn init_creates_log_dir_for_already_initialized_project() -> TestResult {
        let temp_dir = tempfile::TempDir::new()?;
        let root = temp_dir.path();
        let coraline_dir = root.join(".coraline");
        std::fs::create_dir_all(&coraline_dir)?;
        std::fs::write(coraline_dir.join("config.toml"), "")?;

        let _guard = init(Some(root));

        assert!(
            coraline_dir.join("logs").is_dir(),
            "logs/ should be created once the project is genuinely initialized"
        );
        Ok(())
    }
}
