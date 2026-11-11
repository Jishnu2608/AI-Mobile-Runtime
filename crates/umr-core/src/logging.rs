//! Logging for the Runtime Core.
//!
//! - Always writes to `logs/core.log.<date>` (daily rotation, 14 files kept).
//! - In foreground mode also writes to stderr.
//! - Level from `UMR_LOG` (e.g. `debug`, `umr_core=trace`), default `info`.

use std::io;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter, Layer};

use crate::paths::CorePaths;

/// Environment variable with the log filter.
pub const LOG_ENV: &str = "UMR_LOG";

/// Number of daily log files kept.
pub const KEEP_LOG_FILES: usize = 14;

/// Keeps the background log writer alive; flushes on drop.
#[derive(Debug)]
pub struct LogGuard {
    _file: WorkerGuard,
}

/// Installs the global logger. Call once per process.
/// `default_filter` comes from `config.toml` (`[core] log`); `UMR_LOG` overrides it.
pub fn init(paths: &CorePaths, foreground: bool, default_filter: &str) -> io::Result<LogGuard> {
    paths.ensure_dirs()?;
    let appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("core")
        .filename_suffix("log")
        .max_log_files(KEEP_LOG_FILES)
        .build(paths.logs_dir())
        .map_err(io::Error::other)?;
    let (file_writer, guard) = tracing_appender::non_blocking(appender);

    let filter = || {
        EnvFilter::try_from_env(LOG_ENV)
            .or_else(|_| EnvFilter::try_new(default_filter))
            .unwrap_or_else(|_| EnvFilter::new("info"))
    };
    let file_layer = fmt::layer()
        .with_writer(file_writer)
        .with_ansi(false)
        .with_filter(filter());
    let stderr_layer =
        foreground.then(|| fmt::layer().with_writer(io::stderr).with_filter(filter()));

    tracing_subscriber::registry()
        .with(file_layer)
        .with(stderr_layer)
        .try_init()
        .map_err(io::Error::other)?;
    Ok(LogGuard { _file: guard })
}
