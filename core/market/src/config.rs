use std::time::Duration;
use structopt::StructOpt;

#[derive(StructOpt, Clone)]
pub struct Config {
    #[structopt(flatten)]
    pub discovery: DiscoveryConfig,
    #[structopt(flatten)]
    pub subscription: SubscriptionConfig,
    #[structopt(flatten)]
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

#[derive(StructOpt, Clone)]
pub struct EventsConfig {
    #[structopt(env = "MAX_MARKET_EVENTS_DEFAULT", default_value = "20")]
    pub max_events_default: i32,
    #[structopt(env = "MAX_MARKET_EVENTS_MAX", default_value = "100")]
    pub max_events_max: i32,
}

impl Config {
    pub fn from_env() -> Result<Config, structopt::clap::Error> {
        // Empty command line arguments, because we want to use ENV fallback
        // or default values if ENV variables are not set.
        Ok(Config::from_iter_safe(&[""])?)
    }
}

fn parse_chrono_duration(s: &str) -> Result<chrono::Duration, anyhow::Error> {
    Ok(chrono::Duration::from_std(humantime::parse_duration(s)?)?)
}

#[cfg(test)]
mod test {
    use super::Config;

    #[test]
    fn test_default_structopt_subscription_ttl() {
        let c = Config::from_env().unwrap();
        assert_eq!(60, c.subscription.default_ttl.num_minutes());
    }

    #[test]
    fn test_default_structopt_events() {
        let c = Config::from_env().unwrap();
        assert_eq!(20, c.events.max_events_default);
        assert_eq!(100, c.events.max_events_max);
    }
}
