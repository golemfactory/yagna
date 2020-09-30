mod exporter;
mod metrics;
mod service;

pub use service::MetricsService;

pub mod utils {
    const CRYPTOCURRENCY_PRECISION: u64 = 1000000000;
    use bigdecimal::ToPrimitive;
    pub fn cryptocurrency_to_u64(amount: &bigdecimal::BigDecimal) -> u64 {
        (amount * bigdecimal::BigDecimal::from(CRYPTOCURRENCY_PRECISION))
            .with_prec(0)
            .to_u64()
            .unwrap_or(u64::MAX)
    }
}
