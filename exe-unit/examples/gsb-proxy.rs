use actix_rt::time;
use async_stream::stream;
use bytes::{BufMut, BytesMut};
use chrono::Utc;
use futures::prelude::*;
use gsb_http_proxy::{GsbHttpCall, GsbHttpCallEvent};
use reqwest::Client;
use std::env;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;
use structopt::StructOpt;
use ya_service_bus::typed as bus;

#[derive(StructOpt, Debug, PartialEq)]
pub enum Mode {
    Send,
    Receive,
    Trigger,
}

type ParseError = &'static str;

impl FromStr for Mode {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "send" => Ok(Mode::Send),
            "receive" => Ok(Mode::Receive),
            "trigger" => Ok(Mode::Trigger),
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

        let mut count = 0;

        let _stream_handle =
            bus::bind_stream(gsb_http_proxy::BUS_ID, move |http_call: GsbHttpCall| {
                let _interval = tokio::time::interval(Duration::from_secs(1));
                let stream = Box::pin(stream! {
                    for i in 0..10 {
                        let msg = format!("called {} element #{}", http_call.host, i);
                        count += 1;
                        let response = GsbHttpCallEvent {
                            index: count,
                            timestamp: Utc::now().naive_local().to_string(),
                            val: msg,
                        };
                        println!("sending nr {}", count);
                        yield Ok(response);
                    }
                });

                // let stream = tokio_stream::wrappers::IntervalStream::new(interval)
                //     .map(move |_ts| {
                //         println!("Creating response");
                //         let msg = format!("response from {}", http_call.host);
                //         count += 1;
                //         let response = GsbHttpCallEvent {
                //             index: count,
                //             timestamp: Utc::now().naive_local(),
                //             val: msg,
                //         };
                //         if count == 7 {
                //             return Err(HttpProxyStatusError::RuntimeException("end".to_string()));
                //         }
                //         Ok(response)
                //     })
                //     .take(5);
                println!("returning stream");
                stream
            });

        let mut interval = time::interval(tokio::time::Duration::from_secs(3));

        loop {
            interval.tick().await;

            println!("tick");
        }
    } else if args.mode == Mode::Send {
        // env::set_var("GSB_URL", "tcp://127.0.0.1:12501");

        let stream = bus::service(gsb_http_proxy::BUS_ID).call_streaming(GsbHttpCall {
            host: "http://localhost".to_string(),
        });

        stream
            .for_each(|r| async move {
                if let Ok(r_r) = r {
                    if let Ok(e) = r_r {
                        log::info!("[STREAM #{}][{}] {}", e.index, e.timestamp, e.val);
                    }
                }
            })
            .await;
    } else if args.mode == Mode::Trigger {
        let client = Client::new();
        let request = client
            .get("http://127.0.0.1:11502/activity-api/v1/activity/882b48e3307949d6a8015113f09e63cb/proxy_http_request")
            .bearer_auth("b073104dfd8046558b8b76e9e852d0d8")
            .build()?;

        if let Some(req2) = request.try_clone() {
            let resp = client.execute(req2).await;
            println!("{:?}", request);
            println!("{:?}", resp);
            match resp {
                Ok(response) => {
                    println!("{:#?}", response.status());
                    let mut stream = response.bytes_stream();
                    let mut buf = BytesMut::new();
                    while let Some(item) = stream.next().await {
                        for byte in item? {
                            if byte == b'\n' {
                                println!("Got chunk: {:?}\n", buf.clone().freeze());
                                buf.clear();
                            } else {
                                buf.put_u8(byte);
                            }
                        }
                    }
                }
                Err(e) => {
                    println!("{:?}", e);
                }
            }
        }
    }

    Ok(())
}
