mod activity;
mod agreement;
mod event;

pub use activity::ActivityDao;
pub use agreement::{Agreement, AgreementDao};
pub use event::EventDao;

pub type Result<T> = std::result::Result<T, diesel::result::Error>;

pub trait InnerIntoOption<T> {
    fn inner_into_option(self) -> Result<Option<T>>;
}

pub trait FlattenInnerOption<T> {
    fn flatten_inner_option(self) -> Result<T>;
}

impl<T> InnerIntoOption<T> for Result<T> {
    fn inner_into_option(self) -> Result<Option<T>> {
        match self {
            Ok(t) => Ok(Some(t)),
            Err(e) => match e {
                diesel::result::Error::NotFound => Ok(None),
                _ => Err(e),
            },
        }
    }
}

impl<T> FlattenInnerOption<T> for Result<Option<T>> {
    fn flatten_inner_option(self) -> Result<T> {
        match self {
            Ok(option) => match option {
                Some(value) => Ok(value),
                None => Err(diesel::result::Error::NotFound),
            },
            Err(error) => Err(error),
        }
    }
}
