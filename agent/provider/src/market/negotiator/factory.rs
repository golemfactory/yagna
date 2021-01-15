use actix::Addr;
use humantime;
use structopt::StructOpt;

use super::common::NegotiatorAddr;
use crate::market::config::MarketConfig;
use crate::market::negotiator::LimitAgreementsNegotiator;
use crate::market::negotiator::{AcceptAllNegotiator, CompositeNegotiator};
use crate::market::ProviderMarket;
use std::sync::Arc;

/// Configuration for LimitAgreements Negotiator.
#[derive(StructOpt, Clone, Debug)]
pub struct LimitAgreementsNegotiatorConfig {
    #[structopt(long, env, default_value = "1")]
    pub max_simultaneous_agreements: u32,
}

/// Configuration for LimitAgreements Negotiator.
#[derive(StructOpt, Clone, Debug)]
pub struct AgreementExpirationNegotiatorConfig {
    #[structopt(long, env, parse(try_from_str = humantime::parse_duration), default_value = "5min")]
    pub min_agreement_expiration: std::time::Duration,
    #[structopt(long, env, parse(try_from_str = humantime::parse_duration), default_value = "30min")]
    pub max_agreement_expiration: std::time::Duration,
    #[structopt(long, env, parse(try_from_str = humantime::parse_duration), default_value = "3h")]
    pub max_agreement_expiration_without_deadline: std::time::Duration,
    #[structopt(long, env, parse(try_from_str = humantime::parse_duration), default_value = "2min")]
    pub debit_note_acceptance_deadline: std::time::Duration,
}

/// Configuration for LimitAgreements Negotiator.
#[derive(StructOpt, Clone, Debug)]
pub struct CompositeNegotiatorConfig {
    #[structopt(flatten)]
    pub limit_agreements_config: LimitAgreementsNegotiatorConfig,
    #[structopt(flatten)]
    pub expire_agreements_config: AgreementExpirationNegotiatorConfig,
}

#[derive(StructOpt, Clone, Debug)]
pub struct NegotiatorsConfig {
    #[structopt(flatten)]
    pub composite_config: CompositeNegotiatorConfig,
}

pub fn create_negotiator(
    market: Addr<ProviderMarket>,
    config: &MarketConfig,
) -> Arc<NegotiatorAddr> {
    let negotiator = match &config.negotiator_type[..] {
        "LimitAgreements" => NegotiatorAddr::from(LimitAgreementsNegotiator::new(
            market,
            &config
                .negotiator_config
                .composite_config
                .limit_agreements_config,
        )),
        "Composite" => NegotiatorAddr::from(
            CompositeNegotiator::new(market, &config.negotiator_config.composite_config).unwrap(),
        ),
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
