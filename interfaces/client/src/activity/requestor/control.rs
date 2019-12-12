//! Requestor control part of Activity API
use crate::Result;
use ya_model::activity::{ExeScriptCommandResult, ExeScriptRequest};

rest_interface! {
    /// Bindings for Requestor Control part of the Activity API.
    impl RequestorControlApiClient {
        pub async fn create_activity(
            &self,
            agreement_id: &str
        ) -> Result<String> {
            let response = post("activity/").send_json( &agreement_id ).body();

            { Ok( String::from_utf8( response?.to_vec() )? ) }
        }

        pub async fn destroy_activity(
            &self,
            #[path] activity_id: &str
        ) -> Result<()> {
            let _response = delete("activity/{activity_id}/").send().body();

            { Ok(()) }
        }

        pub async fn exec(
            &self,
            script: ExeScriptRequest,
            #[path] activity_id: &str
        ) -> Result<String> {
            let response = post("activity/{activity_id}/state/").send_json( &script ).json();

            response
        }

        pub async fn get_exec_batch_results(
            &self,
            #[path] activity_id: &str,
            #[path] batch_id: &str,
            #[query] timeout: Option<i32>,
            #[query] max_count: Option<i32>
        ) -> Result<Vec<ExeScriptCommandResult>> {
            let response = get("activity/{activity_id}/exec/{batch_id}/").send().json();

            response
        }
    }
}
