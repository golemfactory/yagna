use std::time::Duration;

/// TODO: Decide where should this config be loaded from.
///  We could deserialize it from .json file or use structopt and
///  configure market through env variables.
#[derive(Default)]
pub struct Config {
    pub discovery: DiscoveryConfig,
    pub subscription: SubscriptionConfig,
}

pub struct DiscoveryConfig {
    pub num_broadcasted_offers: u32,
    pub num_broadcasted_unsubscribes: u32,
    pub mean_cyclic_broadcast_interval: Duration,
    pub mean_cyclic_unsubscribes_interval: Duration,
}

pub struct SubscriptionConfig {
    pub default_ttl: chrono::Duration,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        DiscoveryConfig {
            num_broadcasted_offers: 50,
            num_broadcasted_unsubscribes: 50,
            mean_cyclic_broadcast_interval: Duration::from_secs(3),
            mean_cyclic_unsubscribes_interval: Duration::from_secs(3),
        }
    }
}

impl Default for SubscriptionConfig {
    fn default() -> Self {
        SubscriptionConfig {
            default_ttl: chrono::Duration::hours(24),
        }
    }
}
