use structopt::StructOpt;

use crate::market::negotiator::factory::NegotiatorsConfig;

/// Configuration for ProviderMarket actor.
#[derive(StructOpt, Clone)]
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
    #[structopt(long, env, parse(try_from_str = humantime::parse_duration), default_value = "5s")]
    pub agreement_termination_backoff_initial: std::time::Duration,
    #[structopt(long, env, parse(try_from_str = humantime::parse_duration), default_value = "4h")]
    pub agreement_termination_backoff_max: std::time::Duration,
    #[structopt(long, env, parse(try_from_str = humantime::parse_duration), default_value = "5s")]
    pub resubscribe_backoff_initial: std::time::Duration,
    #[structopt(long, env, parse(try_from_str = humantime::parse_duration), default_value = "1h")]
    pub resubscribe_backoff_max: std::time::Duration,
}

// TODO: Change to use clap::Parser and define_from_env! macro in the future
impl MarketConfig {
    pub fn from_env() -> Result<MarketConfig, structopt::clap::Error> {
        // Empty command line arguments, because we want to use ENV fallback
        // or default values if ENV variables are not set.
        MarketConfig::from_iter_safe([""])
    }

    pub fn get_backoff(&self) -> backoff::ExponentialBackoff {
        backoff::ExponentialBackoff {
            current_interval: self.agreement_termination_backoff_initial,
            initial_interval: self.agreement_termination_backoff_initial,
            multiplier: 1.5f64,
            max_interval: self.agreement_termination_backoff_max,
            max_elapsed_time: Some(std::time::Duration::from_secs(u64::MAX)),
            ..Default::default()
        }
    }

    pub fn get_resubscribe_backoff(&self) -> backoff::ExponentialBackoff {
        backoff::ExponentialBackoff {
            current_interval: self.resubscribe_backoff_initial,
            initial_interval: self.resubscribe_backoff_initial,
            multiplier: 1.5f64,
            max_interval: self.resubscribe_backoff_max,
            max_elapsed_time: Some(std::time::Duration::from_secs(u64::MAX)),
            ..Default::default()
        }
    }
}
