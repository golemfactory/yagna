//! Activity API part of the Yagna API
mod provider;
mod requestor;

pub use provider::ActivityProviderApi;
pub use requestor::control::ActivityRequestorControlApi;
pub use requestor::state::ActivityRequestorStateApi;

const ACTIVITY_URL_ENV_VAR: &str = "YAGNA_ACTIVITY_URL";
