#[allow(unused_imports)]
use crate::ExeUnitContext;

use ya_counters::service::{CountersService, CountersServiceBuilder};
use ya_counters::{Counter, TimeCounter};
#[cfg(not(feature = "sgx"))]
use ya_counters::{CpuCounter, MemCounter, StorageCounter};

use std::collections::HashMap;

#[allow(unused_imports)]
use std::time::Duration;

pub fn build(
    ctx: &ExeUnitContext,
    backlog_limit: Option<usize>,
    supervise_caps: bool,
) -> CountersService {
    let mut builder =
        CountersServiceBuilder::new(ctx.agreement.usage_vector.clone(), backlog_limit);

    if supervise_caps {
        builder.with_usage_limits(ctx.agreement.usage_limits.clone());
    }

    for (counter_id, counter) in counters(ctx) {
        builder.with_counter(&counter_id, counter);
    }

    builder.build()
}

#[cfg(feature = "sgx")]
pub fn usage_vector() -> Vec<String> {
    vec![TimeCounter::ID.to_string()]
}

#[cfg(feature = "sgx")]
fn counters(_ctx: &ExeUnitContext) -> HashMap<String, Box<dyn Counter>> {
    vec![(
        TimeCounter::ID.to_string(),
        Box::<TimeCounter>::default() as Box<dyn Counter>,
    )]
    .into_iter()
    .collect()
}

#[cfg(not(feature = "sgx"))]
pub fn usage_vector() -> Vec<String> {
    vec![
        TimeCounter::ID.to_string(),
        CpuCounter::ID.to_string(),
        MemCounter::ID.to_string(),
        StorageCounter::ID.to_string(),
    ]
    .into_iter()
    .collect()
}

#[cfg(not(feature = "sgx"))]
fn counters(ctx: &ExeUnitContext) -> HashMap<String, Box<dyn Counter>> {
    vec![
        (
            CpuCounter::ID.to_string(),
            Box::new(CpuCounter::default()) as Box<dyn Counter>,
        ),
        (
            MemCounter::ID.to_string(),
            Box::new(MemCounter::default()) as Box<dyn Counter>,
        ),
        (
            StorageCounter::ID.to_string(),
            Box::new(StorageCounter::new(
                ctx.work_dir.clone(),
                Duration::from_secs(60 * 5),
            )) as Box<dyn Counter>,
        ),
        (
            TimeCounter::ID.to_string(),
            Box::new(TimeCounter::default()) as Box<dyn Counter>,
        ),
    ]
    .into_iter()
    .collect()
}
