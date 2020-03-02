use crate::error::{DbResult, Error, ExternalServiceError};
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
    use ya_model::ErrorMessage;

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
        HttpResponse::InternalServerError().json(ErrorMessage::new(e.to_string()))
    }

    pub fn bad_request(e: &impl ToString) -> HttpResponse {
        HttpResponse::BadRequest().json(ErrorMessage::new(e.to_string()))
    }
}
