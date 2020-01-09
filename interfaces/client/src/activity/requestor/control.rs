//! Requestor control part of Activity API
use crate::Result;
use ya_model::activity::{ExeScriptCommandResult, ExeScriptRequest};

rest_interface! {
    /// Bindings for Requestor Control part of the Activity API.
    impl RequestorControlApiClient {
        /// Creates new Activity based on given Agreement.
        pub async fn create_activity(
            &self,
            agreement_id: &str
        ) -> Result<String> {
            let response = post("activity/").send_json( &agreement_id ).body();

            { Ok( String::from_utf8( response?.to_vec() )? ) }
        }

        /// Destroys given Activity.
        pub async fn destroy_activity(
            &self,
            #[path] activity_id: &str
        ) -> Result<()> {
            let response = delete("activity/{activity_id}/").send().body();

            { response.map(|_| ()) }
        }

        /// Executes an ExeScript batch within a given Activity.
        pub async fn exec(
            &self,
            script: ExeScriptRequest,
            #[path] activity_id: &str
        ) -> Result<String> {
            let response = post("activity/{activity_id}/state/").send_json( &script ).json();

            response
        }

        /// Queries for ExeScript batch results.
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
