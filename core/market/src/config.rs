use clap::Parser;
use std::time::Duration;
use url::Url;

#[derive(Parser, Clone)]
pub struct Config {
    #[clap(flatten)]
    pub discovery: DiscoveryConfig,
    #[clap(flatten)]
    pub subscription: SubscriptionConfig,
    #[clap(flatten)]
    pub events: EventsConfig,
    #[clap(flatten)]
    pub db: DbConfig,
}

#[derive(Parser, Clone, Debug)]
pub struct DiscoveryConfig {
    #[clap(env, value_parser = parse_url, default_value = "http://localhost:8545")]
    pub golem_base_rpc_url: Url,
    #[clap(env, value_parser = parse_url, default_value = "ws://localhost:8545")]
    pub golem_base_ws_url: Url,
}

#[derive(Parser, Clone)]
pub struct SubscriptionConfig {
    #[clap(env = "DEFAULT_SUBSCRIPTION_TTL", value_parser = parse_chrono_duration, default_value = "1h")]
    pub default_ttl: chrono::Duration,
}

#[derive(Parser, Clone)]
pub struct EventsConfig {
    #[clap(env = "MARKET_MAX_EVENTS_DEFAULT", default_value = "20")]
    pub max_events_default: i32,
    #[clap(env = "MARKET_MAX_EVENTS_MAX", default_value = "100")]
    pub max_events_max: i32,
}

#[derive(Parser, Clone)]
pub struct DbConfig {
    /// Interval in which Market cleaner will be invoked
    #[clap(env = "MARKET_DB_CLEANUP_INTERVAL", value_parser = humantime::parse_duration, default_value = "4h")]
    pub cleanup_interval: Duration,
    /// Number of days to persist Agreements and related Agreement Events
    #[clap(env = "MARKET_AGREEMENT_STORE_DAYS", default_value = "90")]
    pub agreement_store_days: i32,
    /// Number of days to persist Negotiation Events
    #[clap(env = "MARKET_EVENT_STORE_DAYS", default_value = "1")]
    pub event_store_days: i32,
}

impl Config {
    pub fn from_env() -> Result<Config, clap::Error> {
        // Empty command line arguments, because we want to use ENV fallback
        // or default values if ENV variables are not set.
        Config::try_parse_from(&[""])
    }
}

fn parse_chrono_duration(s: &str) -> Result<chrono::Duration, anyhow::Error> {
    Ok(chrono::Duration::from_std(humantime::parse_duration(s)?)?)
}

fn parse_url(s: &str) -> Result<Url, anyhow::Error> {
    Ok(Url::parse(s)?)
}

#[cfg(test)]
mod test {
    use super::Config;

    #[test]
    fn test_default_clap_subscription_ttl() {
        let c = Config::from_env().unwrap();
        assert_eq!(60, c.subscription.default_ttl.num_minutes());
    }

    #[test]
    fn test_default_clap_events() {
        let c = Config::from_env().unwrap();
        assert_eq!(20, c.events.max_events_default);
        assert_eq!(100, c.events.max_events_max);
    }

    #[test]
    fn test_default_clap_db_config() {
        let c = Config::from_env().unwrap();
        assert_eq!(4 * 3600, c.db.cleanup_interval.as_secs());
        assert_eq!(90, c.db.agreement_store_days);
        assert_eq!(1, c.db.event_store_days);
    }
}
