#![allow(unused)]

use crate::counters::{Counter, CounterData, CounterReport};
use crate::error::CounterError;
use crate::message::{GetCounters, SetCounter, Shutdown};

use actix::prelude::*;
use chrono::{DateTime, Utc};

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ya_agreement_utils::AgreementView;

pub struct CountersServiceBuilder {
    usage_vector: Vec<String>,
    backlog_limit: Option<usize>,
    usage_limits: Option<HashMap<String, f64>>,
    counters: HashMap<String, Box<dyn Counter>>,
}

impl CountersServiceBuilder {
    pub fn new(usage_vector: Vec<String>, backlog_limit: Option<usize>) -> Self {
        let counters = Default::default();
        Self {
            usage_vector,
            backlog_limit,
            usage_limits: None,
            counters,
        }
    }

    /// usage_limit: map of counter id to max value
    pub fn with_usage_limits(&mut self, usage_limit: HashMap<String, f64>) -> &mut Self {
        self.usage_limits = Some(usage_limit);
        self
    }

    pub fn with_counter(&mut self, counter_id: &str, counter: Box<dyn Counter>) -> &mut Self {
        // overwriting an existing counter should not matter
        if self.counters.insert(counter_id.into(), counter).is_some() {
            log::warn!("Duplicated counter: {:?}", counter_id);
        }
        self
    }

    pub fn build(self) -> CountersService {
        let custom_counters_ids: Vec<String> = self
            .usage_vector
            .iter()
            .filter(|counter_id| !self.counters.contains_key(*counter_id))
            .cloned()
            .collect();

        if !custom_counters_ids.is_empty() {
            log::debug!("Custom counters: {:?}", custom_counters_ids)
        }

        let mut counters = HashMap::new();
        let usage_limits = self.usage_limits.unwrap_or_default();

        for custom_counter_id in custom_counters_ids {
            let usage_limit = usage_limits.get(&custom_counter_id).cloned();
            let counter = Box::<CustomCounter>::default();
            let provider = CounterProvider::new(counter, self.backlog_limit, usage_limit);
            counters.insert(custom_counter_id, provider);
        }

        for (counter_id, counter) in self.counters {
            let usage_limit = usage_limits.get(&counter_id).cloned();
            // non custom counters have backlog limit set to 1
            let counter_provider = CounterProvider::new(counter, Some(1), usage_limit);
            counters.insert(counter_id.clone(), counter_provider);
        }

        CountersService::new(self.usage_vector, counters)
    }
}

pub struct CountersService {
    usage_vector: Vec<String>,
    counters: HashMap<String, CounterProvider>,
}

impl CountersService {
    pub fn new(usage_vector: Vec<String>, counters: HashMap<String, CounterProvider>) -> Self {
        Self {
            usage_vector,
            counters,
        }
    }
}

impl Actor for CountersService {
    type Context = Context<Self>;
}

impl Handler<Shutdown> for CountersService {
    type Result = <Shutdown as Message>::Result;

    fn handle(&mut self, _: Shutdown, ctx: &mut Self::Context) -> Self::Result {
        ctx.stop();
        Ok(())
    }
}

impl Handler<GetCounters> for CountersService {
    type Result = <GetCounters as Message>::Result;

    fn handle(&mut self, _: GetCounters, _: &mut Self::Context) -> Self::Result {
        let mut counters = vec![0f64; self.usage_vector.len()];

        for (i, name) in self.usage_vector.iter().enumerate() {
            let counter = self
                .counters
                .get_mut(name)
                .ok_or_else(|| CounterError::Unsupported(name.to_string()))?;

            let report = counter.report();
            counter.log_report(report.clone());

            match report {
                CounterReport::Frame(data) => counters[i] = data,
                CounterReport::Error(error) => return Err(error),
                CounterReport::LimitExceeded(data) => {
                    return Err(CounterError::UsageLimitExceeded(format!(
                        "{:?} exceeded the value of {:?}",
                        name, data
                    )))
                }
            }
        }

        Ok::<_, CounterError>(counters)
    }
}

impl Handler<SetCounter> for CountersService {
    type Result = ();

    fn handle(&mut self, msg: SetCounter, ctx: &mut Self::Context) -> Self::Result {
        match self.counters.get_mut(&msg.name) {
            Some(provider) => provider.counter.set(msg.value),
            None => log::debug!("Unknown counter: {}", msg.name),
        }
    }
}

#[derive(Default)]
pub struct CustomCounter {
    val: CounterData,
    peak: CounterData,
}

impl Counter for CustomCounter {
    fn frame(&mut self) -> Result<CounterData, CounterError> {
        Ok(self.val)
    }

    fn peak(&mut self) -> Result<CounterData, CounterError> {
        Ok(self.peak)
    }

    fn set(&mut self, val: CounterData) {
        if val > self.peak {
            self.peak = val;
        }
        self.val = val;
    }
}

#[allow(clippy::type_complexity)]
pub struct CounterProvider {
    counter: Box<dyn Counter>,
    backlog: Arc<Mutex<VecDeque<(DateTime<Utc>, CounterReport)>>>,
    backlog_limit: Option<usize>,
    usage_limit: Option<CounterData>,
}

impl CounterProvider {
    pub fn new(
        counter: Box<dyn Counter>,
        backlog_limit: Option<usize>,
        usage_limit: Option<CounterData>,
    ) -> Self {
        CounterProvider {
            counter,
            backlog: Arc::new(Mutex::new(VecDeque::new())),
            backlog_limit,
            usage_limit,
        }
    }
}

impl CounterProvider {
    fn report(&mut self) -> CounterReport {
        if let Ok(data) = self.counter.peak() {
            if let Some(limit) = &self.usage_limit {
                if data > *limit {
                    return CounterReport::LimitExceeded(data);
                }
            }
        }

        match self.counter.frame() {
            Ok(data) => CounterReport::Frame(data),
            Err(error) => CounterReport::Error(error),
        }
    }

    fn log_report(&mut self, report: CounterReport) {
        let mut backlog = self.backlog.lock().unwrap();
        if let Some(limit) = self.backlog_limit {
            if backlog.len() == limit {
                backlog.pop_back();
            }
        }
        backlog.push_front((Utc::now(), report));
    }
}
