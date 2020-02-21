mod transfers;
mod transfer_protocol;
mod transfer_http;
mod transfer_local;

pub use transfer_protocol::TransferProtocol;
pub use transfers::Transfers;

pub use transfer_http::HttpTransfer;
pub use transfer_local::LocalTransfer;
