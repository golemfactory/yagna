use env_logger::TimestampPrecision;
use std::env;

pub fn enable_logs(enable: bool) {
    env::set_var(
        "RUST_LOG",
        env::var("RUST_LOG").unwrap_or_else(|_| {
            "info,web3=warn,sqlx_core=warn,hyper=warn,erc20_payment_lib=info,trust_dns_proto=warn,erc20_rpc_pool=info,trust_dns_resolver=warn,ya_erc20_driver=info".into()
        }),
    );
    if enable {
        env_logger::builder()
            .format_timestamp(Some(TimestampPrecision::Millis))
            .try_init()
            .ok();
    }
}
