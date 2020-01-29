//! Activity API part of the Yagna API
pub mod provider;
pub mod requestor;

pub use provider::ProviderApiClient;
pub use requestor::control::RequestorControlApiClient;
pub use requestor::state::RequestorStateApiClient;

use crate::web::{WebClient, WebInterface};
use std::rc::Rc;
use url::Url;
pub use ya_service_api::constants::ACTIVITY_API;

impl WebInterface for RequestorControlApiClient {
    fn rebase_service_url(base_url: Rc<Url>) -> Rc<Url> {
        base_url.join("activity-api/v1/").unwrap().into()
    }

    fn from_client(client: WebClient) -> Self {
        RequestorControlApiClient::new(client)
    }
}
