use super::*;
use futures::channel::oneshot;
use futures::future::Shared;
use futures::lock::Mutex;
use futures::{FutureExt, SinkExt};
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

const REQUEST_ID: AtomicU64 = AtomicU64::new(0);

struct ClientInner<Out> {
    ids: u64,
    response_callbacks: HashMap<u64, futures::channel::oneshot::Sender<proto::Response>>,
    output: Out,
}

struct Client<Out> {
    inner: Mutex<ClientInner<Out>>,
    pid: u32,
    kill_cmd: std::sync::Mutex<Option<oneshot::Sender<()>>>,
    status: Shared<oneshot::Receiver<i32>>,
}

impl<Out: Sink<proto::Request> + Unpin> Client<Out>
where
    Out::Error: Debug,
{
    fn new(
        output: Out,
        pid: u32,
        kill_cmd: oneshot::Sender<()>,
        status: Shared<oneshot::Receiver<i32>>,
    ) -> Self {
        let kill_cmd = std::sync::Mutex::new(Some(kill_cmd));
        let inner = Mutex::new(ClientInner {
            ids: 1,
            response_callbacks: Default::default(),
            output,
        });
        Client {
            inner,
            pid,
            kill_cmd,
            status,
        }
    }

    async fn call(&self, mut param: proto::Request) -> proto::Response {
        let (tx, rx) = futures::channel::oneshot::channel();
        {
            let mut inner = self.inner.lock().await;
            inner.ids += 1;
            let id = inner.ids;
            param.id = id;
            let _ = inner.response_callbacks.insert(id, tx);
            log::debug!("sending request: {:?}", param);
            if let Err(e) = SinkExt::send(&mut inner.output, param).await {
                log::error!("Runtime client write error: {:?}", e);
            }
        }
        log::debug!("waiting for response");
        let response = rx.await.unwrap();
        log::debug!("got response: {:?}", response);
        response
    }

    async fn handle_response(&self, resp: proto::Response) {
        if resp.event {
            todo!()
        }

        if let Some(callback) = {
            let mut inner = self.inner.lock().await;
            inner.response_callbacks.remove(&resp.id)
        } {
            let _ = callback.send(resp);
        }
    }
}

impl<Out: Sink<proto::Request> + Unpin> RuntimeControl for Arc<Client<Out>> {
    fn id(&self) -> u32 {
        self.pid
    }

    fn stop(&self) {
        if let Some(s) = self.kill_cmd.lock().unwrap().take() {
            let _ = s.send(());
        }
    }

    fn stopped(&self) -> BoxFuture<'_, i32> {
        Box::pin(self.status.clone().then(|r| async move { r.unwrap_or(1) }))
    }
}

