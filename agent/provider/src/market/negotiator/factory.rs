use actix::Addr;
use structopt::StructOpt;

use super::common::NegotiatorAddr;
use crate::market::config::MarketConfig;
use crate::market::negotiator::LimitAgreementsNegotiator;
use crate::market::negotiator::{AcceptAllNegotiator, CompositeNegotiator};
use crate::market::ProviderMarket;
use std::sync::Arc;

/// Configuration for ProviderMarket actor.
#[derive(StructOpt, Clone, Debug)]
pub struct LimitAgreementsNegotiatorConfig {
    #[structopt(long, env, default_value = "1")]
    pub max_simultaneous_agreements: u32,
}

/// Configuration for ProviderMarket actor.
#[derive(StructOpt, Clone, Debug)]
pub struct NegotiatorsConfig {
    #[structopt(flatten)]
    pub limit_agreements_config: LimitAgreementsNegotiatorConfig,
}

pub fn create_negotiator(
    market: Addr<ProviderMarket>,
    config: &MarketConfig,
) -> Arc<NegotiatorAddr> {
    let negotiator = match &config.negotiator_type[..] {
        "LimitAgreements" => NegotiatorAddr::from(LimitAgreementsNegotiator::new(
            market,
            &config.negotiator_config.limit_agreements_config,
        )),
        "Composite" => NegotiatorAddr::from(CompositeNegotiator::new(
            market,
            &config.negotiator_config.limit_agreements_config,
        )),
        "AcceptAll" => NegotiatorAddr::from(AcceptAllNegotiator::new()),
        _ => Default::default(),
    };
    Arc::new(negotiator)
}

impl Default for NegotiatorAddr {
    fn default() -> Self {
        NegotiatorAddr::from(AcceptAllNegotiator::new())
    }
}
