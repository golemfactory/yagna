pub trait LogErr<T, E: std::error::Error> {
    /// If Result is `Err`, this function logs it on error level
    /// and returns the same Result. In case of `Ok` nothing happens.
    fn log_err(self) -> Result<T, E>;
}

impl<T, E: std::error::Error> LogErr<T, E> for Result<T, E> {
    fn log_err(self) -> Result<T, E> {
        match self {
            Err(e) => {
                backtrace::trace(|frame| {
                    let mut cont = true;
                    backtrace::resolve_frame(frame, |symbol| {
                        if let Some(name) = symbol.name().map(|s| s.to_string()) {
                            if name.starts_with("<ya") {
                                let name = name.strip_prefix("<").unwrap();
                                let name = name.split(" as ").next().unwrap();
                                log::error!("Error at {}: {}", name, &e);
                                cont = false
                            }
                        }
                    });
                    cont
                });
                Err(e)
            }
            _ => self,
        }
    }
}
