use serde::{Deserialize, Serialize};
use std::io::Write;
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

    pub fn response(id: Option<&RpcId>, result: RpcResult) -> Self {
        RpcMessage {
            jsonrpc: JSON_RPC_VERSION.to_string(),
            id: id.cloned(),
            body: RpcBody::Result { result },
        }
    }

    pub fn benchmark_response(id: Option<&RpcId>, url: Url) -> Self {
        Self::response(id, RpcResult::Benchmark(RpcBenchmarkResult { url }))
    }

    pub fn file_response(id: Option<&RpcId>, file: PathBuf, url: Url) -> Self {
        Self::response(id, RpcResult::File(RpcFileResult { file, url }))
    }

    pub fn files_response(id: Option<&RpcId>, items: Vec<(PathBuf, Url)>) -> Self {
        let items = items
            .into_iter()
            .map(|(file, url)| RpcFileResult { file, url })
            .collect();
        Self::response(id, RpcResult::Files(items))
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

    pub fn request_error(id: Option<&RpcId>) -> Self {
        Self::error(id, JsonRpcError::InvalidRequest)
    }

    pub fn validate(&self) -> Result<(), JsonRpcError> {
        if self.jsonrpc.as_str() != JSON_RPC_VERSION {
            return Err(JsonRpcError::InvalidRequest);
        }
        Ok(())
    }

    pub fn print(&self, verbose: bool) {
        let mut stdout = std::io::stdout();
        let json = match verbose {
            true => serde_json::to_string(self).unwrap(),
            false => serde_json::to_string(&self.body).unwrap(),
        };
        let _ = stdout.write_fmt(format_args!("{}\r\n", json));
        let _ = stdout.flush();
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum RpcId {
    Int(i64),
    Float(f64),
    String(String),
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
#[serde(rename_all = "snake_case")]
pub enum RpcBody {
    Request {
        #[serde(flatten)]
        request: RpcRequest,
    },
    Result {
        result: RpcResult,
    },
    Error {
        error: RpcError,
    },
}

#[derive(Serialize, Deserialize, StructOpt, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct BenchmarkOpt {
    #[structopt()]
    pub url: Url,
    #[structopt(short = "b", long)]
    pub max_bytes: Option<u64>,
    #[structopt(short = "t", long)]
    pub max_time_sec: Option<u32>,
    #[structopt(short = "u", long, default_value = "12")]
    pub chunk_at_once: u32,
    #[structopt(short = "c", long, default_value = "40960")]
    pub chunk_size: u64,
    #[structopt(short = "s", long, default_value = "2.0")]
    pub refresh_every_sec: f64,
}

#[derive(Serialize, Deserialize, StructOpt, Debug, Clone)]
pub enum BenchmarkCommands {
    /// Run at one node to enable benchmarking from other nodes
    Publish,
    /// Run if already enabled benchmark server on other node
    Download(BenchmarkOpt),
}

#[derive(Serialize, Deserialize, StructOpt, Debug, Clone)]
#[serde(tag = "method", content = "params")]
#[serde(rename_all = "snake_case")]
pub enum RpcRequest {
    /// Prints out version
    Version {},
    /// Publishes files (blocking)
    Publish { files: Vec<PathBuf> },
    /// Benchmark options
    #[structopt(name = "benchmark")]
    Benchmark(BenchmarkCommands),
    /// Stops publishing a file
    Close { urls: Vec<Url> },
    /// Downloads a file
    Download {
        /// Source URL
        url: Url,
        /// Destination path
        output_file: PathBuf,
    },
    /// Waits for file upload (blocking)
    Receive {
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
    /// Shuts down the server
    Shutdown {},
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
pub enum RpcResult {
    String(String),
    Benchmark(RpcBenchmarkResult),
    File(RpcFileResult),
    Files(Vec<RpcFileResult>),
    Status(RpcStatusResult),
    Statuses(Vec<RpcStatusResult>),
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum RpcStatusResult {
    Ok,
    Error,
}

impl From<bool> for RpcStatusResult {
    fn from(b: bool) -> Self {
        match b {
            true => RpcStatusResult::Ok,
            false => RpcStatusResult::Error,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub struct RpcBenchmarkResult {
    pub url: Url,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub struct RpcFileResult {
    pub file: PathBuf,
    pub url: Url,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub struct RpcError {
    pub code: i32,
    pub message: String,
}
