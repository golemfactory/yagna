use std::time::Duration;

/// TODO: Decide where should this config be loaded from.
///  We could deserialize it from .json file or use structopt and
///  configure market through env variables.
pub struct Config {
    pub discovery: DiscoveryConfig,
}

pub struct DiscoveryConfig {
    pub num_broadcasted_offers: u32,
    pub mean_random_broadcast_interval: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            discovery: DiscoveryConfig::default(),
        }
    }
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        DiscoveryConfig {
            num_broadcasted_offers: 50,
            mean_random_broadcast_interval: Duration::from_secs_f64(3.0 * 60.0),
        }
    }
}
