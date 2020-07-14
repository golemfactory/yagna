pub mod dao;
pub mod models; // TODO: remove plural form
pub mod schema;

pub use ya_persistence::executor::Error as DbError;
pub type DbResult<T> = Result<T, DbError>;
