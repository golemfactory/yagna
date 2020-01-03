use actix::{Arbiter, System};
use futures::future::Future;
use futures03::future::TryFutureExt;

fn main() -> std::io::Result<()> {
    System::run(|| {
        Arbiter::spawn(
            ya_net::init_service_future("hub:9000", "0x789")
                .map_err(|e| eprintln!("Error: {}", e))
                .compat()
                .map(|r| {
                    eprintln!("Result: {:?}", r);
                }),
        );
    })
}
