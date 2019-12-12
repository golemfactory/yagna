//! Activity API part of the Yagna API
pub mod provider;
pub mod requestor;

pub use provider::ProviderApiClient;
pub use requestor::control::RequestorControlApiClient;
pub use requestor::state::RequestorStateApiClient;

pub const API_ROOT: &str = "/activity-api/v1/";
