//! Activity API part of the Yagna API
use ya_model::activity::{ACTIVITY_API, YAGNA_ACTIVITY_URL_ENV_VAR};

mod provider;
mod requestor;

pub use provider::ActivityProviderApi;
pub use requestor::control::ActivityRequestorControlApi;
pub use requestor::state::ActivityRequestorStateApi;
