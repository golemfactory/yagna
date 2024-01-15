use env_logger::TimestampPrecision;
use std::env;

pub fn enable_logs(enable: bool) {
    env::set_var(
        "RUST_LOG",
        env::var("RUST_LOG").unwrap_or_else(|_| "debug".into()),
    );
    if enable {
        env_logger::builder()
            .format_timestamp(Some(TimestampPrecision::Millis))
            .try_init()
            .ok();
    }
}
