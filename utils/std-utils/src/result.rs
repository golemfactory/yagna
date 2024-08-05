use log::{Level, Record};
use std::fmt::{Debug, Display};
use std::sync::atomic::{AtomicBool, Ordering};

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
    fn log_err(self) -> Result<T, E> {
        self.log_err_msg("")
    }

    fn log_warn(self) -> Result<T, E> {
        self.log_warn_msg("")
    }

    fn log_err_msg(self, message: &str) -> Result<T, E> {
        self.log_error(message, Level::Error)
    }

    fn log_warn_msg(self, message: &str) -> Result<T, E> {
        self.log_error(message, Level::Warn)
    }

    fn log_error(self, message: &str, log_level: Level) -> Result<T, E> {
        if let Err(e) = self {
            let std_utils_symbols = AtomicBool::new(false);
            let cont = AtomicBool::new(true);

            backtrace::trace(|frame| {
                backtrace::resolve_frame(frame, |symbol| {
                    if let Some(name) = symbol.name() {
                        let module = name.to_string();

                        // Skip all symbols from this library until we find first symbol of caller function.
                        if std_utils_symbols.load(Ordering::SeqCst)
                            && !module.contains("ya_std_utils::result::LogErr")
                        {
                            cont.store(false, Ordering::SeqCst);

                            // Find out the module path and print log.
                            let suffix = module.rfind("::");
                            if let Some(suffix) = suffix {
                                log(&module[..suffix], log_level, message, &e);
                            } else {
                                log(&module, log_level, message, &e);
                            }
                        }

                        // Mark first symbol of function called in this library.
                        if module.contains("ya_std_utils::result::LogErr") {
                            std_utils_symbols.store(true, Ordering::SeqCst);
                        }
                    }
                });
                cont.load(Ordering::SeqCst)
            });
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
    use testing_logger;

    #[test]
    fn test_log_err_and_warn() {
        testing_logger::setup();

        let _result: anyhow::Result<()> = Err(anyhow!("Message-Message")).log_err();

        testing_logger::validate(|captured_logs| {
            let last = captured_logs.last().unwrap();
            assert_eq!(last.body, "Error occurred: Message-Message");
            assert_eq!(last.level, Level::Error);
            assert_eq!(
                last.target,
                "ya_std_utils::result::tests::test_log_err_and_warn"
            );
        });

        let _result: anyhow::Result<()> = Err(anyhow!("Message-Message")).log_warn_msg("Warning");

        testing_logger::validate(|captured_logs| {
            let last = captured_logs.last().unwrap();
            assert_eq!(last.body, "Warning: Message-Message");
            assert_eq!(last.level, Level::Warn);
            assert_eq!(
                last.target,
                "ya_std_utils::result::tests::test_log_err_and_warn"
            );
        });
    }
}
