use humantime::format_duration;
use std::fmt::{Error, Formatter};

pub struct DisplayEnabler<'a, Type>(pub &'a Type);

pub trait EnableDisplay<Type> {
    fn display(&self) -> DisplayEnabler<Type>;
}

impl<Type> EnableDisplay<Type> for Type {
    fn display(&self) -> DisplayEnabler<Type> {
        DisplayEnabler(self)
    }
}

impl<'a> std::fmt::Display for DisplayEnabler<'a, chrono::Duration> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        match self.0.clone().to_std() {
            Ok(duration) => write!(f, "{}", format_duration(duration)),
            // If we can't convert, we will display ugly version, which we wanted to avoid.
            Err(_) => write!(f, "{}", self.0),
        }
    }
}
