use actix::Addr;
use humantime;
use std::sync::Arc;
use structopt::StructOpt;

use ya_manifest_utils::PolicyConfig;

use super::common::NegotiatorAddr;
use crate::market::config::MarketConfig;
use crate::market::negotiator::{AcceptAllNegotiator, CompositeNegotiator};
use crate::market::ProviderMarket;
use crate::provider_agent::AgentNegotiatorsConfig;

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
    #[structopt(long, env, parse(try_from_str = humantime::parse_duration), default_value = "100years")]
    pub max_agreement_expiration: std::time::Duration,
    #[structopt(long, env, parse(try_from_str = humantime::parse_duration), default_value = "30min")]
    pub max_agreement_expiration_without_deadline: std::time::Duration,
    #[structopt(long, env, parse(try_from_str = humantime::parse_duration), default_value = "4min")]
    pub debit_note_acceptance_deadline: std::time::Duration,
}

/// Configuration for DebitNoteInterval negotiator
#[derive(StructOpt, Clone, Debug)]
pub struct DebitNoteIntervalConfig {
    #[structopt(long, env, parse(try_from_str = humantime::parse_duration), default_value = "1s")]
    pub min_debit_note_interval: std::time::Duration,
    #[structopt(long, env, parse(try_from_str = humantime::parse_duration), default_value = "24h")]
    pub max_debit_note_interval: std::time::Duration,
    #[structopt(long, env, parse(try_from_str = humantime::parse_duration), default_value = "2min")]
    pub debit_note_interval: std::time::Duration,
}

/// Configuration for PaymentTimeout negotiator
#[derive(StructOpt, Clone, Debug)]
pub struct PaymentTimeoutConfig {
    #[structopt(long, env, parse(try_from_str = humantime::parse_duration), default_value = "1s")]
    pub min_payment_timeout: std::time::Duration,
    #[structopt(long, env, parse(try_from_str = humantime::parse_duration), default_value = "24h")]
    pub max_payment_timeout: std::time::Duration,
    #[structopt(long, env, parse(try_from_str = humantime::parse_duration), default_value = "2min")]
    pub payment_timeout: std::time::Duration,
    #[structopt(long, env, parse(try_from_str = humantime::parse_duration), default_value = "10h")]
    pub payment_timeout_required_duration: std::time::Duration,
}

/// Configuration for LimitAgreements Negotiator.
#[derive(StructOpt, Clone, Debug)]
pub struct CompositeNegotiatorConfig {
    #[structopt(flatten)]
    pub limit_agreements_config: LimitAgreementsNegotiatorConfig,
    #[structopt(flatten)]
    pub expire_agreements_config: AgreementExpirationNegotiatorConfig,
    #[structopt(flatten)]
    pub debit_note_interval_config: DebitNoteIntervalConfig,
    #[structopt(flatten)]
    pub payment_timeout_config: PaymentTimeoutConfig,
    #[structopt(flatten)]
    pub policy_config: PolicyConfig,
}

#[derive(StructOpt, Clone, Debug)]
pub struct NegotiatorsConfig {
    #[structopt(flatten)]
    pub composite_config: CompositeNegotiatorConfig,
}

pub fn create_negotiator(
    market: Addr<ProviderMarket>,
    config: &MarketConfig,
    agent_negotiators_cfg: &AgentNegotiatorsConfig,
) -> Arc<NegotiatorAddr> {
    let negotiator = match &config.negotiator_type[..] {
        "Composite" => NegotiatorAddr::from(
            CompositeNegotiator::new(
                market,
                &config.negotiator_config.composite_config,
                agent_negotiators_cfg.clone(),
            )
            .unwrap(),
        ),
        "AcceptAll" => NegotiatorAddr::from(AcceptAllNegotiator::default()),
        _ => Default::default(),
    };
    Arc::new(negotiator)
}

impl Default for NegotiatorAddr {
    fn default() -> Self {
        NegotiatorAddr::from(AcceptAllNegotiator::default())
    }
}
