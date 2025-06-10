use anyhow::Result;
use clap::Parser;
use clap::ValueEnum;
use std::collections::HashMap;
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

#[derive(Parser, Clone, Debug, ValueEnum, PartialEq, Eq, Hash)]
pub enum GolemBaseNetwork {
    #[clap(name = "Kaolin")]
    Kaolin,
    #[clap(name = "Marketplace")]
    Marketplace,
    #[clap(name = "Local")]
    Local,
}

impl GolemBaseNetwork {
    pub fn default_config() -> HashMap<GolemBaseNetwork, GolemBaseRpcConfig> {
        let mut configs = HashMap::new();
        configs.insert(
            GolemBaseNetwork::Kaolin,
            GolemBaseRpcConfig {
                faucet_url: Url::parse("https://faucet.kaolin.holesky.golem-base.io/").unwrap(),
                rpc_url: Url::parse("https://rpc.kaolin.holesky.golem-base.io/").unwrap(),
                ws_url: Url::parse("wss://ws.rpc.kaolin.holesky.golem-base.io/").unwrap(),
                l2_rpc_url: Url::parse("https://execution.holesky.l2.gobas.me").unwrap(),
            },
        );
        // Configuration: https://marketplace.holesky.golem-base.io/
        configs.insert(
            GolemBaseNetwork::Marketplace,
            GolemBaseRpcConfig {
                faucet_url: Url::parse("https://marketplace.holesky.golem-base.io/faucet/")
                    .unwrap(),
                rpc_url: Url::parse("https://marketplace.holesky.golem-base.io/rpc").unwrap(),
                ws_url: Url::parse("wss://marketplace.holesky.golem-base.io/rpc/ws").unwrap(),
                l2_rpc_url: Url::parse("https://execution.holesky.l2.gobas.me").unwrap(),
            },
        );
        configs.insert(
            GolemBaseNetwork::Local,
            GolemBaseRpcConfig {
                faucet_url: Url::parse("http://localhost:8545").unwrap(),
                rpc_url: Url::parse("http://localhost:8545").unwrap(),
                ws_url: Url::parse("ws://localhost:8545").unwrap(),
                l2_rpc_url: Url::parse("http://localhost:8555").unwrap(),
            },
        );
        configs
    }
}

#[derive(Parser, Clone, Debug)]
pub struct GolemBaseRpcConfig {
    #[clap(env, value_parser = parse_url, default_value = "http://localhost:8545")]
    pub faucet_url: Url,
    #[clap(env, value_parser = parse_url, default_value = "http://localhost:8545")]
    pub rpc_url: Url,
    #[clap(env, value_parser = parse_url, default_value = "ws://localhost:8545")]
    pub ws_url: Url,
    #[clap(env, value_parser = parse_url, default_value = "http://localhost:8545")]
    pub l2_rpc_url: Url,
}

#[derive(Parser, Clone, Debug)]
pub struct DiscoveryConfig {
    #[clap(skip = GolemBaseNetwork::default_config())]
    pub configs: HashMap<GolemBaseNetwork, GolemBaseRpcConfig>,
    #[clap(env = "GOLEM_BASE_NETWORK", default_value = "Marketplace")]
    pub network: GolemBaseNetwork,
}

impl DiscoveryConfig {
    pub fn get_network_type(&self) -> &GolemBaseNetwork {
        &self.network
    }

    pub fn get_rpc_url(&self) -> &Url {
        &self.configs.get(&self.network).unwrap().rpc_url
    }

    pub fn get_ws_url(&self) -> &Url {
        &self.configs.get(&self.network).unwrap().ws_url
    }

    pub fn get_faucet_url(&self) -> &Url {
        &self.configs.get(&self.network).unwrap().faucet_url
    }

    pub fn get_l2_rpc_url(&self) -> &Url {
        &self.configs.get(&self.network).unwrap().l2_rpc_url
    }
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            configs: GolemBaseNetwork::default_config(),
            network: GolemBaseNetwork::Kaolin,
        }
    }
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
        Config::try_parse_from([""])
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
