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

#[macro_export]
macro_rules! bind_gsb_method {
    ($service_id:expr, $db:expr, $fn:ident) => {{
        use ya_service_bus::typed as bus;

        let db_ = $db.clone();
        let _ = bus::bind_with_caller(&$service_id, move |c, m| $fn(db_.clone(), c, m));
    }};
}

#[macro_export]
macro_rules! gsb_send {
    ($caller:expr, $msg:expr, $uri:expr, $timeout:expr) => {{
        use ya_service_bus::actix_rpc;
        use ya_service_bus::timeout::*;

        // TODO: this is not enough for the net service, bc it does not contain caller addr
        actix_rpc::service($uri)
            .send($caller, $msg)
            .timeout($timeout)
            .map_err(Error::from)
            .await?
            .map_err(Error::from)?
            .map_err(Error::from)
    }};
}
