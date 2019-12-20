use actix::{Arbiter, System};
use futures::future::Future;
use futures03::future::{FutureExt, TryFutureExt};
use ya_core_model::net::{Message, MessageAddress, MessageType, SendMessage};
use ya_service_bus::{typed as bus, RpcEndpoint};

fn main() -> std::io::Result<()> {
    let message: Message = Message {
        destination: MessageAddress::Node("0x123".into()),
        module: "module".into(),
        method: "method".into(),
        reply_to: "0x999".into(),
        request_id: 1000,
        message_type: MessageType::Request,
    };

    System::run(|| {
        ya_net::init_service();
        Arbiter::spawn(
            bus::service("/local/net")
                .send(SendMessage { message })
                .boxed_local()
                .compat()
                .then(|r| r.unwrap())
                .map_err(|e| eprintln!("err={:?}", e)),
        );
    })
}
