use async_trait::async_trait;
use failure::Fallible;
use ya_model::activity::{ActivityState, ExeScriptCommandState};

#[async_trait(?Send)]
pub trait RequestorStateApi {
    async fn get_running_command(&self, activity_id: &str) -> Fallible<ExeScriptCommandState>;
    async fn get_state(&self, activity_id: &str) -> Fallible<ActivityState>;
    async fn get_usage(&self, activity_id: &str) -> Fallible<Vec<f64>>;
}
