use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{Builder, Rotation};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, fmt};

const FILE_SWITCH: &str = "THELEMAIL_DESKTOP_LOG_FILE";
const KEPT_FILES: usize = 7;

pub fn init() -> Option<WorkerGuard> {
    let filter =
        EnvFilter::try_from_env("THELEMAIL_DESKTOP_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    let stderr = fmt::layer().with_writer(std::io::stderr).with_target(false);

    let (file, guard) = match file_appender() {
        Some((writer, guard)) => (
            Some(
                fmt::layer()
                    .with_writer(writer)
                    .with_ansi(false)
                    .with_target(false),
            ),
            Some(guard),
        ),
        None => (None, None),
    };

    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(stderr)
        .with(file)
        .try_init();
    guard
}

fn file_appender() -> Option<(tracing_appender::non_blocking::NonBlocking, WorkerGuard)> {
    if std::env::var_os(FILE_SWITCH).is_none_or(|v| v != "1") {
        return None;
    }
    let dir = thelemail_core::paths::logs_root();
    let appender = Builder::new()
        .rotation(Rotation::DAILY)
        .filename_prefix("thelemail")
        .filename_suffix("log")
        .max_log_files(KEPT_FILES)
        .build(&dir);
    match appender {
        Ok(appender) => Some(tracing_appender::non_blocking(appender)),
        Err(err) => {
            eprintln!("logging: cannot write to {}: {err}", dir.display());
            None
        }
    }
}
