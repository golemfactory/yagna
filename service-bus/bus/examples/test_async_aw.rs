use serde::{Deserialize, Serialize};
use std::env;

use std::error::Error;
use ya_service_bus::{typed as bus, RpcMessage};

#[derive(Serialize, Deserialize)]
struct Ping(String);

impl RpcMessage for Ping {
    const ID: &'static str = "ping";
    type Item = String;
    type Error = ();
}

async fn server() -> Result<(), Box<dyn Error>> {
    log::info!("starting");
    let (tx, rx) = futures::channel::oneshot::channel::<()>();

    let mut txh = Some(tx);
    let quit = move |_p: Ping| {
        let tx = txh.take().unwrap();
        async move {
            eprintln!("quit!!");
            let _ = tx.send(());
            {
                Ok("quit".to_string())
            }
        }
    };

    let _ = bus::bind_private("/test", |p: Ping| async move {
        eprintln!("test!!");
        Ok(format!("pong {}", p.0))
    });
    let _ = bus::bind_private("/quit", quit);

    let _ = rx.await;
    Ok(())
}

#[actix_rt::main]
async fn main() -> Result<(), Box<dyn Error>> {
    env::set_var("RUST_LOG", env::var("RUST_LOG").unwrap_or("debug".into()));
    env_logger::init();
    server().await
}
