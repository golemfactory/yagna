use log::{Level, Record};
use std::fmt::{Debug, Display};

pub use std::format_args;

pub trait LogErr<T, E: Debug + Display> {
    /// If Result is `Err`, this function logs it on error level
    /// and returns the same Result. In case of `Ok` nothing happens.
    fn log_err(self) -> Result<T, E>;
    fn log_warn(self) -> Result<T, E>;
    fn log_err_msg(self, message: &str) -> Result<T, E>;
    fn log_warn_msg(self, message: &str) -> Result<T, E>;

    fn log_error(self, message: &str, log_level: Level) -> Result<T, E>;
}

impl<T, E: Debug + Display> LogErr<T, E> for Result<T, E> {
    #[track_caller]
    fn log_err(self) -> Result<T, E> {
        self.log_err_msg("")
    }

    #[track_caller]
    fn log_warn(self) -> Result<T, E> {
        self.log_warn_msg("")
    }

    #[track_caller]
    fn log_err_msg(self, message: &str) -> Result<T, E> {
        self.log_error(message, Level::Error)
    }

    #[track_caller]
    fn log_warn_msg(self, message: &str) -> Result<T, E> {
        self.log_error(message, Level::Warn)
    }

    #[track_caller]
    fn log_error(self, message: &str, log_level: Level) -> Result<T, E> {
        if let Err(e) = self {
            // It will return file not module path, so it will differ from the original log macro.
            let module = std::panic::Location::caller().file().to_string();
            let module = module.replace('\\', "/");
            log(&module, log_level, message, &e);
            Err(e)
        } else {
            self
        }
    }
}

fn log<E: Debug + Display>(module: &str, level: Level, message: &str, e: &E) {
    let mut msg = message;
    if msg.is_empty() {
        msg = "Error occurred";
    }

    log::logger().log(
        &Record::builder()
            .level(level)
            .args(format_args!("{}: {}", msg, e))
            .module_path(Some(module))
            .target(module)
            .build(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;

    #[test]
    fn test_log_err_and_warn() {
        testing_logger::setup();

        let _result: anyhow::Result<()> = Err(anyhow!("Message-Message")).log_err();

        testing_logger::validate(|captured_logs| {
            let last = captured_logs.last().unwrap();
            assert_eq!(last.body, "Error occurred: Message-Message");
            assert_eq!(last.level, Level::Error);
            assert_eq!(last.target, "utils/std-utils/src/result.rs");
        });

        let _result: anyhow::Result<()> = Err(anyhow!("Message-Message")).log_warn_msg("Warning");

        testing_logger::validate(|captured_logs| {
            let last = captured_logs.last().unwrap();
            assert_eq!(last.body, "Warning: Message-Message");
            assert_eq!(last.level, Level::Warn);
            assert_eq!(last.target, "utils/std-utils/src/result.rs");
        });
    }
}
