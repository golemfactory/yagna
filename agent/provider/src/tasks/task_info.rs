use anyhow::Result;
use chrono::{DateTime, TimeZone, Utc};
use std::time::Duration;

use ya_agreement_utils::{AgreementView, Error};

/// Stores Task properties negotiated in Agreement.
#[derive(Clone)]
pub struct TaskInfo {
    pub agreement_id: String,
    /// Provider is allowed to kill ExeUnits and Terminate Agreement
    /// without consequences (financial or lost of reputation).
    pub expiration: DateTime<Utc>,
    /// Requestor can create multiple Activities within single Agreement.
    /// Is this flag is set to true, Requestor is responsible for calling
    /// Terminate Agreement. If flag is set to false, Agreement is closed after
    /// first Destroyed Activity. In this case Provider will terminate Agreement.
    ///
    /// Note: This is behavior for compatibility with previous versions, where Terminate
    /// Agreement wasn't implemented. It is recommended, that Requestor should always call
    /// Terminate Agreement, if he finished computations.
    pub multi_activity: bool,
    /// Max allowed time Agreement can have no Activities.
    /// TODO: This could be negotiated between Provider and Requestor.
    pub idle_agreement_timeout: Duration,
}

fn agreement_expiration_from(agreement: &AgreementView) -> Result<DateTime<Utc>> {
    let expiration_key_str = "/demand/properties/golem/srv/comp/expiration";
    let timestamp = agreement.pointer_typed::<i64>(expiration_key_str)?;
    Ok(Utc.timestamp_millis(timestamp))
}

fn multi_activity_from(agreement: &AgreementView) -> Result<bool> {
    let multi_activity_key_str = "/demand/properties/golem/srv/caps/multi-activity";
    match agreement.pointer_typed::<bool>(multi_activity_key_str) {
        Err(e) => match e {
            // For backward compatibility 'multi-activity' field doesn't have to be set
            // and default value: false is used.
            Error::NoKey(_) => Ok(false),
            _ => Err(anyhow::Error::from(e)),
        },
        Ok(multi_activity) => Ok(multi_activity),
    }
}

impl TaskInfo {
    pub fn from(agreement: &AgreementView) -> Result<TaskInfo> {
        Ok(TaskInfo {
            agreement_id: agreement.agreement_id.clone(),
            expiration: agreement_expiration_from(agreement)?,
            multi_activity: multi_activity_from(agreement)?,
            idle_agreement_timeout: Duration::from_secs(90),
        })
    }

    pub fn with_idle_agreement_timeout(mut self, timeout: Duration) -> Self {
        self.idle_agreement_timeout = timeout;
        self
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::convert::TryFrom;
    use ya_agreement_utils::agreement::expand;

    pub static SAMPLE_AGREEMENT_MULTI_TRUE: &str = r#"{
    "demand.properties":
    {
      "golem.srv.comp.expiration": 1590765503361,
      "golem.srv.caps.multi-activity": true
    },
    "offer.properties": {},
    "agreementId": "fb30737abc959a5d464245fed9ecc6c4568190c9daa0221692035f823030fb81"
}"#;

    #[test]
    fn test_task_info_from_agreement_multi_activity_true() {
        let view = AgreementView::try_from(expand(
            serde_json::from_str::<serde_json::Value>(SAMPLE_AGREEMENT_MULTI_TRUE).unwrap(),
        ))
        .unwrap();
        let info = TaskInfo::from(&view).unwrap();

        assert!(info.multi_activity);
    }

    pub static SAMPLE_AGREEMENT_MULTI_FALSE: &str = r#"{
    "demand.properties":
    {
      "golem.srv.comp.expiration": 1590765503361,
      "golem.srv.caps.multi-activity": false
    },
    "offer.properties": {},
    "agreementId": "fb30737abc959a5d464245fed9ecc6c4568190c9daa0221692035f823030fb81"
}"#;

    #[test]
    fn test_task_info_from_agreement_multi_activity_false() {
        let view = AgreementView::try_from(expand(
            serde_json::from_str::<serde_json::Value>(SAMPLE_AGREEMENT_MULTI_FALSE).unwrap(),
        ))
        .unwrap();
        let info = TaskInfo::from(&view).unwrap();

        assert!(!info.multi_activity);
    }

    pub static SAMPLE_AGREEMENT_MULTI_EMPTY: &str = r#"{
    "demand.properties":
    {
      "golem.srv.comp.expiration": 1590765503361
    },
    "offer.properties": {},
    "agreementId": "fb30737abc959a5d464245fed9ecc6c4568190c9daa0221692035f823030fb81"
}"#;

    #[test]
    fn test_task_info_from_agreement_multi_activity_no_entry() {
        let view = AgreementView::try_from(expand(
            serde_json::from_str::<serde_json::Value>(SAMPLE_AGREEMENT_MULTI_EMPTY).unwrap(),
        ))
        .unwrap();
        let info = TaskInfo::from(&view).unwrap();

        assert!(!info.multi_activity);
    }
}
