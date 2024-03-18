pub trait RequestsMonitor: Sync + Send + Clone {
    /// Called once on every HTTP request
    #[allow(async_fn_in_trait)]
    async fn on_request(&mut self) -> impl ResponseMonitor;
}

pub trait ResponseMonitor: Sync + Send {
    /// Called once on HTTP response (for any response code) or on Drop.
    #[allow(async_fn_in_trait)]
    async fn on_response(self);
}
