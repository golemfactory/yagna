use std::env;
use std::time::Duration;
use structopt::StructOpt;

pub fn is_market_memory_on_disk() -> bool {
    // This options is used to troubleshoot memory issues with market.
    // Use only for debugging purposes, default value is false.
    let str = env::var("MARKET_MEMORY_USE_DISK")
        .unwrap_or_default()
        .to_lowercase();
    str == "true" || str == "1"
}

#[derive(StructOpt, Clone)]
pub struct Config {
    #[structopt(flatten)]
    pub discovery: DiscoveryConfig,
    #[structopt(flatten)]
    pub subscription: SubscriptionConfig,
    #[structopt(flatten)]
    pub events: EventsConfig,
    #[structopt(flatten)]
    pub db: DbConfig,
}

#[derive(StructOpt, Clone)]
pub struct DiscoveryConfig {
    // don't set this value higher than SQLITE_MAX_VARIABLE_NUMBER, which defaults to 999 for SQLite versions prior to 3.32.0 (2020-05-22)
    #[structopt(env, default_value = "200")]
    pub max_bcasted_offers: u32,
    #[structopt(env, default_value = "200")]
    pub max_bcasted_unsubscribes: u32,
    /// If number of broadcasts at the same time exceeds this value, than all new broadcasts will be dropped.  
    #[structopt(env, default_value = "14")]
    pub bcast_receiving_queue_size: usize,
    #[structopt(env, parse(try_from_str = humantime::parse_duration), default_value = "4min")]
    pub mean_cyclic_bcast_interval: Duration,
    #[structopt(env, parse(try_from_str = humantime::parse_duration), default_value = "4min")]
    pub mean_cyclic_unsubscribes_interval: Duration,
    #[structopt(env, parse(try_from_str = humantime::parse_duration), default_value = "5sec")]
    pub offer_broadcast_delay: Duration,
    #[structopt(env, parse(try_from_str = humantime::parse_duration), default_value = "5sec")]
    pub unsub_broadcast_delay: Duration,
    #[structopt(env, parse(try_from_str = humantime::parse_duration), default_value = "50ms")]
    pub bcast_tile_time_margin: Duration,
    #[structopt(env, parse(try_from_str = humantime::parse_duration), default_value = "300s")]
    pub bcast_node_ban_timeout: Duration,
}

#[derive(StructOpt, Clone)]
pub struct SubscriptionConfig {
    #[structopt(env = "DEFAULT_SUBSCRIPTION_TTL", parse(try_from_str = parse_chrono_duration), default_value = "1h")]
    pub default_ttl: chrono::Duration,
}

#[derive(StructOpt, Clone)]
pub struct EventsConfig {
    #[structopt(env = "MARKET_MAX_EVENTS_DEFAULT", default_value = "20")]
    pub max_events_default: i32,
    #[structopt(env = "MARKET_MAX_EVENTS_MAX", default_value = "100")]
    pub max_events_max: i32,
}

#[derive(StructOpt, Clone)]
pub struct DbConfig {
    /// Interval in which Market cleaner will be invoked
    #[structopt(env = "MARKET_DB_CLEANUP_INTERVAL", parse(try_from_str = humantime::parse_duration), default_value = "4h")]
    pub cleanup_interval: Duration,
    /// Number of days to persist Agreements and related Agreement Events
    #[structopt(env = "MARKET_AGREEMENT_STORE_DAYS", default_value = "90")]
    pub agreement_store_days: i32,
    /// Number of days to persist Negotiation Events
    #[structopt(env = "MARKET_EVENT_STORE_DAYS", default_value = "1")]
    pub event_store_days: i32,
}

impl Config {
    pub fn from_env() -> Result<Config, structopt::clap::Error> {
        // Empty command line arguments, because we want to use ENV fallback
        // or default values if ENV variables are not set.
        Config::from_iter_safe(&[""])
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

    #[test]
    fn test_default_structopt_db_config() {
        let c = Config::from_env().unwrap();
        assert_eq!(4 * 3600, c.db.cleanup_interval.as_secs());
        assert_eq!(90, c.db.agreement_store_days);
        assert_eq!(1, c.db.event_store_days);
    }
}
