use chrono;

use ya_client::model::market::Agreement as ClientAgreement;
use ya_core_model::{
    market::{GetAgreement, RpcMessageError},
    Role,
};
use ya_service_bus::typed::ServiceBinder;

use crate::db::dao::AgreementDao;
use crate::db::model::{AgreementId, Owner};
use crate::db::DbMixedExecutor;

pub async fn bind_gsb(db: DbMixedExecutor, public_prefix: &str, _local_prefix: &str) {
    log::trace!("Binding market agreement public service to service bus");
    ServiceBinder::new(public_prefix, &db, ()).bind(get_agreement);
    log::debug!("Successfully bound market agreement public service to service bus");
}

async fn get_agreement(
    db: DbMixedExecutor,
    _sender_id: String,
    msg: GetAgreement,
) -> Result<ClientAgreement, RpcMessageError> {
    let owner = match msg.role {
        Role::Provider => Owner::Provider,
        Role::Requestor => Owner::Requestor,
    };

    let agreement_id = AgreementId::from_client(&msg.agreement_id, owner)
        .map_err(|e| RpcMessageError::Market(e.to_string()))?;

    // TODO: We should check Agreement owner, like in REST get_agreement implementation, but
    //  I'm not sure we can trust `sender_id` value from gsb now.
    let dao = db.as_dao::<AgreementDao>();
    let now = chrono::Utc::now().naive_utc();
    Ok(dao
        .select(&agreement_id, None, now)
        .await
        .map_err(|e| RpcMessageError::Market(e.to_string()))?
        .ok_or(RpcMessageError::NotFound(msg.agreement_id.clone()))?
        .into_client()
        .map_err(|e| RpcMessageError::Market(e.to_string()))?)
}
