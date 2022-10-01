use log::{Level, Record};
use std::fmt::{Debug, Display};

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
            backtrace::trace(|frame| {
                let mut cont = true;
                backtrace::resolve_frame(frame, |symbol| {
                    if let Some(name) = symbol.name() {
                        let module_path = name.to_string();
                        if module_path.starts_with("<ya") {
                            let module_path = module_path.strip_prefix('<').unwrap();
                            let module_path = module_path.split(" as ").next().unwrap();
                            let mut msg = message;
                            if msg.len() == 0 {
                                msg = "Error occurred";
                            }
                            log::logger().log(
                                &Record::builder()
                                    .level(log_level)
                                    .args(format_args!("{}: {}", msg, e))
                                    .module_path(Some(module_path))
                                    .build(),
                            );
                            cont = false
                        }
                    }
                });
                cont
            });
            Err(e)
        } else {
            self
        }
    }
}
