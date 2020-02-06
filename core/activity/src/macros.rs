#[macro_export]
macro_rules! db_conn {
    ($db_executor:expr) => {{
        use crate::error::Error;
        $db_executor.conn().map_err(Error::from)
    }};
}

#[macro_export]
macro_rules! impl_restful_handler {
    ($method:ident, $($a:ident),*) => {
        move |d, $($a),*| $method(d, $($a),*).map(crate::common::into_json_response)
    };
}
