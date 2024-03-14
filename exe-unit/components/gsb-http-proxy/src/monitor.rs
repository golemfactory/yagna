pub trait RequestsMonitor: Sync + Send + Clone {
    fn on_request(&mut self) -> impl RequestMonitor;
}

pub trait RequestMonitor {
    fn on_response(self);
}

#[derive(Clone, Debug)]
pub struct DisabledRequestsMonitor {}

impl RequestsMonitor for DisabledRequestsMonitor {
    fn on_request(&mut self) -> impl crate::monitor::RequestMonitor {
        DisabledRequestsMonitor {}
    }
}

pub struct DisabledMonitoredRequest {}

impl RequestMonitor for DisabledRequestsMonitor {
    fn on_response(self) {}
}

//TODO call `on_response` on drop
