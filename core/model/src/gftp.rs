use ya_service_bus::RpcMessage;

use serde::{Deserialize, Serialize};
use thiserror::Error;



pub const BUS_ID: &'static str = "/public/gftp";


#[derive(Clone, Debug, Serialize, Deserialize, Error)]
pub enum Error {
    #[error("Can't read bytes.")]
    ReadError,
}


/// Gets metadata of file publish through gftp.
/// Returns GftpMetadata structure.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetMetadata;

impl RpcMessage for GetMetadata {
    const ID: &'static str = "GetMetadata";
    type Item = GftpMetadata;
    type Error = Error;
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GftpMetadata {
    pub chunks_num: u64,
    pub chunk_size: u64,
    pub file_size: u64,
    //TODO: Add necessary information
}

/// Gets chunk of file. Returns GftpChunk.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetChunk {
    pub chunk_number: u64,
}

impl RpcMessage for GetChunk {
    const ID: &'static str = "GetChunk";
    type Item = GftpChunk;
    type Error = Error;
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GftpChunk {
    #[serde(with = "serde_bytes")]
    pub content: Vec<u8>,
}

