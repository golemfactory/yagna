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
    pub max_bcasted_offers: u32,
    pub max_bcasted_unsubscribes: u32,
    pub mean_cyclic_bcast_interval: Duration,
    pub mean_cyclic_unsubscribes_interval: Duration,
}

pub struct SubscriptionConfig {
    pub default_ttl: chrono::Duration,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        DiscoveryConfig {
            max_bcasted_offers: 50,
            max_bcasted_unsubscribes: 50,
            mean_cyclic_bcast_interval: Duration::from_secs(3),
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
