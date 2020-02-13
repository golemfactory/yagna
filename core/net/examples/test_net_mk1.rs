use anyhow::{Context, Error, Result};
use futures::future::TryFutureExt;
use serde::{Deserialize, Serialize};
use std::env;
use structopt::StructOpt;

use ya_service_bus::{typed as bus, RpcEndpoint, RpcMessage};

#[derive(Serialize, Deserialize)]
struct Test(String);

impl RpcMessage for Test {
    const ID: &'static str = "test";
    type Item = String;
    type Error = ();
}

#[derive(StructOpt)]
#[structopt(name = "Provider", about = "Networked Provider Example")]
struct Options {
    /// remote bus addr (Mk1 centralised net router)
    #[structopt(long, default_value = "hub:9000")]
    hub_addr: String,

    /// Log verbosity
    #[structopt(long, default_value = "debug")]
    log_level: String,

    /// Network side
    #[structopt(subcommand)]
    side: Side,
}

#[derive(StructOpt, Debug)]
enum Side {
    Listener,
    Sender,
}

impl Options {
    fn id(side: &Side) -> String {
        format!("0x0_{:?}", side)
    }

    fn my_id(&self) -> String {
        Options::id(&self.side)
    }
}

#[actix_rt::main]
async fn main() -> Result<()> {
    let options = Options::from_args();
    env::set_var(
        "RUST_LOG",
        env::var("RUST_LOG").unwrap_or(options.log_level.clone()),
    );
    env_logger::init();

    let local_bus_addr = *ya_service_api::constants::YAGNA_BUS_ADDR;
    ya_sb_router::bind_router(local_bus_addr)
        .await
        .context(format!("Error binding local router to {}", local_bus_addr))?;

    ya_net::bind_remote(&options.hub_addr, &options.my_id())
        .await
        .context(format!(
            "Error binding service at {} for {}",
            options.hub_addr,
            options.my_id()
        ))?;
    log::info!("Started listening on the centralised Hub");

    match options.side {
        Side::Listener => {
            let _ = bus::bind("/public", |p: Test| async move {
                log::info!("test called!!");
                Ok(format!("pong {}", p.0))
            });
            log::info!("Started listening on the local bus");
            actix_rt::signal::ctrl_c().await?;
            println!();
            log::info!("SIGINT received, exiting");
        }
        Side::Sender => {
            let listener_id = Options::id(&Side::Listener);
            let r = bus::service(&format!("/net/{}", listener_id))
                .send(Test("Test".into()))
                .map_err(Error::msg)
                .await?;
            log::info!("Sending Result: {:?}", r);
        }
    }

    Ok(())
}
