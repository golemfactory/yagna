use std::fmt::{Error, Formatter};

use crate::db::model::{AgreementId, SubscriptionId};

use ya_service_api_web::middleware::Identity;

pub struct DisplayEnabler<'a, Type>(pub &'a Type);

pub trait EnableDisplay<Type> {
    fn display(&self) -> DisplayEnabler<Type>;
}

impl<Type> EnableDisplay<Type> for Type {
    fn display(&self) -> DisplayEnabler<Type> {
        DisplayEnabler(self)
    }
}

impl<'a> std::fmt::Display for DisplayEnabler<'a, SubscriptionId> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        self.0.fmt(f)
    }
}

impl<'a> std::fmt::Display for DisplayEnabler<'a, AgreementId> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        self.0.fmt(f)
    }
}

impl<'a> std::fmt::Display for DisplayEnabler<'a, Identity> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        write!(f, "'{}' [{}]", &self.0.name, &self.0.identity)
    }
}

impl<'a, Type> std::fmt::Display for DisplayEnabler<'a, Option<Type>>
where
    Type: std::fmt::Display,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        match &self.0 {
            Some(id) => id.fmt(f),
            // TODO: Someone funny could set appSessionId to "None" string.
            None => write!(f, "None"),
        }
    }
}
