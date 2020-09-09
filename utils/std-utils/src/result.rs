pub trait ResultExt<T, E> {
    /// If Result is `Err`, this function logs it on error level
    /// and returns the same Result. In case of `Ok` nothing happens.
    fn inspect_err<F>(self, fun: F) -> Result<T, E>
    where
        F: FnOnce(&E);
}

impl<T, E> ResultExt<T, E> for Result<T, E> {
    fn inspect_err<F>(self, fun: F) -> Result<T, E>
    where
        F: FnOnce(&E),
    {
        if let Err(e) = self {
            fun(&e);
            Err(e)
        } else {
            self
        }
    }
}
