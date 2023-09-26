use anyhow::Result;
use chrono::format::strftime::StrftimeItems;
use chrono::format::DelayedFormat;
use chrono::{DateTime, Local};
use flexi_logger::{
    style, AdaptiveFormat, Cleanup, Criterion, DeferredNow, Duplicate, LogSpecBuilder,
    LogSpecification, Logger, Naming, Record,
};
use std::env;
use std::path::Path;

pub use flexi_logger::{Age, LoggerHandle};

#[allow(clippy::useless_conversion)]
fn log_format_date(now: &mut DeferredNow) -> DelayedFormat<StrftimeItems> {
    //use DateTime::<Local> instead of DateTime::<UTC> to obtain local date
    let local_date = DateTime::<Local>::from(*now.now());

    //format date as following: 2020-08-27T07:56:22.348+02:00 (local date + time zone with milliseconds precision)
    const DATE_FORMAT_STR: &str = "%Y-%m-%dT%H:%M:%S%.3f%z";
    local_date.format(DATE_FORMAT_STR)
}

#[cfg(not(feature = "packet-trace-enable"))]
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

#[cfg(feature = "packet-trace-enable")]
fn log_format(
    w: &mut dyn std::io::Write,
    _now: &mut DeferredNow,
    record: &Record,
) -> Result<(), std::io::Error> {
    write!(w, "[packet-trace]{}", record.args(),)
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

#[derive(Debug)]
struct FileLogConfig<'a> {
    dir: &'a Path,
    keep_compressed: usize,
    keep_uncompressed: usize,
    rotate_age: Age,
    rotate_size: Option<u64>,
}

impl<'a> FileLogConfig<'a> {
    fn with_env(dir: &'a Path, messages: &mut Vec<(log::Level, String)>) -> Self {
        let mut config = Self::new(dir);

        if let Ok(uncompressed) = env::var("LOG_FILES_UNCOMPRESSED") {
            match uncompressed.parse::<usize>() {
                Ok(n) => config.keep_uncompressed = n,
                Err(e) => messages.push((log::Level::Error, format!(
                    "LOG_FILES_UNCOMPRESSED ({uncompressed}) doesn't contain a valid nonnegative integer: {e}"
                ))),
            }
        }
        if let Ok(compressed) = env::var("LOG_FILES_COMPRESSED") {
            match compressed.parse::<usize>() {
                Ok(n) => config.keep_compressed = n,
                Err(e) => messages.push((log::Level::Error, format!(
                    "LOG_FILES_COMPRESSED ({compressed}) doesn't contain a valid nonnegative integer: {e}"
                ))),
            }
        }
        if let Ok(age) = env::var("LOG_ROTATE_AGE") {
            match age.to_ascii_lowercase().as_str() {
                "day" => config.rotate_age = Age::Day,
                "hour" => config.rotate_age = Age::Hour,
                _ => messages.push((
                    log::Level::Error,
                    format!("LOG_ROTATE_AGE ({age}) is neither DAY nor HOUR nor unset"),
                )),
            }
        }
        if let Ok(size) = env::var("LOG_ROTATE_SIZE") {
            match size.parse::<u64>() {
                Ok(n) => config.rotate_size = Some(n),
                Err(e) => messages.push((
                    log::Level::Error,
                    format!(
                        "LOG_ROTATE_SIZE ({size}) doesn't contain a valid positive integer: {e}"
                    ),
                )),
            }
        }

        config
    }

    fn new(dir: &'a Path) -> Self {
        FileLogConfig {
            dir,
            keep_compressed: 10,
            keep_uncompressed: 1,
            rotate_age: Age::Day,
            rotate_size: None,
        }
    }
}

fn set_logging_to_files(logger: Logger, config: FileLogConfig) -> Logger {
    let rotate_criterion = if let Some(rotate_size) = config.rotate_size {
        Criterion::AgeOrSize(config.rotate_age, rotate_size)
    } else {
        Criterion::Age(config.rotate_age)
    };

    logger
        .log_to_file()
        .directory(config.dir)
        .rotate(
            rotate_criterion,
            Naming::Timestamps,
            Cleanup::KeepLogAndCompressedFiles(config.keep_uncompressed, config.keep_compressed),
        )
        .print_message()
        .duplicate_to_stderr(Duplicate::All)
}

pub fn start_logger(
    default_log_spec: &str,
    log_dir: Option<&Path>,
    module_filters: &[(&str, log::LevelFilter)],
    force_debug: bool,
) -> Result<LoggerHandle> {
    let log_spec = LogSpecification::env_or_parse(default_log_spec)?;
    let mut log_spec_builder = LogSpecBuilder::from_module_filters(log_spec.module_filters());
    for filter in module_filters {
        log_spec_builder.module(filter.0, filter.1);
    }

    //override default log level if force_debug is set
    //it leaves module_filters log levels unchanged
    //used for --debug option
    if force_debug {
        log_spec_builder.default(log::LevelFilter::Debug);
    }

    let log_spec = log_spec_builder.finalize();
    let mut logger = Logger::with(log_spec).format(log_format);

    let mut messages = Vec::new();
    if let Some(log_dir) = log_dir {
        let config = FileLogConfig::with_env(log_dir, &mut messages);
        messages.push((
            log::Level::Info,
            format!("Logging configuration: {config:#?}"),
        ));

        logger = set_logging_to_files(logger, config);
    }
    logger = logger
        .adaptive_format_for_stderr(AdaptiveFormat::Custom(log_format, log_format_color))
        .set_palette("9;11;2;7;8".to_string());

    let handle = logger.start()?;

    for (level, text) in messages {
        log::log!(level, "{text}");
    }

    Ok(handle)
}
