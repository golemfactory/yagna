use anyhow::Result;
use chrono::{DateTime, Utc};
use std::time::Duration;

/// Interval date provider with reference to
/// current date and another baseline date.
pub struct RelativeInterval {
    pub base: DateTime<Utc>,
    pub iteration: i32,
    pub interval: chrono::Duration,
}

impl RelativeInterval {
    pub fn new(base: DateTime<Utc>, interval: Duration) -> Result<Self> {
        Ok(Self {
            base,
            iteration: 0,
            interval: chrono::Duration::from_std(interval)?,
        })
    }

    pub fn advance(&mut self) -> Result<Duration> {
        let now = Utc::now();
        let mut i = self.iteration;

        let delay = loop {
            i += 1;

            let value = self.base + (self.interval * i);
            if value >= now {
                break (value - now);
            } else if i == i32::MAX {
                anyhow::bail!("A maximum of {} intervals has passed", i);
            }
        };
        self.iteration = i;

        Ok(delay.to_std()?)
    }

    pub fn current(&self) -> DateTime<Utc> {
        self.base + (self.interval * self.iteration)
    }
}
