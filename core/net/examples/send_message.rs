use actix::{Arbiter, System};
use futures::future::Future;
use futures03::future::TryFutureExt;

fn main() -> std::io::Result<()> {
    System::run(|| {
        Arbiter::spawn(
            ya_net::init_service_future("127.0.0.1:9000", "0x123")
                .and_then(|_| {
                    ya_net::send_message_future("0x123", "0x789/test", "Test".as_bytes().to_vec())
                        .map_err(|e| {
                            std::io::Error::new(std::io::ErrorKind::Other, format!("{}", e))
                        })
                })
                .map_err(|e| eprintln!("Error: {}", e))
                .compat()
                .map(|r| {
                    eprintln!("Result: {:?}", r);
                }),
        );
    })
}
