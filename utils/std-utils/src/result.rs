use std::fmt::Display;

pub trait ResultExt<T, E> {
    /// If Result is `Err`, this function logs it on error level
    /// and returns the same Result. In case of `Ok` nothing happens.
    fn log_err(self) -> Result<T, E>;
}

impl<T, E> ResultExt<T, E> for Result<T, E>
where
    E: Display,
{
    fn log_err(self) -> Result<T, E> {
        match self {
            Ok(content) => Ok(content),
            Err(e) => {
                log::error!("{}", &e);
                Err(e)
            }
        }
    }
}
