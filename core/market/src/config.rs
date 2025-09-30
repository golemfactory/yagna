use anyhow::Result;
use clap::Parser;
use clap::ValueEnum;
use std::collections::HashMap;
use std::time::Duration;
use url::Url;
use ya_utils_cli::define_from_env;

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

#[derive(Parser, derive_more::Display, Clone, Debug, ValueEnum, PartialEq, Eq, Hash)]
pub enum GolemBaseNetwork {
    #[clap(name = "Kaolin")]
    Kaolin,
    #[clap(name = "Marketplace")]
    Marketplace,
    #[clap(name = "MarketplaceLoadTests")]
    MarketplaceLoadTests,
    #[clap(name = "Local")]
    Local,
    #[clap(name = "Custom")]
    Custom,
}

impl GolemBaseNetwork {
    pub fn default_config() -> HashMap<GolemBaseNetwork, GolemBaseRpcConfig> {
        let mut configs = HashMap::new();
        let default = GolemBaseRpcConfig {
            faucet_url: Url::parse("http://localhost:8545").unwrap(),
            rpc_url: Url::parse("http://localhost:8545").unwrap(),
            ws_url: Url::parse("ws://localhost:8545").unwrap(),
            l2_rpc_url: Url::parse("http://localhost:8555").unwrap(),
            chain_id: 1337,
            fund_preallocated: true,
        };

        // Configuration: https://kaolin.hoodi.arkiv.network/
        // Explorer: https://explorer.kaolin.hoodi.arkiv.network/
        configs.insert(
            GolemBaseNetwork::Kaolin,
            GolemBaseRpcConfig {
                faucet_url: Url::parse("https://kaolin.hoodi.arkiv.network/faucet/").unwrap(),
                rpc_url: Url::parse("https://kaolin.hoodi.arkiv.network/rpc").unwrap(),
                ws_url: Url::parse("wss://kaolin.hoodi.arkiv.network/rpc/ws").unwrap(),
                l2_rpc_url: Url::parse("https://l2.hoodi.arkiv.network/rpc").unwrap(),
                chain_id: 60138453025,
                fund_preallocated: false,
            },
        );
        // Configuration: https://marketplace.hoodi.arkiv.network/
        // Explorer: https://explorer.marketplace.hoodi.arkiv.network/
        configs.insert(
            GolemBaseNetwork::Marketplace,
            GolemBaseRpcConfig {
                faucet_url: Url::parse("https://marketplace.hoodi.arkiv.network/faucet/").unwrap(),
                rpc_url: Url::parse("https://marketplace.hoodi.arkiv.network/rpc").unwrap(),
                ws_url: Url::parse("wss://marketplace.hoodi.arkiv.network/rpc/ws").unwrap(),
                l2_rpc_url: Url::parse("https://l2.hoodi.arkiv.network/rpc").unwrap(),
                chain_id: 60138453027,
                fund_preallocated: false,
            },
        );
        // Configuration: https://marketplaceloadtests.hoodi.arkiv.network/
        // Explorer: https://explorer.marketplaceloadtests.hoodi.arkiv.network/
        configs.insert(
            GolemBaseNetwork::MarketplaceLoadTests,
            GolemBaseRpcConfig {
                faucet_url: Url::parse("https://marketplaceloadtests.hoodi.arkiv.network/faucet/")
                    .unwrap(),
                rpc_url: Url::parse("https://marketplaceloadtests.hoodi.arkiv.network/rpc")
                    .unwrap(),
                ws_url: Url::parse("wss://marketplaceloadtests.hoodi.arkiv.network/rpc/ws")
                    .unwrap(),
                l2_rpc_url: Url::parse("https://l2.hoodi.arkiv.network/rpc").unwrap(),
                chain_id: 60138453005,
                fund_preallocated: false,
            },
        );
        configs.insert(GolemBaseNetwork::Local, default.clone());
        configs.insert(
            GolemBaseNetwork::Custom,
            GolemBaseRpcConfig::from_env()
                .map_err(|e| {
                    log::error!("Error parsing GolemBase configuration: {e}");
                })
                .unwrap_or_else(|_| default.clone()),
        );
        configs
    }
}

#[derive(Parser, Clone, Debug)]
pub struct GolemBaseRpcConfig {
    #[clap(env = "GOLEM_BASE_CUSTOM_FAUCET_URL", value_parser = parse_url, default_value = "http://localhost:8545")]
    pub faucet_url: Url,
    #[clap(env = "GOLEM_BASE_CUSTOM_RPC_URL", value_parser = parse_url, default_value = "http://localhost:8545")]
    pub rpc_url: Url,
    #[clap(env = "GOLEM_BASE_CUSTOM_WS_URL", value_parser = parse_url, default_value = "ws://localhost:8545")]
    pub ws_url: Url,
    #[clap(env = "GOLEM_BASE_CUSTOM_L2_RPC_URL", value_parser = parse_url, default_value = "http://localhost:8545")]
    pub l2_rpc_url: Url,
    #[clap(env = "GOLEM_BASE_CUSTOM_CHAIN_ID", default_value = "1337")]
    pub chain_id: u64,
    // In local developer GolemBase environment, pre-allocated account is available to fund other accounts.
    #[clap(
        env = "GOLEM_BASE_CUSTOM_FUND_PREALLOCATED",
        default_value_t = false,
        long
    )]
    pub fund_preallocated: bool,
}

define_from_env!(GolemBaseRpcConfig);

#[derive(Parser, Clone, Debug)]
pub struct DiscoveryConfig {
    #[clap(skip = GolemBaseNetwork::default_config())]
    pub configs: HashMap<GolemBaseNetwork, GolemBaseRpcConfig>,
    #[clap(env = "GOLEM_BASE_NETWORK", default_value = "Marketplace")]
    pub network: GolemBaseNetwork,
    // PoW faucets require to compute PoW solutions. This variable determines how many threads
    // will be used to compute solutions. Note that this is margin realtive to maximal avaiable
    // threads. If machine has N cores, then N - GOLEM_BASE_FUND_POW_THREADS_MARGIN will be used.
    #[clap(env = "GOLEM_BASE_FUND_POW_THREADS_MARGIN", default_value = "2")]
    pub pow_threads_margin: usize,
    /// Timeout for publishing offers on the market
    #[clap(env = "GOLEM_BASE_OFFER_PUBLISH_TIMEOUT", value_parser = humantime::parse_duration, default_value = "120s")]
    pub offer_publish_timeout: Duration,
    /// Number of retries for GolemBase transactions
    #[clap(env = "GOLEM_BASE_PUBLISH_MAX_RETRIES", default_value = "2")]
    pub publish_max_retries: u32,
    /// Number of confirmations required for GolemBase transactions
    #[clap(env = "GOLEM_BASE_REQUIRED_CONFIRMATIONS", default_value = "1")]
    pub required_confirmations: u64,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            configs: GolemBaseNetwork::default_config(),
            network: GolemBaseNetwork::Kaolin,
            pow_threads_margin: 2,
            offer_publish_timeout: Duration::from_secs(30),
            publish_max_retries: 3,
            required_confirmations: 1,
        }
    }
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

    pub fn get_pow_threads(&self) -> usize {
        std::cmp::max(1, num_cpus::get() - self.pow_threads_margin)
    }

    pub fn fund_preallocated(&self) -> bool {
        self.configs.get(&self.network).unwrap().fund_preallocated
    }

    pub fn get_chain_id(&self) -> u64 {
        self.configs.get(&self.network).unwrap().chain_id
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

define_from_env!(Config);

define_from_env!(DiscoveryConfig);

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
