use std::collections::HashMap;
use std::future::Future;
use std::ops::Not;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use futures::channel::mpsc::SendError;
use futures::channel::oneshot;
use futures::future::{BoxFuture, Fuse, Shared};
use futures::{FutureExt, SinkExt, TryFutureExt};

use crate::message::CommandContext;

use ya_client_model::activity::{CommandOutput, RuntimeEvent};
use ya_runtime_api::server::ProcessStatus;

#[derive(Default, Clone)]
pub(crate) struct EventMonitor {
    processes: Arc<Mutex<HashMap<u64, Channel>>>,
    fallback: Arc<Mutex<Option<Channel>>>,
}

impl EventMonitor {
    pub fn any_process<'a>(&mut self, ctx: CommandContext) -> Handle<'a> {
        let mut inner = self.fallback.lock().unwrap();
        inner.replace(Channel::plain(ctx));

        Handle::Fallback {
            monitor: self.clone(),
        }
    }

    pub fn process<'a>(&mut self, ctx: CommandContext, pid: u64) -> Handle<'a> {
        let entry = Channel::new(ctx, Default::default());
        let done_rx = entry.done_rx().unwrap();

        let mut inner = self.processes.lock().unwrap();
        inner.insert(pid, entry);

        Handle::Process {
            monitor: self.clone(),
            pid,
            done_rx,
        }
    }
}

impl ya_runtime_api::server::RuntimeEvent for EventMonitor {
    fn on_process_status<'a>(&self, status: ProcessStatus) -> BoxFuture<'a, ()> {
        let (ctx, done_tx) = {
            let mut proc_map = self.processes.lock().unwrap();
            let mut fallback = self.fallback.lock().unwrap();

            let entry = match proc_map.get_mut(&status.pid).or(fallback.as_mut()) {
                Some(entry) => entry,
                None => return futures::future::ready(()).boxed(),
            };
            let done_tx = status.running.not().then(|| entry.done_tx()).flatten();

            (entry.ctx.clone(), done_tx)
        };

        publish(status, ctx, done_tx)
            .map_err(|err| log::error!("Event channel error: {:?}", err))
            .then(|_| async {})
            .boxed()
    }
}

async fn publish(
    status: ProcessStatus,
    mut ctx: CommandContext,
    done_tx: Option<oneshot::Sender<i32>>,
) -> Result<(), SendError> {
    if !status.stdout.is_empty() {
        ctx.tx
            .send(RuntimeEvent::stdout(
                ctx.batch_id.clone(),
                ctx.idx,
                CommandOutput::Bin(status.stdout),
            ))
            .await?;
    }
    if !status.stderr.is_empty() {
        ctx.tx
            .send(RuntimeEvent::stderr(
                ctx.batch_id,
                ctx.idx,
                CommandOutput::Bin(status.stderr),
            ))
            .await?;
    }
    if let Some(done_tx) = done_tx {
        let _ = done_tx.send(status.return_code);
    }
    Ok(())
}

pub(crate) enum Handle<'a> {
    Process {
        monitor: EventMonitor,
        pid: u64,
        done_rx: BoxFuture<'a, Result<i32, ()>>,
    },
    Fallback {
        monitor: EventMonitor,
    },
}

impl<'a> Drop for Handle<'a> {
    fn drop(&mut self) {
        match self {
            Handle::Process { monitor, pid, .. } => {
                monitor.processes.lock().unwrap().remove(pid);
            }
            Handle::Fallback { monitor, .. } => {
                monitor.fallback.lock().unwrap().take();
            }
        }
    }
}

impl<'a> Future for Handle<'a> {
    type Output = i32;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.get_mut() {
            Handle::Process { done_rx, .. } => match Pin::new(done_rx).poll(cx) {
                Poll::Ready(Ok(c)) => Poll::Ready(c),
                Poll::Ready(Err(_)) => Poll::Ready(1),
                Poll::Pending => Poll::Pending,
            },
            Handle::Fallback { .. } => Poll::Ready(0),
        }
    }
}

struct Channel {
    ctx: CommandContext,
    done: Option<DoneChannel>,
}

impl Channel {
    fn new(ctx: CommandContext, done: DoneChannel) -> Self {
        Channel {
            ctx,
            done: Some(done),
        }
    }

    fn plain(ctx: CommandContext) -> Self {
        Channel { ctx, done: None }
    }

    fn done_tx(&mut self) -> Option<oneshot::Sender<i32>> {
        self.done.as_mut().map(|d| d.tx.take()).flatten()
    }

    fn done_rx<'a>(&self) -> Option<BoxFuture<'a, Result<i32, ()>>> {
        self.done
            .as_ref()
            .map(|d| d.rx.clone().map_err(|_| ()).boxed())
    }
}

struct DoneChannel {
    tx: Option<oneshot::Sender<i32>>,
    rx: Shared<Fuse<oneshot::Receiver<i32>>>,
}

impl Default for DoneChannel {
    fn default() -> Self {
        let (tx, rx) = oneshot::channel();
        Self {
            tx: Some(tx),
            rx: rx.fuse().shared(),
        }
    }
}
