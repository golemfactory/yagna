use humantime::format_duration;
use std::fmt::{Error, Formatter};

use ya_agreement_utils::agreement::flatten_value;
use ya_client::model::market::NewOffer;

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

impl<'a> std::fmt::Display for DisplayEnabler<'a, NewOffer> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        let mut offer = self.0.clone();
        offer.properties = flatten_value(offer.properties);

        // Display not pretty version as fallback.
        match serde_json::to_string_pretty(&offer) {
            Ok(json) => write!(f, "{}", json),
            Err(_) => write!(f, "{:?}", offer),
        }
    }
}
