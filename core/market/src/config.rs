use std::time::Duration;

/// TODO: Decide where should this config be loaded from.
///  We could deserialize it from .json file or use structopt and
///  configure market through env variables.
#[derive(Default)]
pub struct Config {
    pub discovery: DiscoveryConfig,
    pub subscription: SubscriptionConfig,
    pub events: EventsConfig,
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

pub struct EventsConfig {
    pub max_events_default: i32,
    pub max_events_max: i32,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        DiscoveryConfig {
            max_bcasted_offers: 200,
            max_bcasted_unsubscribes: 200,
            mean_cyclic_bcast_interval: Duration::from_secs(240),
            mean_cyclic_unsubscribes_interval: Duration::from_secs(240),
        }
    }
}

impl Default for SubscriptionConfig {
    fn default() -> Self {
        SubscriptionConfig {
            default_ttl: chrono::Duration::hours(1),
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
