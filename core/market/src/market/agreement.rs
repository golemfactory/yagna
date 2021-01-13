use chrono;

use ya_client::model::market::Agreement as ClientAgreement;
use ya_core_model::{
    market::{GetAgreement, RpcMessageError},
    Role,
};
use ya_persistence::executor::DbExecutor;
use ya_service_bus::typed::ServiceBinder;

use crate::db::dao::AgreementDao;
use crate::db::model::{AgreementId, Owner};
use ya_client::model::ParseError;

pub async fn bind_gsb(db: DbExecutor, public_prefix: &str, _local_prefix: &str) {
    log::trace!("Binding market agreement public service to service bus");
    ServiceBinder::new(public_prefix, &db, ()).bind(get_agreement);
    log::debug!("Successfully bound market agreement public service to service bus");
}

async fn get_agreement(
    db: DbExecutor,
    caller: String,
    msg: GetAgreement,
) -> Result<ClientAgreement, RpcMessageError> {
    let owner = match msg.role {
        Role::Provider => Owner::Provider,
        Role::Requestor => Owner::Requestor,
    };

    let agreement_id = AgreementId::from_client(&msg.agreement_id, owner)
        .map_err(|e| RpcMessageError::Market(e.to_string()))?;

    let caller_id = caller
        .parse()
        .map_err(|e: ParseError| RpcMessageError::BadRequest(e.to_string()))?;
    let now = chrono::Utc::now().naive_utc();
    let agreement = db
        .as_dao::<AgreementDao>()
        .select(&agreement_id, Some(caller_id), now)
        .await
        .map_err(|e| RpcMessageError::Market(e.to_string()))?
        .ok_or(RpcMessageError::NotFound(msg.agreement_id.clone()))?
        .into_client()
        .map_err(|e| RpcMessageError::Market(e.to_string()))?;
    Ok(agreement)
}
