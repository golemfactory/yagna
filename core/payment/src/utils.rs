use crate::error::{DbResult, Error, ExternalServiceError};
use actix_web::HttpResponse;
use futures::{Future, FutureExt};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use ya_client_model::market::Agreement;
use ya_core_model::ethaddr::NodeId;
use ya_core_model::{identity, market};
use ya_service_bus::{typed as bus, RpcEndpoint};

pub fn fake_get_agreement(agreement_id: String, agreement: Agreement) {
    bus::bind(market::BUS_ID, move |msg: market::GetAgreement| {
        let agreement_id = agreement_id.clone();
        let agreement = agreement.clone();
        async move {
            if msg.agreement_id == agreement_id {
                Ok(agreement)
            } else {
                Err(market::RpcMessageError::NotFound(format!(
                    "have only agreement id: {}",
                    agreement_id
                )))
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
        Err(Error::ExtService(ExternalServiceError::Market(
            market::RpcMessageError::NotFound(_),
        ))) => Ok(None),
        Err(e) => Err(e),
    }
}

pub mod provider {
    use crate::error::{Error, ExternalServiceError};
    use ya_client_model::market::Agreement;
    use ya_core_model::{activity, market};
    use ya_service_bus::{typed as bus, RpcEndpoint};

    pub fn fake_get_agreement_id(agreement_id: String) {
        bus::bind(
            activity::local::BUS_ID,
            move |msg: activity::local::GetAgreementId| {
                let agreement_id = agreement_id.clone();
                async move { Ok(agreement_id) }
            },
        );
    }

    pub async fn get_agreement_id(activity_id: String) -> Result<Option<String>, Error> {
        match async move {
            let agreement_id = bus::service(activity::local::BUS_ID)
                .send(activity::local::GetAgreementId {
                    activity_id,
                    timeout: None,
                })
                .await??;
            Ok(agreement_id)
        }
        .await
        {
            Ok(agreement_id) => Ok(Some(agreement_id)),
            Err(Error::ExtService(ExternalServiceError::Activity(
                activity::RpcMessageError::NotFound(_),
            ))) => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub async fn get_agreement_for_activity(
        activity_id: String,
    ) -> Result<Option<Agreement>, Error> {
        match async move {
            let agreement_id = bus::service(activity::local::BUS_ID)
                .send(activity::local::GetAgreementId {
                    activity_id,
                    timeout: None,
                })
                .await??;
            let agreement = bus::service(market::BUS_ID)
                .send(market::GetAgreement::with_id(agreement_id.clone()))
                .await??;
            Ok(agreement)
        }
        .await
        {
            Ok(agreement_id) => Ok(Some(agreement_id)),
            Err(Error::ExtService(ExternalServiceError::Activity(
                activity::RpcMessageError::NotFound(_),
            ))) => Ok(None),
            Err(Error::ExtService(ExternalServiceError::Market(
                market::RpcMessageError::NotFound(_),
            ))) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

pub fn fake_sign_tx(sign_tx: Box<dyn Fn(Vec<u8>) -> Pin<Box<dyn Future<Output = Vec<u8>>>>>) {
    let sign_tx: Arc<dyn Fn(Vec<u8>) -> Pin<Box<dyn Future<Output = Vec<u8>>>>> = sign_tx.into();
    bus::bind(identity::BUS_ID, move |msg: identity::Sign| {
        let sign_tx = sign_tx.clone();
        let msg = msg.payload;
        async move { Ok(sign_tx(msg).await) }
    });
}

pub fn get_sign_tx(node_id: NodeId) -> impl Fn(Vec<u8>) -> Pin<Box<dyn Future<Output = Vec<u8>>>> {
    move |payload| {
        let fut = bus::service(identity::BUS_ID)
            .send(identity::Sign { node_id, payload })
            .map(|x| x.unwrap().unwrap());
        Box::pin(fut)
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

pub trait EventGetter {
    type Event;
    type EventFuture: Future<Output = DbResult<Vec<Self::Event>>>;
    fn get_events(&self) -> Self::EventFuture;
}

impl<T, E, F> EventGetter for T
where
    T: Fn() -> F,
    F: Future<Output = DbResult<Vec<E>>>,
{
    type Event = E;
    type EventFuture = F;

    fn get_events(&self) -> Self::EventFuture {
        self()
    }
}

pub async fn listen_for_events<T: EventGetter>(
    getter: T,
    timeout_secs: impl Into<u64>,
) -> DbResult<Vec<T::Event>> {
    let timeout_secs: u64 = timeout_secs.into();
    match getter.get_events().await {
        Err(e) => return Err(e),
        Ok(events) if events.len() > 0 || timeout_secs == 0 => return Ok(events),
        _ => (),
    }

    let timeout = Duration::from_secs(timeout_secs);
    tokio::time::timeout(timeout, async move {
        loop {
            tokio::time::delay_for(Duration::from_secs(1)).await;
            let events = getter.get_events().await?;
            if events.len() > 0 {
                break Ok(events);
            }
        }
    })
    .await
    .unwrap_or(Ok(vec![]))
}

pub mod response {
    use actix_web::HttpResponse;
    use serde::Serialize;
    use ya_client_model::ErrorMessage;

    pub fn ok<T: Serialize>(t: T) -> HttpResponse {
        HttpResponse::Ok().json(t)
    }

    pub fn created<T: Serialize>(t: T) -> HttpResponse {
        HttpResponse::Created().json(t)
    }

    pub fn not_implemented() -> HttpResponse {
        HttpResponse::NotImplemented().json(ErrorMessage { message: None })
    }

    pub fn not_found() -> HttpResponse {
        HttpResponse::NotFound().json(ErrorMessage { message: None })
    }

    pub fn unauthorized() -> HttpResponse {
        HttpResponse::Unauthorized().json(ErrorMessage { message: None })
    }

    pub fn timeout() -> HttpResponse {
        HttpResponse::GatewayTimeout().json(ErrorMessage { message: None })
    }

    pub fn server_error(e: &impl ToString) -> HttpResponse {
        let e = e.to_string();
        log::error!("Payment API server error: {}", e);
        HttpResponse::InternalServerError().json(ErrorMessage::new(e))
    }

    pub fn bad_request(e: &impl ToString) -> HttpResponse {
        HttpResponse::BadRequest().json(ErrorMessage::new(e.to_string()))
    }
}
