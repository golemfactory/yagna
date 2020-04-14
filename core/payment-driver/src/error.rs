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
    LibraryError(String),
    #[error("Ethereum client error: {0}")]
    EthereumClientError(#[from] web3::Error),
    #[error("Database error: {0}")]
    DatabaseError(String),
    #[error("Unknown transaction")]
    UnknownTransaction,
    #[error("Transaction failed")]
    FailedTransaction,
    #[error("Currency conversion error: {0}")]
    Conversion(String),
    #[error("Invalid address: {0}")]
    Address(String),
    #[error("Missing envirnonment variable: {0}")]
    MissingEnvironmentVariable(#[from] std::env::VarError),
    #[error("Unknown chain: {0}")]
    UnknownChain(u64),
}

impl From<secp256k1::Error> for PaymentDriverError {
    fn from(e: secp256k1::Error) -> Self {
        PaymentDriverError::LibraryError(e.to_string())
    }
}

impl From<DbError> for PaymentDriverError {
    fn from(e: DbError) -> Self {
        PaymentDriverError::DatabaseError(e.to_string())
    }
}

impl From<uint::FromDecStrErr> for PaymentDriverError {
    fn from(e: uint::FromDecStrErr) -> Self {
        Self::Conversion(format!("{:?}", e))
    }
}

impl From<bigdecimal::ParseBigDecimalError> for PaymentDriverError {
    fn from(e: bigdecimal::ParseBigDecimalError) -> Self {
        Self::Conversion(e.to_string())
    }
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
