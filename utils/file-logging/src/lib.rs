use anyhow::Result;
use chrono::format::strftime::StrftimeItems;
use chrono::format::DelayedFormat;
use chrono::{DateTime, Local};
use flexi_logger::{
    style, AdaptiveFormat, Age, Cleanup, Criterion, DeferredNow, Duplicate, Logger, Naming, Record,
};
use std::path::Path;

pub use flexi_logger::LoggerHandle;

fn log_format_date(now: &mut DeferredNow) -> DelayedFormat<StrftimeItems> {
    //use DateTime::<Local> instead of DateTime::<UTC> to obtain local date
    let local_date = DateTime::<Local>::from(*now.now());

    //format date as following: 2020-08-27T07:56:22.348+02:00 (local date + time zone with milliseconds precision)
    const DATE_FORMAT_STR: &str = "%Y-%m-%dT%H:%M:%S%.3f%z";
    local_date.format(DATE_FORMAT_STR)
}

fn log_format(
    w: &mut dyn std::io::Write,
    now: &mut DeferredNow,
    record: &Record,
) -> Result<(), std::io::Error> {
    write!(
        w,
        "[{} {:5} {}] {}",
        log_format_date(now),
        record.level(),
        record.module_path().unwrap_or("<unnamed>"),
        record.args()
    )
}

fn log_format_color(
    w: &mut dyn std::io::Write,
    now: &mut DeferredNow,
    record: &Record,
) -> Result<(), std::io::Error> {
    let level = record.level();
    write!(
        w,
        "[{} {:5} {}] {}",
        yansi::Color::Fixed(247).paint(log_format_date(now)),
        style(level, level),
        yansi::Color::Fixed(247).paint(record.module_path().unwrap_or("<unnamed>")),
        &record.args()
    )
}

fn set_logging_to_files(logger: Logger, log_dir: &Path) -> Logger {
    logger
        .log_to_file()
        .directory(log_dir)
        .rotate(
            Criterion::AgeOrSize(Age::Day, /*size in bytes*/ 1024 * 1024 * 1024),
            Naming::Timestamps,
            Cleanup::KeepLogAndCompressedFiles(1, 10),
        )
        .print_message()
        .duplicate_to_stderr(Duplicate::All)
}

pub fn start_logger(
    default_level: &str,
    log_dir: Option<&Path>,
    module_overrides: &str,
    force_debug: bool,
) -> Result<LoggerHandle> {
    //override default log level if force_debug is set
    //it leaves module_filters log levels unchanged
    //used for --debug option
    let default_log_str = format!(
        "{},{}",
        module_overrides,
        if force_debug { "debug" } else { default_level }
    );

    let mut logger = Logger::with_env_or_str(default_log_str).format(log_format);
    if let Some(log_dir) = log_dir {
        logger = set_logging_to_files(logger, log_dir);
    }
    logger = logger
        .adaptive_format_for_stderr(AdaptiveFormat::Custom(log_format, log_format_color))
        .set_palette("9;11;2;7;8".to_string());

    Ok(logger.start()?)
}
