use super::RuntimeService;
use super::{codec, proto, ErrorResponse};
use crate::server::RuntimeEvent;
use futures::lock::Mutex;
use futures::prelude::*;
use futures::SinkExt;

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

        _ => return Err(ErrorResponse::msg("unknown command")),
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

pub struct EventEmiter {
    tx: futures::channel::mpsc::UnboundedSender<proto::Response>,
}

impl RuntimeEvent for EventEmiter {
    fn on_process_status(&self, status: proto::response::ProcessStatus) {
        let mut response = proto::Response::default();
        response.event = true;
        response.command = Some(proto::response::Command::Status(status));
        if let Err(e) = self.tx.unbounded_send(response) {
            log::error!("send event failed: {}", e)
        }
    }
}

pub async fn run<Factory, FutureRuntime, Runtime>(factory: Factory)
where
    Factory: FnOnce(EventEmiter) -> FutureRuntime,
    FutureRuntime: Future<Output = Runtime>,
    Runtime: RuntimeService + 'static,
{
    log::debug!("server starting");
    let stdout = io::stdout();
    let stdin = io::stdin();

    let mut input = codec::Codec::<proto::Request>::stream(stdin);
    let output = Rc::new(Mutex::new(codec::Codec::<proto::Response>::sink(stdout)));
    let (tx, mut rx) = futures::channel::mpsc::unbounded::<proto::Response>();
    let emiter = EventEmiter { tx };
    let service = Rc::new(factory(emiter).await);

    tokio::task::LocalSet::new()
        .run_until(async {
            let _event_sender = {
                let output = output.clone();
                async move {
                    while let Some(event) = rx.next().await {
                        let mut output = output.lock().await;
                        let _r = SinkExt::send(&mut *output, event).await;
                    }
                }
            };

            while let Some(it) = input.next().await {
                match it {
                    Ok(request) => {
                        let service = service.clone();
                        let output = output.clone();
                        let _ = tokio::task::spawn_local(async move {
                            log::debug!("received request: {:?}", request);
                            let resp = handle(service.as_ref(), request).await;
                            log::debug!("response to send: {:?}", resp);
                            let mut output = output.lock().await;
                            log::debug!("sending");
                            let r = SinkExt::send(&mut *output, resp).await;
                            log::debug!("sending done: {:?}", r.is_ok());
                        });
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
