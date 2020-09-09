use thiserror::Error;

#[derive(thiserror::Error, Debug)]
pub enum DbError {
    #[error("Database connection error: {0}")]
    Connection(#[from] r2d2::Error),
    #[error("Database query error: {0}")]
    Query(#[from] diesel::result::Error),
    #[error("Runtime error: {0}")]
    Runtime(#[from] tokio::task::JoinError),
    #[error("{0}")]
    InvalidData(String),
}

#[derive(Debug, Clone, Error)]
pub enum GNTDriverError {
    #[error("Insufficient gas")]
    InsufficientGas,
    #[error("Insufficient funds")]
    InsufficientFunds,
    #[error("Payment: {0} already scheduled")]
    PaymentAlreadyScheduled(String),
    #[error("Unknown payment: {0}")]
    UnknownPayment(String),
    #[error("Payment: {0} not found")]
    PaymentNotFound(String),
    #[error("Connection refused")]
    ConnectionRefused,
    #[error("Library error: {0}")]
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
    #[error("Unknown chain: {0}")]
    UnknownChain(String),
    #[error("Account is locked: {0}")]
    AccountLocked(String),
    #[error("GSB error: {0}")]
    GSBError(String),
}

impl GNTDriverError {
    pub fn library_err_msg<D: std::fmt::Display>(msg: D) -> Self {
        GNTDriverError::LibraryError(msg.to_string())
    }
}

impl From<secp256k1::Error> for GNTDriverError {
    fn from(e: secp256k1::Error) -> Self {
        GNTDriverError::LibraryError(e.to_string())
    }
}

impl From<DbError> for GNTDriverError {
    fn from(e: DbError) -> Self {
        GNTDriverError::DatabaseError(e.to_string())
    }
}

impl From<uint::FromDecStrErr> for GNTDriverError {
    fn from(e: uint::FromDecStrErr) -> Self {
        Self::Conversion(format!("{:?}", e))
    }
}

impl From<hex::FromHexError> for GNTDriverError {
    fn from(e: hex::FromHexError) -> Self {
        Self::Conversion(format!("{:?}", e))
    }
}

impl From<bigdecimal::ParseBigDecimalError> for GNTDriverError {
    fn from(e: bigdecimal::ParseBigDecimalError) -> Self {
        Self::Conversion(e.to_string())
    }
}

impl From<actix::MailboxError> for GNTDriverError {
    fn from(e: actix::MailboxError) -> Self {
        GNTDriverError::LibraryError(e.to_string())
    }
}
