use anyhow::{Context, Error, Result};
use futures::future::TryFutureExt;
use serde::{Deserialize, Serialize};
use std::env;
use structopt::StructOpt;

use ya_client_model::NodeId;
use ya_core_model::net;
use ya_net::TryRemoteEndpoint;
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
    fn id(side: &Side) -> NodeId {
        match side {
            Side::Listener => "0xbabe000000000000000000000000000000000000",
            Side::Sender => "0xfeed000000000000000000000000000000000000",
        }
        .parse()
        .unwrap()
    }

    fn my_id(&self) -> NodeId {
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

    ya_sb_router::bind_gsb_router(None)
        .await
        .context(format!("Error binding local router"))?;

    std::env::set_var(ya_net::CENTRAL_ADDR_ENV_VAR, &options.hub_addr);
    ya_net::bind_remote(&options.my_id())
        .await
        .context(format!(
            "Error binding service at {} for {}",
            options.hub_addr,
            options.my_id()
        ))?;
    log::info!("Started listening on the centralised Hub");

    match options.side {
        Side::Listener => {
            let _ = bus::bind(net::PUBLIC_PREFIX, |p: Test| async move {
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
            let r = listener_id
                .try_service(net::PUBLIC_PREFIX)?
                .send(Test("Test msg".into()))
                .map_err(Error::msg)
                .await?;
            log::info!("Sending Result: {:?}", r);
        }
    }

    Ok(())
}
