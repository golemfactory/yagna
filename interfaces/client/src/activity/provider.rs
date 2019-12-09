use async_trait::async_trait;
use failure::Fallible;
use ya_model::activity::{ActivityState, ActivityUsage, ProviderEvent};

pub mod gsb {
    use async_trait::async_trait;
    use failure::Fallible;
    use ya_model::activity::{
        ActivityState, ActivityUsage, ExeScriptBatch, ExeScriptCommandResult, ExeScriptCommandState,
    };

    #[async_trait(?Send)]
    pub trait GsbProviderApi {
        async fn exec(
            &self,
            activity_id: &str,
            batch_id: &str,
            exe_script: ExeScriptBatch,
        ) -> Fallible<Vec<ExeScriptCommandResult>>;
        async fn get_running_command(&self, activity_id: &str) -> Fallible<ExeScriptCommandState>;
        async fn get_state(&self, activity_id: &str) -> Fallible<ActivityState>;
        async fn get_usage(&self, activity_id: &str) -> Fallible<ActivityUsage>;
    }
}

#[async_trait(?Send)]
pub trait ProviderApi {
    async fn get_activity_events(&self, timeout: Option<i32>) -> Fallible<Vec<ProviderEvent>>;
    async fn set_activity_state(&self, activity_id: &str, state: ActivityState) -> Fallible<()>;
    async fn set_activity_usage(&self, activity_id: &str, usage: ActivityUsage) -> Fallible<()>;
}
