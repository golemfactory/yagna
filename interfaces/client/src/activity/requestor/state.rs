use futures::Future;
use std::pin::Pin;
use ya_model::activity::{ActivityState, ExeScriptCommandState};

pub trait RequestorStateApi {
    fn get_usage<'s>(&'s self, activity_id: &str) -> Pin<Box<dyn Future<Output = Vec<f64>> + 's>>;

    fn get_state<'s>(
        &'s self,
        activity_id: &str,
    ) -> Pin<Box<dyn Future<Output = ActivityState> + 's>>;

    fn get_running_command<'s>(
        &'s self,
        activity_id: &str,
    ) -> Pin<Box<dyn Future<Output = ExeScriptCommandState> + 's>>;
}
