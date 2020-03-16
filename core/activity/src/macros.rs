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
        log::debug!("sending: {:?}, uri: {}, caller: {:?}", $msg, $uri, $caller);

        actix_rpc::service($uri)
            .send(Some($caller.to_string()), $msg) // TODO: introduce automatic caller support
            .timeout($timeout).await??
    }};
}
