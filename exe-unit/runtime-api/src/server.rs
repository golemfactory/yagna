use std::future::Future;
use std::pin::Pin;

pub mod proto {
    include!(concat!(env!("OUT_DIR"), "/ya_runtime_api.rs"));

    impl response::Error {
        pub fn msg<D : std::fmt::Display>(msg : D) -> Self {
            let mut e = Self::default();
            e.set_code(response::ErrorCode::Internal);
            e.message = msg.to_string();
            e
        }
    }
}
mod codec;

pub use proto::request::{KillProcess, RunProcess};
pub use proto::response::Error as ErrorResponse;
pub use proto::response::RunProcess as RunProcessResp;
pub use proto::response::{ErrorCode, ProcessStatus};

pub type DynFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;
pub type AsyncResponse<'a, T> = DynFuture<'a, Result<T, ErrorResponse>>;

use futures::lock::Mutex;
use futures::prelude::*;
use futures::{Sink, SinkExt, Stream, StreamExt, TryStream, TryStreamExt};
use std::collections::HashMap;
use std::fmt::Debug;
use std::process::{Output, Stdio};
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::io;
use tokio::process;

pub trait RuntimeService {
    fn hello(&self, version: &str) -> AsyncResponse<'_, String>;

    fn run_process(&self, run: RunProcess) -> AsyncResponse<'_, RunProcessResp>;

    fn kill_process(&self, kill: KillProcess) -> AsyncResponse<'_, ()>;
}

pub trait RuntimeEvent {
    fn on_process_status(&self, status: ProcessStatus);
}

mod service;
mod client;

pub use service::run;
pub use client::spawn;

