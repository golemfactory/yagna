#[macro_export]
macro_rules! json_response_future {
    ($future:expr) => {
        $future.map(crate::common::into_json_response)
    };
}

#[macro_export]
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
