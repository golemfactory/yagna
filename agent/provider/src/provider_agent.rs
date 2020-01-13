
use ya_client::{
    market::{ApiClient},
    web::WebClient,
    Result,
};

use crate::market::ProviderMarket;



pub struct ProviderAgent {
    market: ProviderMarket,
}


impl ProviderAgent {

    pub fn new() -> Result< ProviderAgent > {
        let client = ApiClient::new(WebClient::builder())?;
        let market = ProviderMarket::new(client, "AcceptAll");

        Ok(ProviderAgent{market})
    }

    pub async fn run(&self) {


    }

}



