use std::collections::HashMap;
use futures::lock::Mutex;
use futures::prelude::*;
use std::fmt::Debug;
use futures::SinkExt;
use super::*;

struct ClientInner<Out> {
    ids: u64,
    response_callbacks: HashMap<u64, futures::channel::oneshot::Sender<proto::Response>>,
    output: Out,
}

struct Client<Out> {
    inner: Mutex<ClientInner<Out>>,
}

impl<Out: Sink<proto::Request> + Unpin> Client<Out>
    where
        Out::Error: Debug,
{
    fn new(output: Out) -> Self {
        let inner = Mutex::new(ClientInner {
            ids: 1,
            response_callbacks: Default::default(),
            output,
        });
        Client { inner }
    }

    async fn call(&self, mut param: proto::Request) -> proto::Response {
        let (tx, rx) = futures::channel::oneshot::channel();
        {
            let mut inner = self.inner.lock().await;
            inner.ids += 1;
            let id = inner.ids;
            param.id = id;
            let _ = inner.response_callbacks.insert(id, tx);
            log::debug!("sending request");
            SinkExt::send(&mut inner.output, param).await.unwrap();
        }
        log::debug!("waiting for response");
        let response = rx.await.unwrap();
        log::debug!("got response");
        response
    }

    async fn handle_response(&self, resp: proto::Response) {
        log::debug!("recv response={:?}", resp);
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

impl<Out: Sink<proto::Request> + Unpin> RuntimeService for Arc<Client<Out>>
    where
        Out::Error: Debug,
{
    fn hello(&self, version: &str) -> AsyncResponse<'_, String> {
        let request = proto::Request {
            id: 0,
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
            id: 0,
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
        unimplemented!()
    }
}

// sends Request, recv Response
pub async fn spawn(mut command: process::Command) -> impl RuntimeService + Clone {
    let (tx, rx) = futures::channel::mpsc::unbounded::<proto::Response>();
    command.stdin(Stdio::piped()).stdout(Stdio::piped());
    command.kill_on_drop(true);
    let mut child: process::Child = command.spawn().expect("run failed");
    let stdin =
        tokio_util::codec::FramedWrite::new(child.stdin.take().unwrap(), codec::Codec::default());
    let stdout = child.stdout.take().unwrap();
    let client = Arc::new(Client::new(stdin));
    {
        let client = client.clone();
        let mut stdout = tokio_util::codec::FramedRead::new(stdout, codec::Codec::default());
        let pump = async move {
            while let Some(it) = stdout.next().await {
                let it = it.unwrap();
                client.handle_response(it).await;
            }
        };
        let _ = tokio::task::spawn(async move {
            let _ = future::join(pump, child).await;
        });
    }
    client
}
