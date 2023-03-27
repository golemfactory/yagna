//! Version handling service bus API.

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

use ya_client_model::ErrorMessage;
use ya_service_bus::RpcMessage;

pub const BUS_ID: &str = "/local/version";

/// Skip upgrading to the latest Yagna release.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Skip();

impl RpcMessage for Skip {
    const ID: &'static str = "skip";
    type Item = Option<Release>;
    type Error = ErrorMessage;
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Get {
    pub check: bool,
}

impl Get {
    pub fn show_only() -> Self {
        Get { check: false }
    }

    pub fn with_check() -> Self {
        Get { check: true }
    }
}

impl RpcMessage for Get {
    const ID: &'static str = "check";
    type Item = VersionInfo;
    type Error = ErrorMessage;
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "camelCase")]
#[error("Version {version} '{name}' released {}", release_ts.format("%Y-%m-%d"))]
pub struct Release {
    pub version: String,
    pub name: String,
    pub seen: bool,
    pub release_ts: NaiveDateTime,
    pub insertion_ts: Option<NaiveDateTime>,
    pub update_ts: Option<NaiveDateTime>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionInfo {
    pub current: Release,
    pub pending: Option<Release>,
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_release_to_string() {
        let now = NaiveDateTime::parse_from_str("2015-10-13T15:43:00GMT+2", "%Y-%m-%dT%H:%M:%S%Z")
            .unwrap();
        let r = Release {
            version: "0.6.1".to_string(),
            name: "some code name".to_string(),
            seen: false,
            release_ts: now,
            insertion_ts: None,
            update_ts: None,
        };

        assert_eq!(
            r.to_string(),
            "Version 0.6.1 'some code name' released 2015-10-13"
        );
    }
}
