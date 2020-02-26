use serde::{Deserialize, Serialize};
use std::env;

use futures::prelude::*;
use std::error::Error;
use std::time::Duration;
use ya_service_bus::{typed as bus, RpcStreamMessage};

#[derive(Serialize, Deserialize)]
struct Ping(String);

impl RpcStreamMessage for Ping {
    const ID: &'static str = "ping";
    type Item = String;
    type Error = ();
}

async fn server() -> Result<(), Box<dyn Error>> {
    log::info!("starting");
    let (tx, rx) = futures::channel::oneshot::channel::<()>();

    let mut txh = Some(tx);
    let _quit = move |_p: Ping| {
        let tx = txh.take().unwrap();
        async move {
            eprintln!("quit!!");
            let _ = tx.send(());
            {
                Ok::<_, ()>("quit".to_string())
            }
        }
    };

    let _ = bus::bind_stream("/local/test", |_p: Ping| {
        tokio::time::interval(Duration::from_secs(1))
            .map(|_ts| Ok("tick".to_string()))
            .take(10)
    });

    //let _ = bus::bind("/local/quit", quit);

    let _ = rx.await;
    Ok(())
}

#[actix_rt::main]
async fn main() -> Result<(), Box<dyn Error>> {
    env::set_var("RUST_LOG", env::var("RUST_LOG").unwrap_or("debug".into()));
    env_logger::init();
    server().await
}
