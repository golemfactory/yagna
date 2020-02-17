use crate::commands::*;
use crate::error::ChannelError;
use crate::Result;
use actix::prelude::*;
use crossbeam_channel::bounded;
use std::path::PathBuf;

pub trait Runtime: Actor<Context = SyncContext<Self>> + Handler<Shutdown> {
    fn new(config_path: Option<PathBuf>, work_dir: PathBuf, cache_dir: PathBuf) -> Self;
}

pub(crate) struct RuntimeThread<R: Runtime> {
    pub handle: std::thread::JoinHandle<()>,
    pub addr: Addr<R>,
}

pub(crate) trait RuntimeThreadExt<R: Runtime> {
    fn flatten_addr(&self) -> Option<Addr<R>>;
}

impl<R: Runtime> RuntimeThread<R> {
    pub fn spawn<F>(factory: F) -> Result<Self>
    where
        F: Fn() -> R + Send + Sync + 'static,
    {
        let (tx, rx) = bounded::<Addr<R>>(1);

        let handle = std::thread::spawn(move || {
            let sys = System::new("runtime");
            let addr = SyncArbiter::start(1, factory);
            tx.send(addr).expect("Channel error");
            sys.run().expect("actix::System run failed");
        });

        let addr = rx.recv().map_err(ChannelError::from)?;
        Ok(RuntimeThread { handle, addr })
    }
}

impl<R: Runtime> RuntimeThreadExt<R> for Option<RuntimeThread<R>> {
    fn flatten_addr(&self) -> Option<Addr<R>> {
        match &self {
            Some(runtime) => Some(runtime.addr.clone()),
            None => None,
        }
    }
}
