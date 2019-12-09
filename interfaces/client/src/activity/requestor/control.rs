use async_trait::async_trait;
use failure::Fallible;
use ya_model::activity::{ExeScriptCommandResult, ExeScriptRequest};

#[async_trait(?Send)]
pub trait RequestorControlApi {
    async fn create_activity(&self, agreement_id: &str) -> Fallible<String>;
    async fn destroy_activity(&self, activity_id: &str) -> Fallible<()>;
    async fn exec(&self, activity_id: &str, script: ExeScriptRequest) -> Fallible<String>;
    async fn get_exec_batch_results(
        &self,
        activity_id: &str,
        batch_id: &str,
        timeout: Option<i32>,
        max_count: Option<i32>,
    ) -> Fallible<Vec<ExeScriptCommandResult>>;
}
