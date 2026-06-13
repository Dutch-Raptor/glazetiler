use crate::diagnostics::{log_dir, LOG_FILE_PREFIX, LOG_RETENTION_DAYS};
use anyhow::{anyhow, Context};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};

pub fn init_file_logging() -> anyhow::Result<WorkerGuard> {
    let log_dir = log_dir();
    std::fs::create_dir_all(&log_dir).with_context(|| {
        format!(
            "Failed to create GlazeTiler log folder at {}",
            log_dir.display()
        )
    })?;

    let file_appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(LOG_FILE_PREFIX)
        .max_log_files(LOG_RETENTION_DAYS)
        .build(&log_dir)
        .with_context(|| {
            format!(
                "Failed to initialize GlazeTiler file logging in {}",
                log_dir.display()
            )
        })?;
    let (writer, guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        .with_target(true)
        .try_init()
        .map_err(|error| anyhow!("Failed to install GlazeTiler tracing subscriber: {error}"))?;

    Ok(guard)
}
