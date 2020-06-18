use std::future::Future;
use std::pin::Pin;

mod proto {
    include!(concat!(env!("OUT_DIR"), "/ya_runtime_api.rs"));
}

pub type DynFuture<T> = Pin<Box<dyn Future<Output=T>>>;
pub use proto::response::Error as ErrorResponse;
pub use proto::request::{RunProcess, KillProcess};
pub use proto::response::RunProcess as RunProcessResp;
pub use proto::response::{ProcessStatus, ErrorCode};

pub trait RuntimeService {

    fn hello(&self, version : &str);

    fn run_process(&self, run : RunProcess) -> DynFuture<Result<RunProcessResp, ErrorResponse>>;

    fn kill_process(&self, kill : KillProcess) -> DynFuture<Result<(), ErrorResponse>>;

}

pub trait RuntimeEvent {

    fn on_process_status(&self, status : ProcessStatus);

}