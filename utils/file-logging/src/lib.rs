use anyhow::Result;
use chrono::format::strftime::StrftimeItems;
use chrono::format::DelayedFormat;

use flexi_logger::{
    style, AdaptiveFormat, Age, Cleanup, Criterion, DeferredNow, Duplicate, LogSpecBuilder,
    LogSpecification, Logger, Naming, Record,
};
use std::path::Path;

pub use flexi_logger::LoggerHandle;

fn log_format_date(now: &mut DeferredNow) -> DelayedFormat<StrftimeItems> {
    //use DateTime::<Local> instead of DateTime::<UTC> to obtain local date
    let local_date = *now.now();

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
    if let Some(log_dir) = log_dir {
        logger = set_logging_to_files(logger, log_dir);
    }
    logger = logger
        .adaptive_format_for_stderr(AdaptiveFormat::Custom(log_format, log_format_color))
        .set_palette("9;11;2;7;8".to_string());

    Ok(logger.start()?)
}
