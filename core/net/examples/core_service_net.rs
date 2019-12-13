use actix::System;

fn main() -> std::io::Result<()> {
    System::run(|| {
        ya_net::init_service();
    })
}
