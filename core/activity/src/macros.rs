macro_rules! db_conn {
    ($db_executor:expr) => {{
        use crate::error::Error;
        $db_executor.lock().await.conn().map_err(Error::from)
    }};
}

macro_rules! bind_gsb_method {
    ($id:expr, $db_executor:expr, $method:ident) => {{
        use ya_service_bus::typed as bus;

        let db_ = $db_executor.clone();
        let _ = bus::bind(&$id, move |m| $method(db_.clone(), m));
    }};
}

macro_rules! gsb_send {
    ($msg:expr, $uri:expr, $timeout:expr) => {{
        use ya_service_bus::actix_rpc;
        use $crate::timeout::IntoTimeoutFuture;

        actix_rpc::service($uri)
            .send($msg)
            .timeout($timeout)
            .map_err(Error::from)
            .await?
            .map_err(Error::from)?
            .map_err(Error::from)
    }};
}

macro_rules! json_response_future {
    ($future:expr) => {
        $future.map(crate::common::into_json_response)
    };
}

macro_rules! impl_restful_handler {
    ($method:ident) => {
        move |d| json_response_future!($method(d))
    };
    ($method:ident, $t:ident) => {
        move |d, $t| json_response_future!($method(d, $t))
    };
    ($method:ident, $t:ident, $u:ident) => {
        move |d, $t, $u| json_response_future!($method(d, $t, $u))
    };
    ($method:ident, $t:ident, $u:ident, $v:ident) => {
        move |d, $t, $u, $v| json_response_future!($method(d, $t, $u, $v))
    };
}
