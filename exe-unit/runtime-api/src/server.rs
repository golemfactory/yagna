pub mod proto {
    include!(concat!(env!("OUT_DIR"), "/ya_runtime_api.rs"));

    impl response::Error {
        pub fn msg<D: std::fmt::Display>(msg: D) -> Self {
            let mut e = Self::default();
            e.set_code(response::ErrorCode::Internal);
            e.message = msg.to_string();
            e
        }
    }
}
mod codec;

#[cfg(feature = "codec")]
pub use codec::Codec;
pub use proto::request::{CreateNetwork, KillProcess, RunProcess};
pub use proto::response::create_network::Endpoint as NetworkEndpoint;
pub use proto::response::runtime_status::Counter as RuntimeCounter;
pub use proto::response::runtime_status::Kind as RuntimeStatusKind;
pub use proto::response::runtime_status::State as RuntimeState;
pub use proto::response::CreateNetwork as CreateNetworkResp;
pub use proto::response::Error as ErrorResponse;
pub use proto::response::RunProcess as RunProcessResp;
pub use proto::response::{ErrorCode, ProcessStatus, RuntimeStatus};
pub use proto::{Network, NetworkInterface};

use futures::future::{BoxFuture, LocalBoxFuture};
use futures::prelude::*;
use std::process::{ExitStatus, Stdio};
use std::sync::Arc;
use tokio::process;

pub type AsyncResponse<'a, T> = LocalBoxFuture<'a, Result<T, ErrorResponse>>;

/// Service interface
pub trait RuntimeService {
    /// Perform version handshake
    fn hello(&self, version: &str) -> AsyncResponse<'_, String>;
    /// Spawn a process
    fn run_process(&self, run: RunProcess) -> AsyncResponse<'_, RunProcessResp>;
    /// Kill a spawned process
    fn kill_process(&self, kill: KillProcess) -> AsyncResponse<'_, ()>;
    /// Setup a virtual private network
    fn create_network(&self, network: CreateNetwork) -> AsyncResponse<'_, CreateNetworkResp>;
    /// Perform service shutdown
    fn shutdown(&self) -> AsyncResponse<'_, ()>;
}

/// Process and internal event handler
pub trait RuntimeHandler {
    /// Process event handler
    fn on_process_status<'a>(&self, status: ProcessStatus) -> BoxFuture<'a, ()>;
    /// Runtime event handler
    fn on_runtime_status<'a>(&self, status: RuntimeStatus) -> BoxFuture<'a, ()>;
}

/// Runtime control interface
pub trait RuntimeControl {
    /// Return runtime process id
    fn id(&self) -> u32;
    /// Stop the runtime
    fn stop(&self);
    /// Return a future, resolved when the runtime is stopped
    fn stopped(&self) -> BoxFuture<'_, i32>;
}

mod client;
mod service;

pub use client::spawn;
pub use service::{run, run_async};
