use structopt::StructOpt;

/// Configuration for ProviderMarket actor.
#[derive(StructOpt, Clone, Debug)]
pub struct MarketConfig {
    #[structopt(long, env, default_value = "2.0")]
    pub agreement_events_interval: f32,
    #[structopt(long, env, default_value = "2.0")]
    pub negotiation_events_interval: f32,
    #[structopt(long, env, default_value = "10.0")]
    pub agreement_approve_timeout: f32,
    #[structopt(long, env, default_value = "LimitAgreements")]
    pub negotiator_type: String,
    #[structopt(skip = "you-forgot-to-set-session-id")]
    pub session_id: String,
}
