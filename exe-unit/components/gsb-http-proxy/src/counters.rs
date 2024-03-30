use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, RwLock,
};

use chrono::{DateTime, Duration, Utc};

use ya_counters::error::MetricError;
use ya_counters::Result as CountersResult;
use ya_counters::{Metric, MetricData};

#[derive(Default, Clone, Debug)]
pub(super) struct Counters {
    requests: Option<RequestCounter>,
    requests_duration: Option<SharedRequestsDurationCounter>,
}

impl Counters {
    pub(super) fn requests_counter(&mut self) -> impl Metric {
        self.requests.get_or_insert_with(Default::default).clone()
    }

    pub(super) fn requests_duration_counter(&mut self) -> impl Metric {
        self.requests_duration
            .get_or_insert_with(Default::default)
            .clone()
    }

    pub(super) fn on_request(&mut self) -> ResponseHandler {
        if let Some(counter) = &mut self.requests {
            counter.on_request();
        }
        if let Some(counter) = &mut self.requests_duration {
            counter.on_request();
            let counter = Some(counter.clone());
            return ResponseHandler {
                counter,
                ..Default::default()
            };
        }
        Default::default()
    }
}

#[derive(Default)]
pub(super) struct ResponseHandler {
    counted: bool,
    counter: Option<SharedRequestsDurationCounter>,
}

impl ResponseHandler {
    pub(super) fn on_response(mut self) {
        self.on_response_priv()
    }

    fn on_response_priv(&mut self) {
        if self.counted {
            return;
        }
        self.counted = true;
        if let Some(counter) = &mut self.counter {
            counter.on_response();
        }
    }
}

impl Drop for ResponseHandler {
    fn drop(&mut self) {
        self.on_response_priv()
    }
}

trait RequestResponseHandler {
    fn on_request(&mut self);
    fn on_response(&mut self);
}

#[derive(Debug, Default, Clone)]
struct RequestCounter {
    count: Arc<AtomicU64>,
}

impl RequestCounter {
    fn on_request(&mut self) {
        self.count.fetch_add(1, Ordering::Relaxed);
    }
}

impl Metric for RequestCounter {
    fn frame(&mut self) -> CountersResult<MetricData> {
        let count = self.count.load(Ordering::Relaxed) as f64;
        Ok(count)
    }

    fn peak(&mut self) -> CountersResult<MetricData> {
        self.frame()
    }
}

#[derive(Clone, Default, Debug)]
struct SharedRequestsDurationCounter(Arc<RwLock<RequestsDurationCounter>>);

impl SharedRequestsDurationCounter {
    fn on_request(&mut self) {
        match self.0.write() {
            Ok(mut counter) => counter.on_request(),
            Err(err) => log::error!("Requests Duration Counter on_request Error: {err}"),
        }
    }

    fn on_response(&mut self) {
        match self.0.write() {
            Ok(mut counter) => counter.on_response(),
            Err(err) => log::error!("Requests Duration Counter on_response Error: {err}"),
        }
    }
}

impl Metric for SharedRequestsDurationCounter {
    fn frame(&mut self) -> CountersResult<MetricData> {
        match self.0.read() {
            Ok(counter) => counter.count(),
            Err(err) => Err(MetricError::Other(err.to_string())),
        }
    }

    fn peak(&mut self) -> CountersResult<MetricData> {
        self.frame()
    }
}

#[derive(Debug)]
pub(super) struct RequestsDurationCounter {
    duration: Duration,
    active_requests_count: u16,
    first_active_request_start_time: Option<DateTime<Utc>>,
}

impl RequestsDurationCounter {
    fn count(&self) -> CountersResult<f64> {
        let duration_so_far = self.duration + self.active_request_duration(Utc::now());
        Ok(duration_to_secs(duration_so_far))
    }

    fn on_request(&mut self) {
        let request_time = Utc::now();
        self.active_requests_count += 1;
        if self.first_active_request_start_time.is_none() {
            self.first_active_request_start_time = Some(request_time);
        }
    }

    fn on_response(&mut self) {
        let response_time = Utc::now();
        self.active_requests_count -= 1;
        if self.active_requests_count == 0 {
            self.duration = self.duration + self.active_request_duration(response_time);
            self.first_active_request_start_time = None;
        }
    }

    fn active_request_duration(&self, response_time: DateTime<Utc>) -> Duration {
        if let Some(active_request_start_time) = self.first_active_request_start_time {
            return response_time - active_request_start_time;
        }
        Duration::zero()
    }
}

impl Default for RequestsDurationCounter {
    fn default() -> Self {
        let duration = Duration::zero();
        Self {
            duration,
            active_requests_count: 0,
            first_active_request_start_time: None,
        }
    }
}

fn duration_to_secs(duration: Duration) -> f64 {
    duration.to_std().expect("Duration is >= 0").as_secs_f64()
}
