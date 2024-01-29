use serde::{Deserialize, Serialize};
use ya_service_bus::Error;

#[derive(thiserror::Error, Clone, Debug, Serialize, Deserialize)]
pub enum HttpProxyStatusError {
    #[error("{0}")]
    RuntimeException(String),
}

impl From<ya_service_bus::error::Error> for HttpProxyStatusError {
    fn from(e: Error) -> Self {
        let msg = e.to_string();
        HttpProxyStatusError::RuntimeException(msg)
    }
}
