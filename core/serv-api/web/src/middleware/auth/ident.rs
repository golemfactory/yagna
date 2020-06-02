use actix_web::dev::{Extensions, Payload, ServiceRequest};
use actix_web::error::PayloadError;
use actix_web::web::Bytes;
use actix_web::{FromRequest, HttpMessage, HttpRequest};
use futures::prelude::*;
use serde::Serialize;
use std::cell::Ref;
use std::convert::TryFrom;
use std::pin::Pin;
use ya_client::model::NodeId;
use ya_core_model::appkey::AppKey;

#[derive(Clone, Debug, Serialize)]
pub struct Identity {
    pub identity: NodeId,
    pub name: String,
    pub role: String,
}

impl From<AppKey> for Identity {
    fn from(app_key: AppKey) -> Self {
        Identity {
            identity: app_key.identity,
            name: app_key.name,
            role: app_key.role,
        }
    }
}

impl From<&AppKey> for Identity {
    fn from(app_key: &AppKey) -> Self {
        app_key.clone().into()
    }
}

impl TryFrom<Ref<'_, Extensions>> for Identity {
    type Error = ();

    fn try_from(ext: Ref<'_, Extensions>) -> Result<Self, Self::Error> {
        ext.get::<Identity>().cloned().ok_or(())
    }
}

macro_rules! impl_try_from {
    ($ty:ty) => {
        impl TryFrom<$ty> for Identity {
            type Error = ();

            #[inline]
            fn try_from(v: $ty) -> Result<Self, Self::Error> {
                Self::try_from(v.extensions())
            }
        }
    };
}

impl FromRequest for Identity {
    type Error = ();
    type Future = future::Ready<Result<Self, Self::Error>>;
    type Config = ();

    fn from_request(
        req: &HttpRequest,
        _payload: &mut Payload<Pin<Box<dyn Stream<Item = Result<Bytes, PayloadError>>>>>,
    ) -> Self::Future {
        if let Some(v) = req.extensions().get::<Identity>() {
            future::ok(v.clone())
        } else {
            future::err(())
        }
    }
}

impl_try_from!(ServiceRequest);
impl_try_from!(&ServiceRequest);
impl_try_from!(HttpRequest);
impl_try_from!(&HttpRequest);
