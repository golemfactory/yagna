pub mod appkey;
pub mod identity;

pub use appkey::AppKeyDao;
pub use identity::IdentityDao;

use r2d2;
use std::fmt::Display;
use thiserror::Error;

use ya_core_model::appkey as model;
use ya_persistence::executor::DbExecutor;

#[derive(Error, Debug)]
pub enum Error {
    #[error("DB connection error: {0}")]
    Db(#[from] r2d2::Error),
    #[error("DAO error: {0}")]
    Dao(#[from] diesel::result::Error),
    #[error("GSB error: {0}")]
    Gsb(ya_service_bus::error::Error),
    #[error("task: {0}")]
    RuntimeError(#[from] tokio::task::JoinError),
    #[error("{0}")]
    Internal(String),
    #[error("Already exists")]
    AlreadyExists,
    #[error("Not found")]
    NotFound,
    #[error("Forbidden")]
    Forbidden,
}

impl Error {
    pub fn internal(e: impl Display) -> Self {
        Self::Internal(e.to_string())
    }
}

impl From<ya_service_bus::error::Error> for Error {
    fn from(e: ya_service_bus::error::Error) -> Self {
        Error::Gsb(e)
    }
}

macro_rules! into_error {
    ($self:ident, $code:expr) => {
        model::Error {
            code: $code,
            message: format!("{:?}", $self),
        }
    };
}

impl Into<model::Error> for Error {
    fn into(self) -> model::Error {
        match self {
            Error::Db(_) => into_error!(self, 500),
            Error::Dao(_) => into_error!(self, 500),
            Error::Gsb(_) => into_error!(self, 500),
            Error::RuntimeError(_) => into_error!(self, 500),
            Error::Internal(_) => into_error!(self, 500),
            Error::AlreadyExists => into_error!(self, 400),
            Error::NotFound => into_error!(self, 404),
            Error::Forbidden => into_error!(self, 403),
        }
    }
}

pub async fn init(db: &DbExecutor) -> anyhow::Result<()> {
    db.apply_migration(crate::db::migrations::run_with_output)
}
