use ya_core_model::payment::local::ScheduleError;
use ya_core_model::payment::public::{AcceptRejectError, CancelError, SendError};
use ya_core_model::payment::RpcMessageError;

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
pub enum PaymentError {
    #[error("Currency conversion error: {0}")]
    Conversion(String),
    #[error("Invalid address: {0}")]
    Address(String),
    #[error("Verification error: {0}")]
    Verification(String),
    #[error("Payment driver error: {0}")]
    Driver(#[from] ya_payment_driver::PaymentDriverError),
}

pub type PaymentResult<T> = Result<T, PaymentError>;

impl From<uint::FromDecStrErr> for PaymentError {
    fn from(e: uint::FromDecStrErr) -> Self {
        Self::Conversion(format!("{:?}", e))
    }
}

impl From<bigdecimal::ParseBigDecimalError> for PaymentError {
    fn from(e: bigdecimal::ParseBigDecimalError) -> Self {
        Self::Conversion(e.to_string())
    }
}

impl From<PaymentError> for ScheduleError {
    fn from(e: PaymentError) -> Self {
        match e {
            PaymentError::Conversion(e) => ScheduleError::Conversion(e),
            PaymentError::Address(e) => ScheduleError::Address(e),
            PaymentError::Driver(e) => ScheduleError::Driver(e.to_string()),
            PaymentError::Verification(e) => panic!(e),
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Database error: {0}")]
    Database(#[from] DbError),
    #[error("Service bus error: {0}")]
    ServiceBus(#[from] ya_service_bus::Error),
    #[error("External service error: {0}")]
    ExtService(#[from] ExternalServiceError),
    #[error("Payment error: {0}")]
    Payment(#[from] PaymentError),
    #[error("RPC error: {0}")]
    Rpc(#[from] RpcMessageError),
    #[error("Timeout")]
    Timeout(#[from] tokio::time::Elapsed),
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

impl From<ya_payment_driver::PaymentDriverError> for Error {
    fn from(e: ya_payment_driver::PaymentDriverError) -> Self {
        Into::<PaymentError>::into(e).into()
    }
}
