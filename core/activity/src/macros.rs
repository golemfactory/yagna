#[macro_export]
macro_rules! db_conn {
    ($db_executor:expr) => {{
        use crate::error::Error;
        $db_executor.conn().map_err(Error::from)
    }};
}
