use thiserror::Error;

#[derive(Debug, Clone, Error)]
pub enum PaymentDriverError {
    #[error("Insufficient gas")]
    InsufficientGas,
    #[error("Insufficient funds")]
    InsufficientFunds,
    #[error("Payment already scheduled")]
    AlreadyScheduled,
    #[error("Payment not found")]
    NotFound,
    #[error("Connection refused")]
    ConnectionRefused,
    #[error("Library error")]
    LibraryError { msg: String },
}

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
