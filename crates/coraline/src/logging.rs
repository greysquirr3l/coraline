#![forbid(unsafe_code)]

//! Structured logging setup for Coraline.
//!
//! Initializes `tracing` with:
//! - File output to `.coraline/logs/coraline.log` (daily rotation)
//! - Stderr fallback when no project root is available
//! - Log level controlled by `CORALINE_LOG` env var (default: `coraline=info`)

use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

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
    let env_filter = EnvFilter::try_from_env("CORALINE_LOG")
        .unwrap_or_else(|_| EnvFilter::new("coraline=info"));

    // Attempt to set up file logging
    if let Some(root) = project_root {
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
