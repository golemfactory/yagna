pub mod error;
pub mod gsb_to_http;
mod headers;
pub mod http_to_gsb;
pub mod message;
pub mod response;

/*
Proxy http request through GSB
- create a HttpToGsbProxy
- pass a GsbHttpCallMessage
- receive the message and execute with GsbToHttpProxy
 */

pub const BUS_ID: &str = "/public/http-proxy";
