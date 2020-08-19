use chrono::Duration;

/// TODO: Decide where should this config be loaded from.
///  We could deserialize it from .json file or use structopt and
///  configure market through env variables.
pub struct Config {
    pub discovery: DiscoveryConfig,
    pub subscription: SubscriptionConfig,
}

pub struct DiscoveryConfig {
    pub num_broadcasted_offers: u32,
    pub num_broadcasted_unsubscribes: u32,
    pub mean_random_broadcast_interval: Duration,
    pub mean_random_broadcast_unsubscribes_interval: Duration,
}

pub struct SubscriptionConfig {
    pub default_ttl: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            discovery: DiscoveryConfig::default(),
            subscription: SubscriptionConfig::default(),
        }
    }
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        DiscoveryConfig {
            num_broadcasted_offers: 50,
            num_broadcasted_unsubscribes: 50,
            mean_random_broadcast_interval: Duration::seconds(3),
            mean_random_broadcast_unsubscribes_interval: Duration::seconds(3),
        }
    }
}

impl Default for SubscriptionConfig {
    fn default() -> Self {
        SubscriptionConfig {
            default_ttl: Duration::hours(24),
        }
    }
}
