use actix_rt::time;
use async_stream::stream;
use futures::prelude::*;
use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;
use structopt::StructOpt;
use ya_gsb_http_proxy::message::GsbHttpCallStreamingMessage;
use ya_gsb_http_proxy::response::{
    GsbHttpCallResponseBody, GsbHttpCallResponseHeader, GsbHttpCallResponseStreamChunk,
};
use ya_service_bus::typed as bus;

/// This example allows to test proxying http requests via GSB.
/// It should be ran in two modes:
/// - first Receive
/// - then Send
///
///   cargo run -p ya-exe-unit --example gsb-proxy -- --mode receive
///   cargo run -p ya-exe-unit --example gsb-proxy -- --mode send
///

#[derive(StructOpt, Debug, PartialEq)]
pub enum Mode {
    Send,
    Receive,
}

type ParseError = &'static str;

impl FromStr for Mode {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "send" => Ok(Mode::Send),
            "receive" => Ok(Mode::Receive),
            _ => Err("Could not parse mode"),
        }
    }
}

#[derive(StructOpt, Debug)]
pub struct Cli {
    #[structopt(long, default_value = "/")]
    pub url: PathBuf,
    #[structopt(short = "m", long = "mode", default_value = "send")]
    pub mode: Mode,
}

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    env::set_var(
        "RUST_LOG",
        env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
    );
    env_logger::init();

    let args: Cli = Cli::from_args();

    println!("args: url {:?}", args.url);
    println!("args: mode {:?}", args.mode);

    if args.mode == Mode::Receive {
        ya_sb_router::bind_gsb_router(None).await?;

        let _stream_handle = bus::bind_stream(
            ya_gsb_http_proxy::BUS_ID,
            move |msg: GsbHttpCallStreamingMessage| {
                let _interval = tokio::time::interval(Duration::from_secs(1));
                println!("Received request, responding with 10 elements");
                Box::pin(stream! {
                    let header = GsbHttpCallResponseStreamChunk::Header(GsbHttpCallResponseHeader {
                        response_headers: Default::default(),
                        status_code: 200,
                    });
                    yield Ok(header);

                    for i in 0..10 {
                        let msg = format!("called {} {} #{} time", msg.method, msg.path, i);
                        let chunk = GsbHttpCallResponseStreamChunk::Body (
                            GsbHttpCallResponseBody {
                                msg_bytes: msg.into_bytes()
                            });
                        yield Ok(chunk);
                    }
                })
            },
        );

        let mut interval = time::interval(tokio::time::Duration::from_secs(3));

        loop {
            interval.tick().await;

            println!("tick");
        }
    } else if args.mode == Mode::Send {
        // env::set_var("GSB_URL", "tcp://127.0.0.1:12501");

        let stream =
            bus::service(ya_gsb_http_proxy::BUS_ID).call_streaming(GsbHttpCallStreamingMessage {
                method: "GET".to_string(),
                path: args.url.to_str().unwrap_or("/").to_string(),
                body: None,
                headers: HashMap::new(),
            });

        stream
            .for_each(|r| async move {
                if let Ok(Ok(chunk)) = r {
                    match chunk {
                        GsbHttpCallResponseStreamChunk::Header(h) => {
                            log::info!("[Stream response]: status code: {:}", h.status_code);
                        }
                        GsbHttpCallResponseStreamChunk::Body(b) => {
                            log::info!("[Stream response]: {:?}", String::from_utf8(b.msg_bytes));
                        }
                    }
                }
            })
            .await;
    }

    Ok(())
}
