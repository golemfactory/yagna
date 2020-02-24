use thiserror::Error;

#[derive(Debug, Clone, Error)]
pub enum PaymentDriverError {
    #[error("Insufficient funds")]
    InsufficientFunds,
    #[error("Payment already scheduled")]
    AlreadyScheduled,
    #[error("Payment not found")]
    NotFound,
    #[error("Connection refused")]
    ConnectionRefused,
}
