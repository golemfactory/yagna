use chrono;
use std::sync::Arc;

use ya_client::model::market::Agreement as ClientAgreement;
use ya_core_model::{
    market::{GetAgreement, RpcMessageError},
    Role,
};
use ya_persistence::executor::DbExecutor;
use ya_service_bus::typed::ServiceBinder;

use crate::db::dao::AgreementDao;
use crate::db::model::{AgreementId, Owner};
use crate::MarketService;

pub async fn bind_gsb(
    market: Arc<MarketService>,
    db: DbExecutor,
    public_prefix: &str,
    _local_prefix: &str,
) {
    log::trace!("Binding market agreement public service to service bus");
    ServiceBinder::new(public_prefix, &db, market).bind_with_processor(get_agreement);
    log::debug!("Successfully bound market agreement public service to service bus");
}

async fn get_agreement(
    db: DbExecutor,
    market: Arc<MarketService>,
    _sender_id: String,
    msg: GetAgreement,
) -> Result<ClientAgreement, RpcMessageError> {
    let owner = match msg.role {
        Role::Provider => Owner::Provider,
        Role::Requestor => Owner::Requestor,
    };

    let agreement_id = AgreementId::from_client(&msg.agreement_id, owner)
        .map_err(|e| RpcMessageError::Market(e.to_string()))?;

    {
        // If Agreement state is changing at this moment, we will wait until this change is finished.
        match owner {
            Owner::Provider => market.provider_engine.common.agreement_lock.clone(),
            Owner::Requestor => market.requestor_engine.common.agreement_lock.clone(),
        }
        .get_lock(&agreement_id)
        .await
        .lock()
        .await;

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
}
