
use ya_client::{
    market::{ApiClient},
    web::WebClient,
    Result,
};

use crate::market::ProviderMarket;

use std::{thread, time};
use log::{error};



pub struct ProviderAgent {
    market: ProviderMarket,
}


impl ProviderAgent {

    pub fn new() -> Result< ProviderAgent > {
        let client = ApiClient::new(WebClient::builder())?;
        let market = ProviderMarket::new(client, "AcceptAll");

        Ok(ProviderAgent{market})
    }

    pub async fn run(&mut self) {

        if let Err(error) = self.market.start().await {
            error!("Error while starting market: {}", error);
            return ();
        }

        //TODO: We should replace this loop with scheduler in future.
        loop {
            match self.market.run_step() {
                Err(error) => error!("Market error: {}", error),
                _ => {}
            }
            thread::sleep(time::Duration::from_secs(1));
        }
    }

}



