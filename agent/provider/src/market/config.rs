use structopt::StructOpt;

use crate::market::negotiator::factory::NegotiatorsConfig;

/// Configuration for ProviderMarket actor.
#[derive(StructOpt, Clone, Debug)]
pub struct MarketConfig {
    #[structopt(long, env, default_value = "20.0")]
    pub agreement_events_interval: f32,
    #[structopt(long, env, default_value = "20.0")]
    pub negotiation_events_interval: f32,
    #[structopt(long, env, default_value = "10.0")]
    pub agreement_approve_timeout: f32,
    #[structopt(long, env, default_value = "Composite")]
    pub negotiator_type: String,
    #[structopt(flatten)]
    pub negotiator_config: NegotiatorsConfig,
    #[structopt(skip = "you-forgot-to-set-session-id")]
    pub session_id: String,
    #[structopt(long, env, parse(try_from_str = humantime::parse_duration), default_value = "20s")]
    pub process_market_events_timeout: std::time::Duration,
}
