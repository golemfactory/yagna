use ya_service_bus::RpcMessage;

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub fn file_bus_id(hash: &str) -> String {
    format!("{}/gftp/{}", crate::net::PUBLIC_PREFIX, hash)
}

#[derive(Clone, Debug, Serialize, Deserialize, Error)]
pub enum Error {
    #[error("Can't read from file. {0}")]
    ReadError(String),
    #[error("Can't write to file. {0}")]
    WriteError(String),
    #[error("File hash verification failed.")]
    IntegrityError,
    #[error("Internal error: {0}.")]
    InternalError(String),
}

// =========================================== //
// Download messages
// =========================================== //

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
/// If offset is greater than file size, operation ends with error.
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

// =========================================== //
// Upload messages
// =========================================== //

/// Sends chunk of file.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadChunk {
    pub chunk: GftpChunk,
}

impl RpcMessage for UploadChunk {
    const ID: &'static str = "UploadChunk";
    type Item = ();
    type Error = Error;
}

/// Notifies file publisher that upload has been finished.
/// Uploader can send optional hash of file.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadFinished {
    pub hash: Option<String>,
}

impl RpcMessage for UploadFinished {
    const ID: &'static str = "UploadFinished";
    type Item = ();
    type Error = Error;
}

// =========================================== //
// Chunk structure
// =========================================== //

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GftpChunk {
    pub offset: u64,
    #[serde(with = "serde_bytes")]
    pub content: Vec<u8>,
}
