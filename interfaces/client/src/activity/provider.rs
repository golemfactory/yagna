use futures::Future;
use std::pin::Pin;
use ya_model::activity::{ActivityState, ActivityUsage, ProviderEvent};

pub mod gsb {
    use futures::Future;
    use std::pin::Pin;
    use ya_model::activity::{
        ActivityState, ActivityUsage, ExeScriptBatch, ExeScriptCommandResult, ExeScriptCommandState,
    };

    pub trait GsbProviderApi {
        fn exec<'s>(
            &'s self,
            activity_id: &str,
            batch_id: &str,
            exe_script: ExeScriptBatch,
        ) -> Pin<Box<dyn Future<Output = Vec<ExeScriptCommandResult>> + 's>>;

        fn get_running_command<'s>(
            &'s self,
            activity_id: &str,
        ) -> Pin<Box<dyn Future<Output = ExeScriptCommandState> + 's>>;

        fn get_state<'s>(
            &'s self,
            activity_id: &str,
        ) -> Pin<Box<dyn Future<Output = ActivityState> + 's>>;

        fn get_usage<'s>(
            &'s self,
            activity_id: &str,
        ) -> Pin<Box<dyn Future<Output = ActivityUsage> + 's>>;
    }
}

pub trait ProviderApi {
    fn get_activity_events<'s>(
        &'s self,
        timeout: Option<i32>,
    ) -> Pin<Box<dyn Future<Output = Vec<ProviderEvent>> + 's>>;

    fn set_activity_state<'s>(
        &'s self,
        activity_id: &str,
        state: Option<ActivityState>,
    ) -> Pin<Box<dyn Future<Output = ()> + 's>>;

    fn set_activity_usage<'s>(
        &'s self,
        activity_id: &str,
        state: Option<ActivityUsage>,
    ) -> Pin<Box<dyn Future<Output = ()> + 's>>;
}
