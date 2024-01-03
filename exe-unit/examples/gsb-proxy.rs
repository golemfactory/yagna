use actix::{Actor, Addr, Context, Handler, Running};
use actix_rt::{time, ArbiterHandle};
use chrono::{NaiveDateTime, Utc};
use futures::prelude::*;
use serde::{Deserialize, Serialize};
use std::env;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;
use structopt::StructOpt;
use ya_core_model::net::local as model;
use ya_core_model::net::local::StatusError;
use ya_service_bus::{typed as bus, RpcStreamCall, RpcStreamHandler, RpcStreamMessage};
use ya_service_bus::{Error, RpcMessage};

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

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct GsbHttpCall {
    pub host: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GsbHttpCallEvent {
    pub index: usize,
    pub timestamp: NaiveDateTime,
    pub val: String,
}

impl RpcMessage for GsbHttpCall {
    const ID: &'static str = "GsbHttpCall";
    type Item = GsbHttpCallEvent;
    type Error = StatusError;
}

impl RpcStreamMessage for GsbHttpCall {
    const ID: &'static str = "GsbHttpCall";
    type Item = GsbHttpCallEvent;
    type Error = StatusError;
}

pub async fn process_msg(mut msg: RpcStreamCall<GsbHttpCall>) -> Result<Vec<String>, Error> {
    println!("process {:?}", msg.body.host);

    let stream = stream::iter(vec![1, 2, 3]);

    let result = vec![];

    let event: GsbHttpCallEvent;

    //msg.reply.send().await;

    Ok(result)
}

#[inline]
fn status_err(e: anyhow::Error) -> StatusError {
    println!("error {}", e);
    StatusError::RuntimeException(e.to_string())
}

#[derive(Serialize, Deserialize)]
struct Ping(String);

impl RpcStreamMessage for Ping {
    const ID: &'static str = "ping";
    type Item = String;
    type Error = ();
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

        let _ = bus::bind_stream(model::BUS_ID, |_p: GsbHttpCall| {
            let interval = tokio::time::interval(Duration::from_secs(1));
            tokio_stream::wrappers::IntervalStream::new(interval)
                .map(|_ts| {
                    let response = GsbHttpCallEvent {
                        index: 1,
                        timestamp: Utc::now().naive_local(),
                        val: "response".to_string(),
                    };
                    Ok(response)
                })
                .take(10)
        });

        let mut interval = time::interval(tokio::time::Duration::from_secs(3));

        loop {
            interval.tick().await;

            println!("tick");
        }
    } else if args.mode == Mode::Send {
        let stream = bus::service(model::BUS_ID).call_streaming(GsbHttpCall {
            host: "http://localhost".to_string(),
        });
        // let stream =
        //     bus::service(model::BUS_ID).call_streaming(Ping("http://localhost".to_string()));

        stream
            .for_each(|r| async move {
                if let Ok(r_r) = r {
                    if let Ok(e) = r_r {
                        log::info!("[STREAM #{}][{}] {}", e.index, e.timestamp, e.val);
                    }
                }
            })
            .await;
        // while let Some(r) = stream.next() {
        //     println!("got {:?}", r);
        // }
    }

    Ok(())
}
