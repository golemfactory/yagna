use crate::error::{Error, ExternalServiceError};
use actix_web::HttpResponse;
use futures::Future;
use std::time::Duration;
use ya_core_model::market;
use ya_model::market::Agreement;
use ya_service_bus::{typed as bus, RpcEndpoint};

pub fn fake_get_agreement(agreement_id: String, agreement: Agreement) {
    bus::bind(market::BUS_ID, move |msg: market::GetAgreement| {
        let agreement_id = agreement_id.clone();
        let agreement = agreement.clone();
        async move {
            if msg.agreement_id == agreement_id {
                Ok(agreement)
            } else {
                Err(market::RpcMessageError::NotFound)
            }
        }
    });
}

pub async fn get_agreement(agreement_id: String) -> Result<Option<Agreement>, Error> {
    match async move {
        let agreement = bus::service(market::BUS_ID)
            .send(market::GetAgreement::with_id(agreement_id.clone()))
            .await??;
        Ok(agreement)
    }
    .await
    {
        Ok(agreement) => Ok(Some(agreement)),
        Err(Error::ExtService(ExternalServiceError::Market(market::RpcMessageError::NotFound))) => {
            Ok(None)
        }
        Err(e) => Err(e),
    }
}

pub async fn with_timeout<Work: Future<Output = HttpResponse>>(
    timeout_secs: impl Into<u64>,
    work: Work,
) -> HttpResponse {
    let timeout_secs = timeout_secs.into();
    if timeout_secs > 0 {
        match tokio::time::timeout(Duration::from_secs(timeout_secs), work).await {
            Ok(v) => v,
            Err(_) => return HttpResponse::GatewayTimeout().finish(),
        }
    } else {
        work.await
    }
}
