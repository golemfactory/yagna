use super::RuntimeService;
use super::{codec, proto, ErrorResponse};
use futures::prelude::*;
use tokio::io;
use std::rc::Rc;
use futures::lock::Mutex;
use futures::SinkExt;
use std::process::Command;

async fn handle_command(service : &impl RuntimeService, command : proto::request::Command) -> Result<proto::response::Command, ErrorResponse> {
    Ok(match command {
        proto::request::Command::Hello(hello) => {
            let version = service.hello(&hello.version).await?;
            proto::response::Command::Hello(proto::response::Hello {
                version
            })
        }
        proto::request::Command::Run(run) => {
            proto::response::Command::Run(service.run_process(run).await?)
        }
        proto::request::Command::Kill(kill) => {
            service.kill_process(kill).await?;
            proto::response::Command::Kill(Default::default())
        }

        _ => return Err(ErrorResponse::msg("unknown command"))
    })
}

async fn handle(service : &impl RuntimeService, request : proto::Request) -> proto::Response {
    let id = request.id;
    let mut resp = proto::Response::default();
    resp.id = id;

    resp.command = Some(if let Some(command) = request.command {
        match handle_command(service, command).await {
            Ok(response) => response,
            Err(err) => proto::response::Command::Error(err)
        }
    }
    else {
        proto::response::Command::Error(ErrorResponse::msg("unknown command"))
    });
    eprintln!("response={:?}", resp);
    resp
}


pub async fn run(service: impl RuntimeService + 'static) {
    log::debug!("server starting");
    let mut stdout = io::stdout();
    let mut stdin = io::stdin();

    let mut input = codec::Codec::<proto::Request>::stream(stdin);
    let mut output = Rc::new(Mutex::new(codec::Codec::<proto::Response>::sink(stdout)));
    let service = Rc::new(service);

    tokio::task::LocalSet::new().run_until(async {

        while let Some(it) = input.next().await {
            match it {
                Ok(request) => {
                    let service = service.clone();
                    let output = output.clone();
                    let _ = tokio::task::spawn_local(async move {
                        let resp = handle(service.as_ref(), request).await;
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

    }).await;

    log::debug!("server stopped");
}
