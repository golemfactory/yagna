use ya_core_model::payment::local::ScheduleError;
use ya_core_model::payment::public::{AcceptRejectError, CancelError, SendError};
use ya_core_model::payment::RpcMessageError;

#[derive(thiserror::Error, Debug)]
pub enum DbError {
    #[error("Connection error: {0}")]
    Connection(#[from] r2d2::Error),
    #[error("Runtime error: {0}")]
    Runtime(#[from] tokio::task::JoinError),
    #[error("Query error: {0}")]
    Query(String),
    #[error("Data integrity error: {0}")]
    Integrity(String),
}

impl From<diesel::result::Error> for DbError {
    fn from(e: diesel::result::Error) -> Self {
        DbError::Query(e.to_string())
    }
}

impl From<std::string::FromUtf8Error> for DbError {
    fn from(e: std::string::FromUtf8Error) -> Self {
        DbError::Integrity(e.to_string())
    }
}

impl From<ya_client_model::payment::document_status::InvalidOption> for DbError {
    fn from(e: ya_client_model::payment::document_status::InvalidOption) -> Self {
        DbError::Integrity(e.to_string())
    }
}

impl From<ya_client_model::payment::event_type::InvalidOption> for DbError {
    fn from(e: ya_client_model::payment::event_type::InvalidOption) -> Self {
        DbError::Integrity(e.to_string())
    }
}

pub type DbResult<T> = Result<T, DbError>;

#[derive(thiserror::Error, Debug)]
pub enum ExternalServiceError {
    #[error("Activity service error: {0}")]
    Activity(#[from] ya_core_model::activity::RpcMessageError),
    #[error("Market service error: {0}")]
    Market(#[from] ya_core_model::market::RpcMessageError),
}
#[derive(thiserror::Error, Debug)]
pub enum PaymentError {
    #[error("Verification error: {0}")]
    Verification(String),
    #[error("Payment driver error: {0}")]
    Driver(String),
    #[error("Payment Driver Service error: {0}")]
    DriverService(#[from] ya_service_bus::error::Error),
}

pub type PaymentResult<T> = Result<T, PaymentError>;

impl From<PaymentError> for ScheduleError {
    fn from(e: PaymentError) -> Self {
        match e {
            PaymentError::Driver(e) => ScheduleError::Driver(e),
            PaymentError::Verification(e) => panic!(e),
            PaymentError::DriverService(e) => panic!(e),
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Database error: {0}")]
    Database(#[from] DbError),
    #[error("Service bus error: {0}")]
    ServiceBus(#[from] ya_service_bus::Error),
    #[error("Network error: {0}")]
    Network(#[from] ya_net::NetApiError),
    #[error("External service error: {0}")]
    ExtService(#[from] ExternalServiceError),
    #[error("Payment error: {0}")]
    Payment(#[from] PaymentError),
    #[error("RPC error: {0}")]
    Rpc(#[from] RpcMessageError),
    #[error("Timeout")]
    Timeout(#[from] tokio::time::Elapsed),
}

impl From<ya_core_model::activity::RpcMessageError> for Error {
    fn from(e: ya_core_model::activity::RpcMessageError) -> Self {
        Into::<ExternalServiceError>::into(e).into()
    }
}

impl From<ya_core_model::market::RpcMessageError> for Error {
    fn from(e: ya_core_model::market::RpcMessageError) -> Self {
        Into::<ExternalServiceError>::into(e).into()
    }
}

impl From<ScheduleError> for Error {
    fn from(e: ScheduleError) -> Self {
        Into::<RpcMessageError>::into(e).into()
    }
}

impl From<SendError> for Error {
    fn from(e: SendError) -> Self {
        Into::<RpcMessageError>::into(e).into()
    }
}

impl From<AcceptRejectError> for Error {
    fn from(e: AcceptRejectError) -> Self {
        Into::<RpcMessageError>::into(e).into()
    }
}

impl From<CancelError> for Error {
    fn from(e: CancelError) -> Self {
        Into::<RpcMessageError>::into(e).into()
    }
}
