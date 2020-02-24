use actix::prelude::*;
use serde::{Deserialize, Serialize};
use std::env;
use ya_service_bus::{actix_rpc, Handle, RpcEnvelope, RpcMessage};

#[derive(Default)]
struct Server(Option<Handle>);

impl Actor for Server {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        self.0 = Some(actix_rpc::bind("/local/ping", ctx.address().recipient()))
    }
}

#[derive(Serialize, Deserialize)]
struct Ping(String);

impl RpcMessage for Ping {
    const ID: &'static str = "PING";
    type Item = String;
    type Error = ();
}

impl Handler<RpcEnvelope<Ping>> for Server {
    type Result = Result<String, ()>;

    fn handle(&mut self, msg: RpcEnvelope<Ping>, _ctx: &mut Self::Context) -> Self::Result {
        eprintln!("got ping");
        Ok(msg.into_inner().0)
    }
}

async fn start_server() {
    let server = Server::default().start();

    let resp = server
        .send(RpcEnvelope::local(Ping("test01".into())))
        .await
        .unwrap();
    eprintln!("resp = {:?}", resp);
}

#[actix_rt::main]
async fn main() {
    env::set_var("RUST_LOG", env::var("RUST_LOG").unwrap_or("debug".into()));
    env_logger::init();
    start_server().await
}
