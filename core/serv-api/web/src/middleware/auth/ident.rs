use actix_web::dev::{Extensions, Payload, ServiceRequest};
use actix_web::error::PayloadError;
use actix_web::error::{ErrorForbidden, ErrorUnauthorized};
use actix_web::web::Bytes;
use actix_web::{Error, FromRequest, HttpMessage, HttpRequest, ResponseError};
use futures::prelude::*;
use serde::Serialize;
use std::cell::Ref;
use std::convert::TryFrom;
use std::fmt::{Display, Formatter};
use std::pin::Pin;
use std::str::FromStr;
use ya_client::model::NodeId;
use ya_core_model::appkey::AppKey;

pub const AUTOCONFIGURED_ADMIN_SUBJECT: &str = "autoconfigured-admin";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Manager,
    Admin,
}

impl Display for Role {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Manager => f.write_str("manager"),
            Self::Admin => f.write_str("admin"),
        }
    }
}

impl FromStr for Role {
    type Err = InvalidRole;

    fn from_str(role: &str) -> Result<Self, Self::Err> {
        match role {
            "manager" => Ok(Self::Manager),
            "admin" => Ok(Self::Admin),
            _ => Err(InvalidRole(role.to_string())),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvalidRole(String);

impl Display for InvalidRole {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "unsupported application-key role: {}", self.0)
    }
}

impl std::error::Error for InvalidRole {}

#[derive(Clone, Debug, Serialize)]
pub struct Identity {
    pub identity: NodeId,
    pub name: String,
    pub subject: String,
    pub role: Role,
}

impl Identity {
    pub fn admin(identity: NodeId) -> Self {
        Self {
            identity,
            name: AUTOCONFIGURED_ADMIN_SUBJECT.to_string(),
            subject: AUTOCONFIGURED_ADMIN_SUBJECT.to_string(),
            role: Role::Admin,
        }
    }
}

impl TryFrom<AppKey> for Identity {
    type Error = InvalidRole;

    fn try_from(app_key: AppKey) -> Result<Self, Self::Error> {
        let role = Role::from_str(&app_key.role)?;
        if role != Role::Manager {
            return Err(InvalidRole(app_key.role));
        }

        Ok(Self {
            identity: app_key.identity,
            subject: app_key.name.clone(),
            name: app_key.name,
            role,
        })
    }
}

impl TryFrom<&AppKey> for Identity {
    type Error = InvalidRole;

    fn try_from(app_key: &AppKey) -> Result<Self, Self::Error> {
        Self::try_from(app_key.clone())
    }
}

/// Extractor for handlers which require daemon administrator privileges.
///
/// Authentication middleware inserts the complete [`Identity`] principal.
/// This extractor makes the authorization requirement explicit in the handler
/// signature and rejects ordinary manager principals before the handler runs.
#[derive(Clone, Debug)]
pub struct Admin(Identity);

impl Admin {
    pub fn principal(&self) -> &Identity {
        &self.0
    }

    pub fn into_principal(self) -> Identity {
        self.0
    }
}

impl FromRequest for Admin {
    type Error = Error;
    type Future = future::Ready<Result<Self, Self::Error>>;

    fn from_request(
        req: &HttpRequest,
        _payload: &mut Payload<Pin<Box<dyn Stream<Item = Result<Bytes, PayloadError>>>>>,
    ) -> Self::Future {
        match req.extensions().get::<Identity>() {
            Some(principal) if principal.role == Role::Admin => future::ok(Self(principal.clone())),
            Some(_) => future::err(ErrorForbidden("Administrator role required")),
            None => future::err(ErrorUnauthorized("Authentication required")),
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

impl FromRequest for Identity {
    type Error = EmptyError;
    type Future = future::Ready<Result<Self, Self::Error>>;

    fn from_request(
        req: &HttpRequest,
        _payload: &mut Payload<Pin<Box<dyn Stream<Item = Result<Bytes, PayloadError>>>>>,
    ) -> Self::Future {
        if let Some(v) = req.extensions().get::<Identity>() {
            future::ok(v.clone())
        } else {
            future::err(EmptyError {})
        }
    }
}

#[derive(Debug)]
pub struct EmptyError;

impl Display for EmptyError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "()")
    }
}

impl ResponseError for EmptyError {}

impl_try_from!(ServiceRequest);
impl_try_from!(&ServiceRequest);
impl_try_from!(HttpRequest);
impl_try_from!(&HttpRequest);

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::http::StatusCode;
    use actix_web::test::TestRequest;

    #[test]
    fn roles_serialize_as_lowercase_strings() {
        assert_eq!(
            serde_json::to_string(&Role::Manager).unwrap(),
            r#""manager""#
        );
        assert_eq!(serde_json::to_string(&Role::Admin).unwrap(), r#""admin""#);
    }

    #[test]
    fn unknown_role_is_rejected() {
        assert!(Role::from_str("unknown").is_err());
    }

    #[test]
    fn admin_principal_uses_reserved_non_secret_subject() {
        let principal = Identity::admin(NodeId::default());
        assert_eq!(principal.name, AUTOCONFIGURED_ADMIN_SUBJECT);
        assert_eq!(principal.subject, AUTOCONFIGURED_ADMIN_SUBJECT);
        assert_eq!(principal.role, Role::Admin);
    }

    #[actix_rt::test]
    async fn admin_extractor_accepts_only_admin_principal() {
        let admin_request = TestRequest::default().to_http_request();
        admin_request
            .extensions_mut()
            .insert(Identity::admin(NodeId::default()));
        let extracted = Admin::extract(&admin_request).await.unwrap();
        assert_eq!(extracted.principal().role, Role::Admin);

        let manager_request = TestRequest::default().to_http_request();
        manager_request.extensions_mut().insert(Identity {
            identity: NodeId::default(),
            name: "manager".to_string(),
            subject: "manager".to_string(),
            role: Role::Manager,
        });
        let error = Admin::extract(&manager_request).await.unwrap_err();
        assert_eq!(
            error.as_response_error().status_code(),
            StatusCode::FORBIDDEN
        );
    }
}
