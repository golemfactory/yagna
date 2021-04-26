use std::time::Duration;
use structopt::StructOpt;

#[derive(StructOpt, Clone)]
pub struct Config {
    #[structopt(flatten)]
    pub discovery: DiscoveryConfig,
    #[structopt(skip)]
    pub subscription: SubscriptionConfig,
    #[structopt(skip)]
    pub events: EventsConfig,
}

#[derive(StructOpt, Clone)]
pub struct DiscoveryConfig {
    #[structopt(env, default_value = "200")]
    pub max_bcasted_offers: u32,
    #[structopt(env, default_value = "200")]
    pub max_bcasted_unsubscribes: u32,
    #[structopt(env, parse(try_from_str = humantime::parse_duration), default_value = "4min")]
    pub mean_cyclic_bcast_interval: Duration,
    #[structopt(env, parse(try_from_str = humantime::parse_duration), default_value = "4min")]
    pub mean_cyclic_unsubscribes_interval: Duration,
    #[structopt(env, parse(try_from_str = humantime::parse_duration), default_value = "5sec")]
    pub offer_broadcast_delay: Duration,
    #[structopt(env, parse(try_from_str = humantime::parse_duration), default_value = "5sec")]
    pub unsub_broadcast_delay: Duration,
}

#[derive(StructOpt, Clone)]
pub struct SubscriptionConfig {
    #[structopt(env = "DEFAULT_SUBSCRIPTION_TTL", parse(try_from_str = parse_chrono_duration), default_value = "1h")]
    pub default_ttl: chrono::Duration,
}

#[derive(Clone)]
pub struct EventsConfig {
    pub max_events_default: i32,
    pub max_events_max: i32,
}

impl Config {
    pub fn from_env() -> Result<Config, structopt::clap::Error> {
        // Mock command line arguments, because we want to use ENV fallback
        // or default values if ENV variables don't exist.
        Ok(Config::from_iter_safe(vec!["yagna"].iter())?)
    }
}

impl Default for SubscriptionConfig {
    fn default() -> Self {
        SubscriptionConfig {
            default_ttl: chrono::Duration::seconds(50),
        }
    }
}

impl Default for EventsConfig {
    fn default() -> Self {
        EventsConfig {
            max_events_default: 20,
            max_events_max: 100,
        }
    }
}

fn parse_chrono_duration(s: &str) -> Result<chrono::Duration, anyhow::Error> {
    Ok(chrono::Duration::from_std(humantime::parse_duration(s)?)?)
}
