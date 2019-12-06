use futures::Future;
use std::pin::Pin;
use ya_model::activity::{ExeScriptCommandResult, ExeScriptRequest};

pub trait RequestorControlApi {
    fn create_activity<'s>(
        &'s self,
        agreement_id: &str,
    ) -> Pin<Box<dyn Future<Output = String> + 's>>;

    fn destroy_activity<'s>(&'s self, activity_id: &str) -> Pin<Box<dyn Future<Output = ()> + 's>>;

    fn exec<'s>(
        &'s self,
        activity_id: &str,
        script: ExeScriptRequest,
    ) -> Pin<Box<dyn Future<Output = String> + 's>>;

    fn get_exec_batch_results<'s>(
        &'s self,
        activity_id: &str,
        batch_id: &str,
        timeout: Option<i32>,
        max_count: Option<i32>,
    ) -> Pin<Box<dyn Future<Output = Vec<ExeScriptCommandResult>> + 's>>;
}
