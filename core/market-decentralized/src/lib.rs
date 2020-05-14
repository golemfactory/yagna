// TODO: This is only temporary as long there's only market structure.
//       Remove as soon as possible.
#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unused_imports)]

mod market;
mod matcher;
mod negotiation;

pub mod protocol;
pub mod service {
    use ya_persistence::executor::DbExecutor;
    use ya_service_api_interfaces::{Provider, Service};

    pub struct MarketService;

    impl Service for MarketService {
        type Cli = ();
    }

    impl MarketService {
        pub fn rest(db: &DbExecutor) -> actix_web::Scope {
            unimplemented!()
        }
        pub async fn gsb<Context: Provider<Self, DbExecutor>>(ctx: &Context) -> anyhow::Result<()> {
            unimplemented!()
        }
    }
}

pub use market::Market;
pub use ya_client_model::market::MARKET_API_PATH;
