use ya_core_model::payment::local::{GenericError, ValidateAllocationError};
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

impl From<serde_json::Error> for DbError {
    fn from(e: serde_json::Error) -> Self {
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
pub enum Error {
    #[error("Database error: {0}")]
    Database(#[from] DbError),
    #[error("Service bus error: {0}")]
    ServiceBus(#[from] ya_service_bus::Error),
    #[error("Network error: {0}")]
    Network(#[from] ya_net::NetApiError),
    #[error("External service error: {0}")]
    ExtService(#[from] ExternalServiceError),
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

impl From<GenericError> for Error {
    fn from(e: GenericError) -> Self {
        Into::<RpcMessageError>::into(e).into()
    }
}

impl From<ValidateAllocationError> for Error {
    fn from(e: ValidateAllocationError) -> Self {
        Into::<RpcMessageError>::into(e).into()
    }
}

pub mod processor {
    use super::DbError;
    use crate::models::activity::ReadObj as Activity;
    use crate::models::agreement::ReadObj as Agreement;
    use crate::models::order::ReadObj as Order;
    use bigdecimal::BigDecimal;
    use std::fmt::Display;
    use ya_core_model::driver::AccountMode;
    use ya_core_model::payment::local::{
        GenericError, ValidateAllocationError as GsbValidateAllocationError,
    };
    use ya_core_model::payment::public::SendError;

    #[derive(thiserror::Error, Debug)]
    #[error(
        "Account not registered. Hint: Did you run `yagna payment init --{}` after restarting? platform={platform} address={address} mode={mode:?}",
        if *.mode == AccountMode::SEND {"sender"} else {"receiver"}
    )]
    pub struct AccountNotRegistered {
        platform: String,
        address: String,
        mode: AccountMode,
    }

    impl AccountNotRegistered {
        pub fn new(platform: &str, address: &str, mode: AccountMode) -> Self {
            Self {
                platform: platform.to_owned(),
                address: address.to_owned(),
                mode,
            }
        }
    }

    #[derive(thiserror::Error, Debug)]
    pub enum SchedulePaymentError {
        #[error("{0}")]
        InvalidInput(String),
        #[error("{0}")]
        AccountNotRegistered(#[from] AccountNotRegistered),
        #[error("Service bus error: {0}")]
        ServiceBus(#[from] ya_service_bus::error::Error),
        #[error("Payment Driver Service error: {0}")]
        Driver(#[from] ya_core_model::driver::GenericError),
        #[error("Database error: {0}")]
        Database(#[from] DbError),
        #[error("Payment service is shutting down")]
        Shutdown,
    }

    impl From<SchedulePaymentError> for GenericError {
        fn from(e: SchedulePaymentError) -> Self {
            GenericError::new(e)
        }
    }

    #[derive(thiserror::Error, Debug)]
    #[error("{0}")]
    pub struct OrderValidationError(String);

    impl OrderValidationError {
        pub fn new<T: Display>(e: T) -> Self {
            Self(e.to_string())
        }

        pub fn platform(order: &Order, platform: &str) -> Result<(), Self> {
            Err(Self(format!(
                "Invalid platform for payment order {}: {} != {}",
                order.id, order.payment_platform, platform
            )))
        }

        pub fn payer_addr(order: &Order, payer_addr: &str) -> Result<(), Self> {
            Err(Self(format!(
                "Invalid payer address for payment order {}: {} != {}",
                order.id, order.payer_addr, payer_addr
            )))
        }

        pub fn payee_addr(order: &Order, payee_addr: &str) -> Result<(), Self> {
            Err(Self(format!(
                "Invalid payee address for payment order {}: {} != {}",
                order.id, order.payee_addr, payee_addr
            )))
        }

        pub fn amount(expected: &BigDecimal, actual: &BigDecimal) -> Result<(), Self> {
            Err(Self(format!(
                "Invalid payment amount: {} != {}",
                expected, actual
            )))
        }
    }

    #[derive(thiserror::Error, Debug)]
    pub enum NotifyPaymentError {
        #[error("{0}")]
        Validation(#[from] OrderValidationError),
        #[error("Service bus error: {0}")]
        ServiceBus(#[from] ya_service_bus::error::Error),
        #[error("Error while sending payment: {0}")]
        Send(#[from] SendError),
        #[error("Database error: {0}")]
        Database(#[from] DbError),
        #[error("Singning error: {0}")]
        Sign(#[from] ya_core_model::driver::GenericError),
    }

    impl NotifyPaymentError {
        pub fn invalid_order(order: &Order) -> Result<(), Self> {
            Err(Self::Validation(OrderValidationError::new(format!(
                "Invalid payment order retrieved from database: {:?}",
                order
            ))))
        }
    }

    impl From<NotifyPaymentError> for GenericError {
        fn from(e: NotifyPaymentError) -> Self {
            GenericError::new(e)
        }
    }

    #[derive(thiserror::Error, Debug)]
    pub enum VerifyPaymentError {
        #[error("Invalid payment signature")]
        InvalidSignature,
        #[error("Confirmation is not base64-encoded")]
        ConfirmationEncoding,
        #[error("{0}")]
        AccountNotRegistered(#[from] AccountNotRegistered),
        #[error("Service bus error: {0}")]
        ServiceBus(#[from] ya_service_bus::error::Error),
        #[error("Error while sending payment: {0}")]
        Driver(#[from] ya_core_model::driver::GenericError),
        #[error("Database error: {0}")]
        Database(#[from] DbError),
        #[error("{0}")]
        Validation(String),
    }

    impl VerifyPaymentError {
        pub fn amount(actual: &BigDecimal, declared: &BigDecimal) -> Result<(), Self> {
            Err(Self::Validation(format!(
                "Invalid payment amount. Declared: {} Actual: {}",
                declared, actual
            )))
        }

        pub fn shares(
            total: &BigDecimal,
            agreement_sum: &BigDecimal,
            activity_sum: &BigDecimal,
        ) -> Result<(), Self> {
            Err(Self::Validation(format!(
                "Payment shares do not sum up. {} != {} + {}",
                total, agreement_sum, activity_sum
            )))
        }

        pub fn recipient(declared: &str, actual: &str) -> Result<(), Self> {
            Err(Self::Validation(format!(
                "Invalid transaction recipient. Declared: {} Actual: {}",
                &declared, &actual
            )))
        }

        pub fn sender(declared: &str, actual: &str) -> Result<(), Self> {
            Err(Self::Validation(format!(
                "Invalid transaction sender. Declared: {} Actual: {}",
                &declared, &actual
            )))
        }

        pub fn agreement_zero_amount(agreement_id: &str) -> Result<(), Self> {
            Err(Self::Validation(format!(
                "Agreement with 0 amount: {}",
                agreement_id
            )))
        }

        pub fn agreement_not_found(agreement_id: &str) -> Result<(), Self> {
            Err(Self::Validation(format!(
                "Agreement not found: {}",
                agreement_id
            )))
        }

        pub fn agreement_payee(agreement: &Agreement, payee_addr: &str) -> Result<(), Self> {
            Err(Self::Validation(format!(
                "Invalid payee address for agreement {}: {} != {}",
                agreement.id, agreement.payee_addr, payee_addr
            )))
        }

        pub fn agreement_payer(agreement: &Agreement, payer_addr: &str) -> Result<(), Self> {
            Err(Self::Validation(format!(
                "Invalid payer address for agreement {}: {} != {}",
                agreement.id, agreement.payer_addr, payer_addr
            )))
        }

        pub fn agreement_platform(agreement: &Agreement, platform: &str) -> Result<(), Self> {
            Err(Self::Validation(format!(
                "Invalid payment platform for agreement {}: {} != {}",
                agreement.id, agreement.payment_platform, platform
            )))
        }

        pub fn activity_not_found(activity_id: &str) -> Result<(), Self> {
            Err(Self::Validation(format!(
                "Activity not found: {}",
                activity_id
            )))
        }

        pub fn activity_payee(activity: &Activity, payee_addr: &str) -> Result<(), Self> {
            Err(Self::Validation(format!(
                "Invalid payee address for activity {}: {} != {}",
                activity.id, activity.payee_addr, payee_addr
            )))
        }

        pub fn activity_payer(activity: &Activity, payer_addr: &str) -> Result<(), Self> {
            Err(Self::Validation(format!(
                "Invalid payer address for activity {}: {} != {}",
                activity.id, activity.payer_addr, payer_addr
            )))
        }

        pub fn balance() -> Result<(), Self> {
            Err(Self::Validation(
                "Transaction balance too low (probably tx hash re-used)".to_owned(),
            ))
        }
    }

    #[derive(thiserror::Error, Debug)]
    pub enum GetStatusError {
        #[error("Please wait. Account is not yet initialized. platform={} address={}", .0.platform, .0.address)]
        AccountNotRegistered(#[from] AccountNotRegistered),
        #[error("Service bus error: {0}")]
        ServiceBus(#[from] ya_service_bus::error::Error),
        #[error("Error while sending payment: {0}")]
        Driver(#[from] ya_core_model::driver::GenericError),
    }

    #[derive(thiserror::Error, Debug)]
    pub enum ValidateAllocationError {
        #[error("{0}")]
        AccountNotRegistered(#[from] AccountNotRegistered),
        #[error("Service bus error: {0}")]
        ServiceBus(#[from] ya_service_bus::error::Error),
        #[error("Error while sending payment: {0}")]
        Driver(#[from] ya_core_model::driver::GenericError),
        #[error("Database error: {0}")]
        Database(#[from] DbError),
        #[error("Payment service is shutting down")]
        Shutdown,
    }

    impl From<ValidateAllocationError> for GsbValidateAllocationError {
        fn from(e: ValidateAllocationError) -> Self {
            match e {
                ValidateAllocationError::AccountNotRegistered(e) => {
                    GsbValidateAllocationError::AccountNotRegistered
                }
                e => GsbValidateAllocationError::Other(e.to_string()),
            }
        }
    }
}
