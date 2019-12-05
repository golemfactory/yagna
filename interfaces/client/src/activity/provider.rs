use futures::Future;
use std::pin::Pin;
use ya_model::activity::{ActivityState, ActivityUsage, ProviderEvent};

pub mod gsb {
    use futures::Future;
    use std::pin::Pin;
    use ya_model::activity::gsb::{Exec, GetRunningCommand};
    use ya_model::activity::{ExeScriptCommandResult, ExeScriptCommandState};

    pub trait ProviderApi {
        fn execute<'s>(
            &'s mut self,
            event: &Exec,
        ) -> Pin<Box<dyn Future<Output = Vec<ExeScriptCommandResult>> + 's>>;

        fn get_running_command<'s>(
            &'s self,
            event: &GetRunningCommand,
        ) -> Pin<Box<dyn Future<Output = ExeScriptCommandState> + 's>>;
    }
}

pub trait ProviderApi {
    fn get_activity_events<'s>(
        &'s self,
        timeout: Option<i32>,
    ) -> Pin<Box<dyn Future<Output = Vec<ProviderEvent>> + 's>>;

    fn set_activity_state<'s>(
        &'s mut self,
        activity_id: &str,
        state: Option<ActivityState>,
    ) -> Pin<Box<dyn Future<Output = ()> + 's>>;

    fn set_activity_usage<'s>(
        &'s mut self,
        activity_id: &str,
        state: Option<ActivityUsage>,
    ) -> Pin<Box<dyn Future<Output = ()> + 's>>;
}
