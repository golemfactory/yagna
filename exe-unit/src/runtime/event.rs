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

use crate::message::{CommandContext, RuntimeEvent};

use ya_client_model::activity::CommandOutput;
use ya_runtime_api::server::{ProcessStatus, RuntimeStatus};

#[derive(Default, Clone)]
pub(crate) struct EventMonitor {
    processes: Arc<Mutex<HashMap<u64, Channel>>>,
    fallback: Arc<Mutex<Option<Channel>>>,
}

impl EventMonitor {
    pub fn any_process<'a>(&mut self, ctx: CommandContext) -> Handle<'a> {
        let mut inner = self.fallback.lock().unwrap();
        inner.replace(Channel::simple(ctx));

        Handle::Fallback {
            monitor: self.clone(),
        }
    }

    pub fn process<'a>(&mut self, ctx: CommandContext, pid: u64) -> Handle<'a> {
        let entry = Channel::new(ctx);
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

impl ya_runtime_api::server::RuntimeHandler for EventMonitor {
    fn on_process_status<'a>(&self, status: ProcessStatus) -> BoxFuture<'a, ()> {
        let (mut ctx, done_tx) = {
            let mut proc_map = self.processes.lock().unwrap();
            let mut fallback = self.fallback.lock().unwrap();

            let entry = match proc_map.get_mut(&status.pid).or(fallback.as_mut()) {
                Some(entry) => entry,
                None => return futures::future::ready(()).boxed(),
            };
            let done_tx = status.running.not().then(|| entry.done_tx()).flatten();
            (entry.ctx.clone(), done_tx)
        };

        async move {
            if !status.stdout.is_empty() {
                let out = CommandOutput::Bin(status.stdout);
                let evt = RuntimeEvent::stdout(ctx.batch_id.clone(), ctx.idx, out);
                ctx.tx.send(evt).await?;
            }
            if !status.stderr.is_empty() {
                let out = CommandOutput::Bin(status.stderr);
                let evt = RuntimeEvent::stderr(ctx.batch_id, ctx.idx, out);
                ctx.tx.send(evt).await?;
            }
            if let Some(done_tx) = done_tx {
                let _ = done_tx.send(status.return_code);
            }
            Ok::<_, SendError>(())
        }
        .map_err(|err| log::error!("Event channel error: {:?}", err))
        .then(|_| async {})
        .boxed()
    }

    fn on_runtime_status<'a>(&self, status: RuntimeStatus) -> BoxFuture<'a, ()> {
        use ya_runtime_api::server::proto::response::runtime_status::Kind;

        let mut ctx = {
            let channel = self.fallback.lock().unwrap();
            match channel.as_ref() {
                Some(c) => c.ctx.clone(),
                None => return futures::future::ready(()).boxed(),
            }
        };

        async move {
            let evt = match status.kind {
                Some(Kind::State(state)) => RuntimeEvent::State {
                    name: state.name,
                    value: serde_json::from_slice(&state.value[..])
                        .map_err(|e| log::warn!("Invalid runtime state value: {:?}", e))
                        .ok(),
                },
                Some(Kind::Counter(counter)) => RuntimeEvent::Counter {
                    name: counter.name,
                    value: counter.value,
                },
                evt => {
                    log::warn!("Unsupported runtime event: {:?}", evt);
                    return Ok::<_, SendError>(());
                }
            };

            ctx.tx.send(evt).await?;
            Ok::<_, SendError>(())
        }
        .map_err(|err| log::error!("Event channel error: {:?}", err))
        .then(|_| async {})
        .boxed()
    }
}

pub(crate) enum Handle<'a> {
    Process {
        monitor: EventMonitor,
        pid: u64,
        done_rx: BoxFuture<'a, Result<i32, ()>>,
    },
    Fallback {
        #[allow(unused)]
        monitor: EventMonitor,
    },
}

impl<'a> Drop for Handle<'a> {
    fn drop(&mut self) {
        match self {
            Handle::Process { monitor, pid, .. } => {
                monitor.processes.lock().unwrap().remove(pid);
            }
            _ => {
                // ignore
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
    fn new(ctx: CommandContext) -> Self {
        Channel {
            ctx,
            done: Some(Default::default()),
        }
    }

    fn simple(ctx: CommandContext) -> Self {
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
