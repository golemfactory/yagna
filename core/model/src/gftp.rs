use ya_service_bus::RpcMessage;

use serde::{Deserialize, Serialize};
use thiserror::Error;


pub fn file_bus_id(hash: &str) -> String {
    format!("/public/gftp/{}", hash)
}


#[derive(Clone, Debug, Serialize, Deserialize, Error)]
pub enum Error {
    #[error("Can't read from file. {0}")]
    ReadError(String),
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
    pub file_size: u64,
}

/// Gets chunk of file. Returns GftpChunk.
/// Result chunk can be smaller if offset + size exceeds end of file.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetChunk {
    pub offset: u64,
    pub size: u64,
}

impl RpcMessage for GetChunk {
    const ID: &'static str = "GetChunk";
    type Item = GftpChunk;
    type Error = Error;
}

/// Sends chunk of file.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadChunk {
    chunk: GftpChunk,
}

impl RpcMessage for UploadChunk {
    const ID: &'static str = "UploadChunk";
    type Item = ();
    type Error = Error;
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GftpChunk {
    pub offset: u64,
    #[serde(with = "serde_bytes")]
    pub content: Vec<u8>,
}
