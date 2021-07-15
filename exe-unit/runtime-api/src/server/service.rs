use super::RuntimeService;
use super::{codec, proto, ErrorResponse};
use crate::server::RuntimeEvent;
use futures::future::BoxFuture;
use futures::lock::Mutex;
use futures::prelude::*;
use futures::{FutureExt, SinkExt};
use std::rc::Rc;
use tokio::io;

async fn handle_command(
    service: &impl RuntimeService,
    command: proto::request::Command,
) -> Result<proto::response::Command, ErrorResponse> {
    Ok(match command {
        proto::request::Command::Hello(hello) => {
            let version = service.hello(&hello.version).await?;
            proto::response::Command::Hello(proto::response::Hello { version })
        }
        proto::request::Command::Run(run) => {
            proto::response::Command::Run(service.run_process(run).await?)
        }
        proto::request::Command::Kill(kill) => {
            service.kill_process(kill).await?;
            proto::response::Command::Kill(Default::default())
        }
        proto::request::Command::Network(network) => {
            proto::response::Command::Network(service.create_network(network).await?)
        }
        proto::request::Command::Shutdown(_) => {
            service.shutdown().await?;
            proto::response::Command::Shutdown(Default::default())
        }
    })
}

async fn handle(service: &impl RuntimeService, request: proto::Request) -> proto::Response {
    let id = request.id;
    let mut resp = proto::Response::default();
    resp.id = id;

    resp.command = Some(if let Some(command) = request.command {
        match handle_command(service, command).await {
            Ok(response) => response,
            Err(err) => proto::response::Command::Error(err),
        }
    } else {
        proto::response::Command::Error(ErrorResponse::msg("unknown command"))
    });
    resp
}

pub struct EventEmitter {
    tx: futures::channel::mpsc::Sender<proto::Response>,
}

impl RuntimeEvent for EventEmitter {
    fn on_process_status<'a>(&self, status: proto::response::ProcessStatus) -> BoxFuture<'a, ()> {
        let mut response = proto::Response::default();
        response.event = true;
        response.command = Some(proto::response::Command::Status(status));
        let mut tx = self.tx.clone();
        async move {
            if let Err(e) = tx.send(response).await {
                log::error!("send event failed: {}", e)
            }
        }
        .boxed()
    }
}

pub async fn run_async<Factory, FutureRuntime, Runtime>(factory: Factory)
where
    Factory: FnOnce(EventEmitter) -> FutureRuntime,
    FutureRuntime: Future<Output = Runtime>,
    Runtime: RuntimeService + 'static,
{
    log::debug!("server starting");
    let stdout = io::stdout();
    let stdin = io::stdin();

    let mut input = codec::Codec::<proto::Request>::stream(stdin);
    let output = Rc::new(Mutex::new(codec::Codec::<proto::Response>::sink(stdout)));
    let (tx, mut rx) = futures::channel::mpsc::channel::<proto::Response>(1);
    let emitter = EventEmitter { tx };
    let service = Rc::new(factory(emitter).await);

    let local = tokio::task::LocalSet::new();

    local.spawn_local({
        let output = output.clone();
        async move {
            while let Some(event) = rx.next().await {
                log::trace!("event: {:?}", event);
                let mut output = output.lock().await;
                let r = SinkExt::send(&mut *output, event).await;
                log::trace!("sending event done: {:?}", r);
            }
        }
    });

    local
        .run_until(async {
            while let Some(it) = input.next().await {
                match it {
                    Ok(request) => {
                        let service = service.clone();
                        let output = output.clone();
                        log::trace!("received request: {:?}", request);
                        let resp = handle(service.as_ref(), request).await;
                        log::trace!("response to send: {:?}", resp);
                        let mut output = output.lock().await;
                        log::trace!("sending");
                        let r = SinkExt::send(&mut *output, resp).await;
                        log::trace!("sending done: {:?}", r);
                    }
                    Err(e) => {
                        log::error!("fail: {}", e);
                        break;
                    }
                }
            }
        })
        .await;

    log::debug!("server stopped");
}

pub async fn run<Factory, Runtime>(factory: Factory)
where
    Factory: Fn(EventEmitter) -> Runtime,
    Runtime: RuntimeService + 'static,
{
    run_async(|e| async { factory(e) }).await
}
