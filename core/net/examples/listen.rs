use actix::{Arbiter, System};
use futures::prelude::*;

fn main() -> std::io::Result<()> {
    System::run(|| {
        Arbiter::spawn(
            ya_net::init_service_future("hub:9000", "0x789")
                .map_err(|e| eprintln!("Error: {}", e))
                .map(|r| {
                    eprintln!("Result: {:?}", r);
                }),
        );
    })
}
