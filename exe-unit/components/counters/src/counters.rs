use std::fmt::Debug;
use std::time::SystemTime;

use crate::error::{self, CounterError};

pub type Result<T> = std::result::Result<T, error::CounterError>;
pub type CounterData = f64;

#[derive(Clone, Debug)]
pub enum CounterReport {
    Frame(CounterData),
    LimitExceeded(CounterData),
    Error(error::CounterError),
}

pub trait Counter {
    fn frame(&mut self) -> Result<CounterData>;
    fn peak(&mut self) -> Result<CounterData>;
    fn set(&mut self, _value: CounterData) {}
}

pub struct TimeCounter {
    started: SystemTime,
}

impl TimeCounter {
    pub const ID: &'static str = "golem.usage.duration_sec";
}

impl Default for TimeCounter {
    fn default() -> Self {
        TimeCounter {
            started: SystemTime::now(),
        }
    }
}

impl Counter for TimeCounter {
    fn frame(&mut self) -> Result<CounterData> {
        Ok(SystemTime::now()
            .duration_since(self.started)
            .map_err(|err| CounterError::Other(err.to_string()))?
            .as_secs_f64())
    }

    fn peak(&mut self) -> Result<CounterData> {
        self.frame()
    }
}
