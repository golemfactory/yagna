mod exporter;
mod metrics;
pub(crate) mod pusher;
pub mod service;

pub use service::{MetricsPusherOpts, MetricsService};

pub mod utils {
    const CRYPTOCURRENCY_PRECISION: u64 = 1000000000;
    use bigdecimal::ToPrimitive;
    pub fn cryptocurrency_to_u64(amount: &bigdecimal::BigDecimal) -> u64 {
        (amount * bigdecimal::BigDecimal::from(CRYPTOCURRENCY_PRECISION))
            .to_u64()
            .unwrap_or(u64::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test() {
        assert_eq!(
            88775939,
            utils::cryptocurrency_to_u64(
                &bigdecimal::BigDecimal::from_str("0.08877593981600002").unwrap()
            )
        );
    }
}
