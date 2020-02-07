mod activity;
mod activity_state;
mod activity_usage;
mod event;

pub use activity::ActivityDao;
pub use activity_state::ActivityStateDao;
pub use activity_usage::ActivityUsageDao;
pub use event::{Event, EventDao};

type Result<T> = std::result::Result<T, diesel::result::Error>;

no_arg_sql_function!(last_insert_rowid, diesel::sql_types::Integer);

pub trait NotFoundAsOption<T> {
    fn not_found_as_option(self) -> Result<Option<T>>;
}

impl<T> NotFoundAsOption<T> for Result<T> {
    fn not_found_as_option(self) -> Result<Option<T>> {
        match self {
            Ok(t) => Ok(Some(t)),
            Err(e) => match e {
                diesel::result::Error::NotFound => Ok(None),
                _ => Err(e),
            },
        }
    }
}
