use std::collections::HashMap;
use std::future::Future;
use std::ops::Not;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use futures::channel::mpsc::SendError;
use futures::channel::oneshot;
use futures::future::{BoxFuture, Shared};
use futures::{FutureExt, SinkExt, TryFutureExt};

use crate::message::{CommandContext, RuntimeEvent};

use ya_client_model::activity::CommandOutput;
use ya_runtime_api::server::{ProcessStatus, RuntimeStatus};

#[derive(Default, Clone)]
pub(crate) struct EventMonitor {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Default)]
struct Inner {
    next_process: Option<Channel>,
    processes: HashMap<u64, Channel>,
    fallback: Option<Channel>,
}

impl EventMonitor {
    pub fn any_process<'a>(&mut self, ctx: CommandContext) -> Handle<'a> {
        let mut inner = self.inner.lock().unwrap();
        inner.fallback.replace(Channel::fallback(ctx));

        Handle::Fallback {}
    }

    pub fn next_process<'a>(&mut self, ctx: CommandContext) -> Handle<'a> {
        let mut inner = self.inner.lock().unwrap();
        let channel = Channel::new(ctx, Default::default());
        let handle = Handle::process(self, &channel);
        inner.next_process.replace(channel);

        handle
    }

    #[allow(unused)]
    pub fn process<'a>(&mut self, ctx: CommandContext, pid: u64) -> Handle<'a> {
        let mut inner = self.inner.lock().unwrap();
        let channel = Channel::new(ctx, pid);
        let handle = Handle::process(self, &channel);
        inner.processes.insert(pid, channel);

        handle
    }
}

impl ya_runtime_api::server::RuntimeHandler for EventMonitor {
    fn on_process_status<'a>(&self, status: ProcessStatus) -> BoxFuture<'a, ()> {
        let running = status.running;
        let (mut ctx, done_tx) = {
            let mut inner = self.inner.lock().unwrap();

            #[allow(clippy::map_entry)]
            if !inner.processes.contains_key(&status.pid) {
                if let Some(channel) = inner.next_process.take() {
                    channel.waker.lock().unwrap().pid.replace(status.pid);
                    inner.processes.insert(status.pid, channel);
                }
            }

            let entry = match inner.processes.get_mut(&status.pid) {
                Some(entry) => entry,
                None => match inner.fallback.as_mut() {
                    Some(entry) => entry,
                    None => return futures::future::ready(()).boxed(),
                },
            };

            let done_tx = running.not().then(|| entry.done_tx()).flatten();
            entry.wake();

            (entry.ctx.clone(), done_tx)
        };

        async move {
            if !status.stdout.is_empty() {
                log::info!(
                    "stdout: {}",
                    String::from_utf8_lossy(&status.stdout).trim_end()
                );

                let out = CommandOutput::Bin(status.stdout);
                let evt = RuntimeEvent::stdout(ctx.batch_id.clone(), ctx.idx, out);
                ctx.tx.send(evt).await?;
            }
            if !status.stderr.is_empty() {
                log::info!(
                    "stderr: {}",
                    String::from_utf8_lossy(&status.stderr).trim_end()
                );

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
            let inner = self.inner.lock().unwrap();
            match inner.fallback.as_ref() {
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
        done_rx: BoxFuture<'a, Result<i32, ()>>,
        waker: Arc<Mutex<ProcessWaker>>,
    },
    Fallback {},
}

#[derive(Default)]
pub(crate) struct ProcessWaker {
    pid: Option<u64>,
    waker: Option<Waker>,
}

impl<'a> Handle<'a> {
    fn process(monitor: &EventMonitor, channel: &Channel) -> Self {
        Handle::Process {
            monitor: monitor.clone(),
            done_rx: channel.done_rx().unwrap(),
            waker: channel.waker.clone(),
        }
    }
}

impl<'a> Drop for Handle<'a> {
    fn drop(&mut self) {
        if let Handle::Process { monitor, waker, .. } = self {
            if let Some(pid) = { waker.lock().unwrap().pid } {
                let mut inner = monitor.inner.lock().unwrap();
                inner.processes.remove(&pid);
                inner.next_process.take();
            }
        }
    }
}

impl<'a> Future for Handle<'a> {
    type Output = i32;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.get_mut() {
            Handle::Process { done_rx, waker, .. } => match Pin::new(done_rx).poll(cx) {
                Poll::Ready(Ok(c)) => Poll::Ready(c),
                Poll::Ready(Err(_)) => Poll::Ready(1),
                Poll::Pending => {
                    let mut guard = waker.lock().unwrap();

                    if let Some(waker) = &guard.waker {
                        let cx_waker = cx.waker();
                        if !waker.will_wake(cx_waker) {
                            guard.waker.replace(cx_waker.clone());
                        }
                    } else {
                        guard.waker.replace(cx.waker().clone());
                    }

                    Poll::Pending
                }
            },
            Handle::Fallback { .. } => Poll::Ready(0),
        }
    }
}

pub(crate) struct Channel {
    ctx: CommandContext,
    done: Option<DoneChannel>,
    waker: Arc<Mutex<ProcessWaker>>,
}

impl Channel {
    fn new(ctx: CommandContext, pid: u64) -> Self {
        Channel {
            ctx,
            done: Some(Default::default()),
            waker: Arc::new(Mutex::new(ProcessWaker {
                pid: Some(pid),
                waker: None,
            })),
        }
    }

    fn fallback(ctx: CommandContext) -> Self {
        Channel {
            ctx,
            done: None,
            waker: Default::default(),
        }
    }

    fn wake(&self) {
        let guard = self.waker.lock().unwrap();
        if let Some(waker) = &guard.waker {
            waker.wake_by_ref();
        }
    }

    fn done_tx(&mut self) -> Option<oneshot::Sender<i32>> {
        self.done.as_mut().and_then(|d| d.tx.take())
    }

    fn done_rx<'a>(&self) -> Option<BoxFuture<'a, Result<i32, ()>>> {
        self.done
            .as_ref()
            .map(|d| d.rx.clone().map_err(|_| ()).boxed())
    }
}

struct DoneChannel {
    tx: Option<oneshot::Sender<i32>>,
    rx: Shared<oneshot::Receiver<i32>>,
}

impl Default for DoneChannel {
    fn default() -> Self {
        let (tx, rx) = oneshot::channel();
        Self {
            tx: Some(tx),
            rx: rx.shared(),
        }
    }
}
