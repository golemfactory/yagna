use chrono;
use std::str::FromStr;

use crate::db::dao::AgreementDao;
use crate::db::model::AgreementId;
use ya_client::model::market::Agreement as ClientAgreement;
use ya_core_model::market::{GetAgreement, RpcMessageError};
use ya_persistence::executor::DbExecutor;
use ya_service_bus::typed::ServiceBinder;

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
    let agreement_id = AgreementId::from_str(&msg.agreement_id)
        .map_err(|e| RpcMessageError::Market(e.to_string()))?;
    Ok(db
        .as_dao::<AgreementDao>()
        .select(&agreement_id, chrono::Utc::now().naive_utc())
        .await
        .map_err(|e| RpcMessageError::Market(e.to_string()))?
        .ok_or(RpcMessageError::NotFound(msg.agreement_id.clone()))?
        .into_client()
        .map_err(|e| RpcMessageError::Market(e.to_string()))?)
}
