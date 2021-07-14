use std::future::Future;
use std::pin::Pin;
use std::process::{ExitStatus, Stdio};
use std::sync::Arc;

use futures::future::BoxFuture;
use futures::prelude::*;
use tokio::process;

pub use client::spawn;
#[cfg(feature = "codec")]
pub use codec::Codec;
pub use proto::request::{CreateNetwork, KillProcess, RunProcess};
pub use proto::response::create_network::Endpoint as NetworkEndpoint;
pub use proto::response::CreateNetwork as CreateNetworkResp;
pub use proto::response::Error as ErrorResponse;
pub use proto::response::RunProcess as RunProcessResp;
pub use proto::response::{ErrorCode, ProcessStatus};
pub use proto::Network;
pub use service::{run, run_async};

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

pub type DynFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;
pub type AsyncResponse<'a, T> = DynFuture<'a, Result<T, ErrorResponse>>;

pub trait RuntimeService {
    fn hello(&self, version: &str) -> AsyncResponse<'_, String>;

    fn run_process(&self, run: RunProcess) -> AsyncResponse<'_, RunProcessResp>;

    fn kill_process(&self, kill: KillProcess) -> AsyncResponse<'_, ()>;

    fn create_network(&self, network: CreateNetwork) -> AsyncResponse<'_, CreateNetworkResp>;

    fn shutdown(&self) -> AsyncResponse<'_, ()>;
}

pub trait RuntimeEvent {
    fn on_process_status<'a>(&self, _status: ProcessStatus) -> BoxFuture<'a, ()> {
        future::ready(()).boxed()
    }
}

pub trait RuntimeStatus {
    fn exited<'a>(&self) -> BoxFuture<'a, i32>;
}

pub trait ProcessControl {
    fn id(&self) -> u32;

    fn kill(&self);
}

mod client;
mod service;
