use crate::error::ChannelError;
use crate::message::*;
use crate::Result;
use actix::prelude::*;
use crossbeam_channel::bounded;
use std::path::PathBuf;

pub trait Runtime:
    Actor<Context = SyncContext<Self>> + Handler<ExecCmd> + Handler<Shutdown>
{
    const EXECUTABLE: &'static str;

    fn new(agreement: PathBuf, workdir: PathBuf, cachedir: PathBuf) -> Self;
}

pub(crate) struct RuntimeHandler<R: Runtime> {
    pub handle: std::thread::JoinHandle<()>,
    pub addr: Addr<R>,
}

pub(crate) trait RuntimeHandlerExt<R: Runtime> {
    fn flatten_addr(&self) -> Option<Addr<R>>;
}

impl<R: Runtime> RuntimeHandler<R> {
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
        Ok(RuntimeHandler { handle, addr })
    }
}

impl<R: Runtime> RuntimeHandlerExt<R> for Option<RuntimeHandler<R>> {
    fn flatten_addr(&self) -> Option<Addr<R>> {
        match &self {
            Some(runtime) => Some(runtime.addr.clone()),
            None => None,
        }
    }
}
