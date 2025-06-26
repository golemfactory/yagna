use env_logger::{Env, TimestampPrecision};
use std::env;

pub fn enable_logs(enable: bool) {
    // Check if TEST_ENABLE_ALL_LOGS_OVERRIDE environment variable overrides the enable parameter
    let env_override = env::var("TEST_ENABLE_ALL_LOGS_OVERRIDE")
        .map(|val| val.parse::<bool>().unwrap_or(false))
        .unwrap_or(false);
    let should_enable = enable || env_override;

    if should_enable {
        if let Ok(_env) = env::var("RUST_LOG") {
            env_logger::try_init_from_env(Env::default()).ok();
        } else {
            env_logger::builder()
                .filter_level(log::LevelFilter::Debug)
                .filter(Some("web3"), log::LevelFilter::Warn)
                .filter(Some("sqlx"), log::LevelFilter::Info)
                .filter(Some("hyper"), log::LevelFilter::Warn)
                .filter(Some("erc20_payment_lib"), log::LevelFilter::Info)
                .filter(Some("trust_dns_proto"), log::LevelFilter::Warn)
                .filter(Some("erc20_rpc_pool"), log::LevelFilter::Info)
                .filter(Some("trust_dns_resolver"), log::LevelFilter::Warn)
                .filter(Some("ya_erc20_driver"), log::LevelFilter::Info)
                .format_timestamp(Some(TimestampPrecision::Millis))
                .try_init()
                .ok();
        }
    }
}
