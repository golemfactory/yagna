extern crate market_api;
extern crate uuid;

use market_api::*;
use uuid::Uuid;
use std::collections::HashMap;

use provider::MarketProviderFacade;
use provider::market_impl::{GolemMarketProviderFacade};

#[test]
fn provider_api_subscribe_returns_success() {

    let offer = Offer{
        offer_id : Uuid::new_v4(),
        provider_id : NodeId{},
        constraints : String::new(),
        exp_properties : HashMap::new(),
        imp_properties : vec![]
    };

    let facade : GolemMarketProviderFacade = GolemMarketProviderFacade::new();

    assert_eq!(facade.subscribe(offer), Ok(0));
}

