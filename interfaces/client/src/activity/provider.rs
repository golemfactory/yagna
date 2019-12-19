//! Provider part of Activity API
use crate::Result;
use ya_model::activity::{ActivityState, ActivityUsage, ProviderEvent};

pub mod gsb {
    use crate::Result;
    use ya_model::activity::{
        ActivityState, ActivityUsage, ExeScriptBatch, ExeScriptCommandResult, ExeScriptCommandState,
    };

    pub struct GsbProviderApi;

    impl GsbProviderApi {
        pub async fn exec(
            &self,
            _activity_id: &str,
            _batch_id: &str,
            _exe_script: ExeScriptBatch,
        ) -> Result<Vec<ExeScriptCommandResult>> {
            unimplemented!()
        }

        pub async fn get_running_command(
            &self,
            _activity_id: &str,
        ) -> Result<ExeScriptCommandState> {
            unimplemented!()
        }

        pub async fn get_state(&self, _activity_id: &str) -> Result<ActivityState> {
            unimplemented!()
        }

        pub async fn get_usage(&self, _activity_id: &str) -> Result<ActivityUsage> {
            unimplemented!()
        }
    }
}

rest_interface! {
    /// Bindings for Provider part of the Activity API.
    impl ProviderApiClient {

        pub async fn get_activity_events(
            &self,
            #[query] timeout: Option<i32>
        ) -> Result<Vec<ProviderEvent>> {
            let response = get("activity/events/").send().json();

            response
        }

        pub async fn set_activity_state(
            &self,
            state: ActivityState,
            #[path] activity_id: &str
        ) -> Result<()> {
            let response = put("activity/{activity_id}/state/").send_json( &state ).body();

            { response.map(|_| ()) }
        }

        pub async fn set_activity_usage(
            &self,
            usage: ActivityUsage,
            #[path] activity_id: &str
        ) -> Result<()> {
            let response = put("activity/{activity_id}/usage/").send_json( &usage ).body();

            { response.map(|_| ()) }
        }
    }
}
