#[derive(thiserror::Error, Debug)]
pub enum DbError {
    #[error("Database connection error: {0}")]
    Connection(#[from] r2d2::Error),
    #[error("Database query error: {0}")]
    Query(#[from] diesel::result::Error),
    #[error("Runtime error: {0}")]
    Runtime(#[from] tokio::task::JoinError),
}

pub type DbResult<T> = Result<T, DbError>;

#[derive(thiserror::Error, Debug)]
pub enum ExternalServiceError {
    #[error("Market service error: {0}")]
    Market(#[from] ya_core_model::market::RpcMessageError),
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Database error: {0}")]
    Database(#[from] DbError),
    #[error("Service bus error: {0}")]
    ServiceBus(#[from] ya_service_bus::Error),
    #[error("External service error: {0}")]
    ExtService(#[from] ExternalServiceError),
    #[error("RPC error: {0}")]
    Rpc(#[from] ya_core_model::payment::RpcMessageError),
    #[error("Timeout")]
    Timeout(#[from] tokio::time::Elapsed),
}

impl From<ya_core_model::market::RpcMessageError> for Error {
    fn from(e: ya_core_model::market::RpcMessageError) -> Self {
        Into::<ExternalServiceError>::into(e).into()
    }
}

impl From<ya_core_model::payment::SendError> for Error {
    fn from(e: ya_core_model::payment::SendError) -> Self {
        Into::<ya_core_model::payment::RpcMessageError>::into(e).into()
    }
}

impl From<ya_core_model::payment::AcceptRejectError> for Error {
    fn from(e: ya_core_model::payment::AcceptRejectError) -> Self {
        Into::<ya_core_model::payment::RpcMessageError>::into(e).into()
    }
}

impl From<ya_core_model::payment::CancelError> for Error {
    fn from(e: ya_core_model::payment::CancelError) -> Self {
        Into::<ya_core_model::payment::RpcMessageError>::into(e).into()
    }
}
