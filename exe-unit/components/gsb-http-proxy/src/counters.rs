use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use ya_counters::{counters::Metric, error::MetricError};

#[derive(Default, Clone, Debug)]
pub(super) struct Counters {
    //TODO make it a vec of boxed type
    pub(super) requests: Option<RequestCounter>,
    /*
    pub(super) requests_duration: Option<RequestsDurationCounter>,
    */
}

impl Counters {
    pub(super) fn requests_counter(&mut self) -> impl Metric {
        self.requests.get_or_insert_with(Default::default).clone()
    }

    /*
    pub(super) fn requests_duration_counter(&mut self) -> impl Metric {
        self.requests_duration.get_or_insert_with(|| Default::default()).clone()
    }
    */

    pub(super) fn on_request(&mut self) -> ResponseHandler {
        if let Some(ref mut requests_counter) = self.requests {
            requests_counter.on_request();
        }
        let requests = self.requests.clone();
        ResponseHandler {
            requests,
            ..Default::default()
        }
    }
}

#[derive(Default)]
pub(super) struct ResponseHandler {
    counted: bool,
    pub(super) requests: Option<RequestCounter>,
    /*
    pub(super) requests_duration: Option<RequestsDurationCounter>,
    */
}

impl ResponseHandler {
    fn on_response_priv(&mut self) {
        if self.counted {
            return;
        }
        self.counted = true;
        if let Some(ref mut requests_counter) = self.requests {
            requests_counter.on_response();
        }
    }

    pub(super) fn on_response(mut self) {
        self.on_response_priv()
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

//

#[derive(Debug, Default, Clone)]
struct RequestCounter {
    count: Arc<AtomicU64>,
}

impl Metric for RequestCounter {
    fn frame(&mut self) -> Result<f64, MetricError> {
        let count = self.count.load(Ordering::Relaxed) as f64;
        Ok(count)
    }

    fn peak(&mut self) -> ya_counters::counters::Result<ya_counters::counters::MetricData> {
        self.frame()
    }
}

impl RequestResponseHandler for RequestCounter {
    fn on_request(&mut self) {
        self.count.fetch_add(1, Ordering::Relaxed);
    }

    fn on_response(&mut self) {}
}

//////////////

// #[derive(Clone, Copy, Debug)]
// pub(super) struct RequestsDurationCounter {
//     duration: Duration,
//     active_requests_count: u16,
//     first_active_request_start_time: Option<DateTime<Utc>>,
// }

// #[derive(Clone, Copy, Debug)]
// pub(super) struct RequestsDurationCounter {
//     duration: Duration,
//     active_requests_count: u16,
//     first_active_request_start_time: Option<DateTime<Utc>>,
// }

// impl RequestsDurationCounter {
//     fn active_request_duration(&self, response_time: DateTime<Utc>) -> Duration {
//         if let Some(active_request_start_time) = self.first_active_request_start_time {
//             return response_time - active_request_start_time;
//         }
//         Duration::zero()
//     }
// }

// impl Counter for RequestsDurationCounter {
//     fn count(&self) -> f64 {
//         let duration_so_far = self.duration + self.active_request_duration(Utc::now());
//         super::duration_to_secs(duration_so_far)
//     }
// }

// impl RequestMonitoringCounter for RequestsDurationCounter {
//     fn on_request(&mut self, request_time: DateTime<Utc>) {
//         self.active_requests_count += 1;
//         if self.first_active_request_start_time.is_none() {
//             self.first_active_request_start_time = Some(request_time);
//         }
//     }

//     fn on_response(&mut self, response_time: DateTime<Utc>) {
//         self.active_requests_count -= 1;
//         if self.active_requests_count == 0 {
//             self.duration = self.duration + self.active_request_duration(response_time);
//             self.first_active_request_start_time = None;
//         }
//     }
// }

// impl Default for RequestsDurationCounter {
//     fn default() -> Self {
//         let duration = Duration::zero();
//         Self {
//             duration,
//             active_requests_count: 0,
//             first_active_request_start_time: None,
//         }
//     }
// }
