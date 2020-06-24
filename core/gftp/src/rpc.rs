use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;
use structopt::StructOpt;
use url::Url;

const JSON_RPC_VERSION: &str = "2.0";

#[allow(unused)]
#[derive(Debug)]
#[repr(i32)]
pub enum JsonRpcError {
    /// Invalid JSON was received by the server.
    ParseError = -32700,
    /// The JSON sent is not a valid Request object.
    InvalidRequest = -32600,
    /// The method does not exist / is not available.
    MethodNotFound = -32601,
    /// Invalid method parameter(s).
    InvalidParams = -32602,
    /// Internal JSON-RPC error.
    InternalError = -32603,
    /// Server error
    ServerError = -32000,
}

impl ToString for JsonRpcError {
    fn to_string(&self) -> String {
        format!("{:?}", self)
    }
}

impl From<serde_json::Error> for JsonRpcError {
    fn from(e: serde_json::Error) -> Self {
        use serde_json::error::Category;
        match e.classify() {
            Category::Data => JsonRpcError::InvalidRequest,
            Category::Eof | Category::Io | Category::Syntax => JsonRpcError::ParseError,
        }
    }
}

impl From<std::io::Error> for JsonRpcError {
    fn from(_: std::io::Error) -> Self {
        JsonRpcError::ServerError
    }
}

impl From<anyhow::Error> for JsonRpcError {
    fn from(_: anyhow::Error) -> Self {
        JsonRpcError::ServerError
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RpcMessage {
    pub jsonrpc: String,
    pub id: Option<RpcId>,
    #[serde(flatten)]
    pub body: RpcBody,
}

impl RpcMessage {
    pub fn request(id: Option<&RpcId>, request: RpcRequest) -> Self {
        RpcMessage {
            jsonrpc: JSON_RPC_VERSION.to_string(),
            id: id.cloned(),
            body: RpcBody::Request { request },
        }
    }

    pub fn response(id: Option<&RpcId>, file: PathBuf, url: Url) -> Self {
        RpcMessage {
            jsonrpc: JSON_RPC_VERSION.to_string(),
            id: id.cloned(),
            body: RpcBody::Response {
                response: RpcResponse { file, url },
            },
        }
    }

    pub fn error<E: Into<JsonRpcError> + ToString>(id: Option<&RpcId>, err: E) -> Self {
        let message = err.to_string();
        RpcMessage {
            jsonrpc: JSON_RPC_VERSION.to_string(),
            id: id.cloned(),
            body: RpcBody::Error {
                error: RpcError {
                    message,
                    code: err.into() as i32,
                },
            },
        }
    }

    pub fn type_error(id: Option<&RpcId>) -> Self {
        RpcMessage {
            jsonrpc: JSON_RPC_VERSION.to_string(),
            id: id.cloned(),
            body: RpcBody::Error {
                error: RpcError {
                    message: "invalid message type".to_string(),
                    code: JsonRpcError::InvalidRequest as i32,
                },
            },
        }
    }

    pub fn validate(&self) -> Result<(), JsonRpcError> {
        if self.jsonrpc.as_str() != JSON_RPC_VERSION {
            return Err(JsonRpcError::InvalidRequest);
        }
        Ok(())
    }

    pub fn print(&self, verbose: bool) {
        match verbose {
            true => print!("{}", self),
            false => match &self.body {
                RpcBody::Request { .. } => (),
                body => print!("{}", body),
            },
        }
    }
}

impl fmt::Display for RpcMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}\n\0", serde_json::to_string_pretty(self).unwrap())
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum RpcId {
    Int(i32),
    Float(f32),
    String(String),
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
#[serde(rename_all = "lowercase")]
pub enum RpcBody {
    Request {
        #[serde(flatten)]
        request: RpcRequest,
    },
    Response {
        response: RpcResponse,
    },
    Error {
        error: RpcError,
    },
}

impl fmt::Display for RpcBody {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}\n\0", serde_json::to_string_pretty(self).unwrap())
    }
}

#[derive(Serialize, Deserialize, StructOpt, Debug, Clone)]
#[serde(tag = "method", content = "params")]
#[serde(rename_all = "lowercase")]
pub enum RpcRequest {
    /// Publishes files (blocking)
    Publish { files: Vec<PathBuf> },
    /// Downloads a file
    Download {
        /// Source URL
        url: Url,
        /// Destination path
        output_file: PathBuf,
    },
    /// Waits for file upload (blocking)
    AwaitUpload {
        /// Destination path
        output_file: PathBuf,
    },
    /// Uploads a file
    Upload {
        /// Destination URL
        url: Url,
        /// Source path
        file: PathBuf,
    },
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RpcResponse {
    pub file: PathBuf,
    pub url: Url,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
}
