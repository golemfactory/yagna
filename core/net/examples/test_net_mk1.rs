use anyhow::{Context, Error, Result};
use futures::future::TryFutureExt;
use serde::{Deserialize, Serialize};
use std::env;
use structopt::StructOpt;

use ya_client_model::NodeId;
use ya_core_model::net;
use ya_net::{RemoteEndpoint, TryRemoteEndpoint};
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
    #[structopt(long, default_value = "127.0.0.1:9000")]
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
    Hub,
    Listener,
    Sender,
}

impl Options {
    fn id(side: &Side) -> NodeId {
        match side {
            Side::Listener => "0xbabe000000000000000000000000000000000000",
            Side::Sender => "0xfeed000000000000000000000000000000000000",
            _ => unimplemented!(),
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

    match options.side {
        Side::Hub => {
            ya_sb_router::bind_gsb_router(Some(format!("tcp://{}", options.hub_addr).parse()?))
                .await?;
            actix_rt::signal::ctrl_c().await?;
            println!();
            log::info!("SIGINT received, exiting");
            return Ok(());
        }
        Side::Listener => {
            // will use default GSB_URL
        }
        Side::Sender => {
            // redefine GSB_URL not to make bind conflict
            std::env::set_var(ya_sb_proto::GSB_URL_ENV_VAR, "tcp://127.0.0.1:7777");
        }
    }

    // code below is only for Listener and Sender

    ya_sb_router::bind_gsb_router(None)
        .await
        .context(format!("Error binding local router"))?;

    std::env::set_var(ya_net::CENTRAL_ADDR_ENV_VAR, &options.hub_addr);
    let registered_id: NodeId = "0xdad0000000000000000000000000000000000000".parse()?;
    let unregistered_id: NodeId = "0xbed0000000000000000000000000000000000000".parse()?;
    let registered_net_ids = match options.side {
        Side::Sender => vec![options.my_id(), registered_id],
        _ => vec![options.my_id()],
    };
    ya_net::bind_remote(options.my_id(), registered_net_ids)
        .await
        .context(format!(
            "Error binding service at {} for {}",
            options.hub_addr,
            options.my_id()
        ))?;
    log::info!("Started listening on the centralised Hub");

    match options.side {
        Side::Hub => unimplemented!(),
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

            let caller_id: NodeId = "0xbeef000000000000000000000000000000000000".parse()?;
            let r = listener_id
                .try_service(net::PUBLIC_PREFIX)?
                .send_as(caller_id, Test("Test 1 msg".into()))
                .map_err(Error::msg)
                .await?;
            assert!(r.is_ok());
            log::info!(
                "should ignore send_as and allow sending as default identity: {:?}",
                r
            );

            let r = ya_net::from(unregistered_id)
                .to(listener_id)
                .service(net::PUBLIC_PREFIX)
                .send(Test("Test 2 msg".into()))
                .map_err(Error::msg)
                .await;
            assert!(r.is_err());
            log::info!(
                "should disallow sending as node id not registered with `bind_remote`: {:?}",
                r
            );

            let r = ya_net::from(unregistered_id)
                .to(listener_id)
                .service(net::PUBLIC_PREFIX)
                .send_as(registered_id, Test("Test 3 msg".into()))
                .map_err(Error::msg)
                .await;
            assert!(r.is_err());
            log::info!("should ignore send_as and disallow sending as node id not registered with `bind_remote`: {:?}", r);

            let r = ya_net::from(registered_id)
                .to(listener_id)
                .service(net::PUBLIC_PREFIX)
                .send_as(unregistered_id, Test("Test 4 msg".into()))
                .map_err(Error::msg)
                .await;
            assert!(r.is_ok());
            log::info!("should ignore send_as and allow sending as node id registered with `bind_remote`: {:?}", r);
        }
    }

    Ok(())
}
