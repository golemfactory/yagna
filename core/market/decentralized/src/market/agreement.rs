use chrono;

use ya_client::model::market::Agreement as ClientAgreement;
use ya_core_model::market::{GetAgreement, RpcMessageError};
use ya_persistence::executor::DbExecutor;
use ya_service_bus::typed::ServiceBinder;

use crate::db::dao::AgreementDao;
use crate::db::model::{AgreementId, OwnerType};

pub async fn bind_gsb(db: DbExecutor, public_prefix: &str, _local_prefix: &str) {
    log::debug!("Binding market agreement public service to service bus");
    ServiceBinder::new(public_prefix, &db, ()).bind(get_agreement);
    log::debug!("Successfully bound market agreement public service to service bus");
}

async fn get_agreement(
    db: DbExecutor,
    _sender_id: String,
    msg: GetAgreement,
) -> Result<ClientAgreement, RpcMessageError> {
    // On GSB we don't know if Provider or Requestor is calling, so we will try both versions.
    let agreement_id = AgreementId::from_client(&msg.agreement_id, OwnerType::Provider)
        .map_err(|e| RpcMessageError::Market(e.to_string()))?;

    // TODO: We should check Agreement owner, like in REST get_agreement implementation, but
    //  I'm not sure we can trust `sender_id` value from gsb now.
    let dao = db.as_dao::<AgreementDao>();
    let now = chrono::Utc::now().naive_utc();
    Ok(match dao
        .select(&agreement_id, None, now)
        .await
        .map_err(|e| RpcMessageError::Market(e.to_string()))?
    {
        None => dao
            .select(&agreement_id.swap_owner(), None, now)
            .await
            .map_err(|e| RpcMessageError::Market(e.to_string()))?,
        Some(agreement) => Some(agreement),
    }
    .ok_or(RpcMessageError::NotFound(msg.agreement_id.clone()))?
    .into_client()
    .map_err(|e| RpcMessageError::Market(e.to_string()))?)
}
