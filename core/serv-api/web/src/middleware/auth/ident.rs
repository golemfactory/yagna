use actix_web::dev::{Extensions, ServiceRequest};
use actix_web::{HttpMessage, HttpRequest};
use std::cell::Ref;
use std::convert::TryFrom;
use ya_core_model::appkey::AppKey;

#[derive(Clone, Debug)]
pub struct Identity {
    key: String,
    role: String,
}

impl From<AppKey> for Identity {
    fn from(app_key: AppKey) -> Self {
        Identity {
            key: app_key.key,
            role: app_key.role,
        }
    }
}

impl From<&AppKey> for Identity {
    fn from(app_key: &AppKey) -> Self {
        Identity {
            key: app_key.key.clone(),
            role: app_key.role.clone(),
        }
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

impl_try_from!(ServiceRequest);
impl_try_from!(HttpRequest);
impl_try_from!(&HttpRequest);
impl_try_from!(&ServiceRequest);
