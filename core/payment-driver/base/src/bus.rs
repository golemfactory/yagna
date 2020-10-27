/*
    Collection of interactions a PaymendDriver can have with ya_service_bus

    All interactions with the bus from the driver should go through this mod.
*/

// Extrernal crates
use actix::Arbiter;
use std::sync::Arc;
use uuid::Uuid;

// Workspace uses
use ya_client_model::NodeId;
use ya_core_model::driver::{
    driver_bus_id, AccountMode, GenericError, PaymentConfirmation, PaymentDetails,
};
use ya_core_model::identity;
use ya_core_model::payment::local as payment_srv;
use ya_service_bus::{
    typed::{service, ServiceBinder},
    RpcEndpoint,
};

// Local uses
use crate::driver::PaymentDriver;

pub fn bind_service(driver: Arc<dyn PaymentDriver>) {
    log::debug!("Binding payment driver service to service bus");

    /* Short variable names explained:
        db = DbExecutor || ()
        dr = Arc<dyn PaymentDriver>
        c = caller
        m = message
    */
    #[rustfmt::skip] // Keep move's neatly alligned
    ServiceBinder::new(&driver_bus_id(driver.get_name()), &(), driver)
        .bind_with_processor(
            move |db, dr, c, m| async move { dr.init(db, c, m).await }
        )
        .bind_with_processor(
            move |db, dr, c, m| async move { dr.account_event(db, c, m).await }
        )
        .bind_with_processor(
            move |db, dr, c, m| async move { dr.get_account_balance(db, c, m).await }
        )
        .bind_with_processor(
            move |db, dr, c, m| async move { dr.get_transaction_balance(db, c, m).await }
        )
        .bind_with_processor(
            move |db, dr, c, m| async move { dr.schedule_payment(db, c, m).await }
        )
        .bind_with_processor(
            move |db, dr, c, m| async move { dr.verify_payment(db, c, m).await }
        );

    log::debug!("Successfully bound payment driver service to service bus");
}

pub async fn subscribe_to_identity_events(driver: Arc<dyn PaymentDriver>) {
    log::debug!("Subscribing to identity events");
    let message = identity::Subscribe {
        endpoint: driver_bus_id(driver.get_name()),
    };
    let result = service(identity::BUS_ID).send(message).await;
    match result {
        Err(e) => log::error!("init app-key listener error: {}", e),
        _ => log::debug!("Successfully subscribed payment driver service to identity events"),
    }
}

pub async fn register_account(
    driver: &(dyn PaymentDriver),
    address: &str,
    mode: AccountMode,
) -> Result<(), GenericError> {
    let address = address.to_string();
    let msg = payment_srv::RegisterAccount {
        platform: driver.get_platform(),
        address,
        driver: driver.get_name(),
        mode,
    };
    service(payment_srv::BUS_ID)
        .send(msg)
        .await
        .map_err(GenericError::new)?
        .map_err(GenericError::new)?;
    Ok(())
}

pub async fn sign(node_id: NodeId, payload: Vec<u8>) -> Result<Vec<u8>, GenericError> {
    let signature = service(identity::BUS_ID)
        .send(identity::Sign { node_id, payload })
        .await
        .map_err(GenericError::new)?
        .map_err(GenericError::new)?;
    Ok(signature)
}

pub fn notify_payment(
    driver: &(dyn PaymentDriver),
    details: &PaymentDetails,
    confirmation: Vec<u8>,
) -> String {
    let order_id = Uuid::new_v4().to_string();
    let msg = payment_srv::NotifyPayment {
        driver: driver.get_name(),
        amount: details.amount.clone(),
        sender: details.sender.clone(),
        recipient: details.recipient.clone(),
        order_ids: vec![order_id.clone()],
        confirmation: PaymentConfirmation { confirmation },
    };

    // Spawned because calling payment service while handling a call from payment service
    // would result in a deadlock.
    Arbiter::spawn(async move {
        let _ = service(payment_srv::BUS_ID)
            .send(msg)
            .await
            .map_err(|e| log::error!("{}", e));
    });
    order_id
}