impl<Out: Sink<proto::Request> + Unpin> RuntimeService for Arc<Client<Out>>
where
    Out::Error: Debug,
{
    fn hello(&self, version: &str) -> AsyncResponse<'_, String> {
        let request = proto::Request {
            id: REQUEST_ID.fetch_add(1, Relaxed),
            command: Some(proto::request::Command::Hello(proto::request::Hello {
                version: version.to_owned(),
            })),
        };
        let fut = self.call(request);
        async move {
            match fut.await.command {
                Some(proto::response::Command::Hello(hello)) => Ok(hello.version),
                Some(proto::response::Command::Error(error)) => Err(error),
                _ => panic!("invalid response"),
            }
        }
        .boxed_local()
    }

    fn run_process(&self, run: RunProcess) -> AsyncResponse<RunProcessResp> {
        let request = proto::Request {
            id: REQUEST_ID.fetch_add(1, Relaxed),
            command: Some(proto::request::Command::Run(run)),
        };
        let fut = self.call(request);
        async move {
            match fut.await.command {
                Some(proto::response::Command::Run(run)) => Ok(run),
                Some(proto::response::Command::Error(error)) => Err(error),
                _ => panic!("invalid response"),
            }
        }
        .boxed_local()
    }

    fn kill_process(&self, kill: KillProcess) -> AsyncResponse<()> {
        let request = proto::Request {
            id: REQUEST_ID.fetch_add(1, Relaxed),
            command: Some(proto::request::Command::Kill(kill)),
        };
        let fut = self.call(request);
        async move {
            match fut.await.command {
                Some(proto::response::Command::Kill(_kill)) => Ok(()),
                Some(proto::response::Command::Error(error)) => Err(error),
                _ => panic!("invalid response"),
            }
        }
        .boxed_local()
    }

    fn create_network(&self, network: CreateNetwork) -> AsyncResponse<CreateNetworkResp> {
        let request = proto::Request {
            id: REQUEST_ID.fetch_add(1, Relaxed),
            command: Some(proto::request::Command::Network(network)),
        };
        let fut = self.call(request);
        async move {
            match fut.await.command {
                Some(proto::response::Command::Network(res)) => Ok(res),
                Some(proto::response::Command::Error(error)) => Err(error),
                _ => panic!("invalid response"),
            }
        }
        .boxed_local()
    }

    fn shutdown(&self) -> AsyncResponse<'_, ()> {
        let shutdown = proto::request::Shutdown::default();
        let request = proto::Request {
            id: REQUEST_ID.fetch_add(1, Relaxed),
            command: Some(proto::request::Command::Shutdown(shutdown)),
        };
        let fut = self.call(request);
        async move {
            match fut.await.command {
                Some(proto::response::Command::Shutdown(_shutdown)) => Ok(()),
                Some(proto::response::Command::Error(error)) => Err(error),
                _ => panic!("invalid response"),
            }
        }
        .boxed_local()
    }
}

// sends Request, recv Response
pub async fn spawn(
    mut command: process::Command,
    event_handler: impl RuntimeHandler + Send + Sync + 'static,
) -> Result<impl RuntimeService + RuntimeControl + Clone, anyhow::Error> {
    command.stdin(Stdio::piped()).stdout(Stdio::piped());
    command.kill_on_drop(true);
    let mut child: process::Child = command.spawn()?;
    let pid = child.id();
    let stdin =
        tokio_util::codec::FramedWrite::new(child.stdin.take().unwrap(), codec::Codec::default());
    let stdout = child.stdout.take().unwrap();
    let (kill_tx, kill_rx) = oneshot::channel();
    let (status_tx, status_rx) = oneshot::channel();

    let client = Arc::new(Client::new(stdin, pid, kill_tx, status_rx.shared()));
    {
        let client = client.clone();
        let mut stdout =
            tokio_util::codec::FramedRead::new(stdout, codec::Codec::<proto::Response>::default());
        let pump = async move {
            while let Some(Ok(it)) = stdout.next().await {
                if it.event {
                    handle_event(it, &event_handler).await;
                } else {
                    client.handle_response(it).await;
                }
            }
        };
        let _ = tokio::task::spawn(async move {
            futures::pin_mut!(child);
            futures::pin_mut!(kill_rx);
            futures::pin_mut!(pump);
            let code = match future::select(child, future::select(pump, kill_rx)).await {
                future::Either::Left((result, _)) => map_return_code(result, pid),
                future::Either::Right((_, mut child)) => {
                    let _ = child.kill();
                    map_return_code(child.await, pid)
                }
            };
            if let Err(_) = status_tx.send(code) {
                log::warn!("Unable to update process {} status: receiver is gone", pid);
            }
        });
    }

    async fn handle_event(response: proto::Response, handler: &impl RuntimeHandler) {
        use proto::response::Command;
        match response.command {
            Some(Command::Status(status)) => {
                let _ = handler.on_process_status(status).await;
            }
            cmd => log::warn!("invalid event: {:?}", cmd),
        }
    }

    Ok(client)
}

fn map_return_code(result: std::io::Result<ExitStatus>, pid: u32) -> i32 {
    result
        .map(|e| match e.code() {
            Some(code) => code,
            None => {
                log::warn!("Unable to kill process {}: {}", pid, e);
                1
            }
        })
        .unwrap_or_else(|e| {
            log::warn!("Child process {} error: {}", pid, e);
            1
        })
}
