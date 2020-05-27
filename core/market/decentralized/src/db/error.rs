use ya_persistence::executor::Error as DbError;

pub type DbResult<T> = Result<T, DbError>;
