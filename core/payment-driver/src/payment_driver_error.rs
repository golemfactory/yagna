use std::error;
use std::fmt;

#[derive(Debug, Clone)]
pub enum PaymentDriverError {
    InsufficientFunds,
    ConnectionRefused,
}

#[allow(unused)]
impl fmt::Display for PaymentDriverError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        unimplemented!();
    }
}

#[allow(unused)]
impl error::Error for PaymentDriverError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        unimplemented!();
    }
}

impl PaymentDriverError {}
