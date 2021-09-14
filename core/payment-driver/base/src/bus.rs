/*
    Collection of interactions a PaymentDriver can have with ya_service_bus

    All interactions with the bus from the driver should go through this mod.
*/

// External crates
use std::sync::Arc;

// Workspace uses
use ya_client_model::payment::driver_details::DriverDetails;
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
use crate::dao::DbExecutor;
use crate::driver::PaymentDriver;

pub async fn bind_service<Driver: PaymentDriver + 'static>(
    db: &DbExecutor,
    driver: Arc<Driver>,
) -> anyhow::Result<()> {
    log::debug!("Binding payment driver service to service bus...");
    let bus_id = driver_bus_id(driver.get_name());

    /* Short variable names explained:
        db = DbExecutor || ()
        dr = Arc<dyn PaymentDriver>
        c = caller
        m = message
    */
    #[rustfmt::skip] // Keep move's neatly aligned
    ServiceBinder::new(&bus_id, db, driver.clone())
        .bind_with_processor(
            move |db, dr, c, m| async move { dr.account_event(db, c, m).await }
        )
        .bind_with_processor(
            move |db, dr, c, m| async move { dr.enter(db, c, m).await }
        )
        .bind_with_processor(move |_, dr, _, m| async move { dr.exit_fee(m).await })
        .bind_with_processor(
            move |db, dr, c, m| async move { dr.exit(db, c, m).await }
        )
        .bind_with_processor(
            move |db, dr, c, m| async move { dr.fund(db, c, m).await }
        )
        .bind_with_processor(
            move |db, dr, c, m| async move { dr.get_account_balance(db, c, m).await }
        )
        .bind_with_processor(
            move |db, dr, c, m| async move { dr.init(db, c, m).await }
        )
        .bind_with_processor(
            move |db, dr, c, m| async move { dr.transfer(db, c, m).await }
        )
        .bind_with_processor(
            move |db, dr, c, m| async move { dr.schedule_payment(db, c, m).await }
        )
        .bind_with_processor(
            move |db, dr, c, m| async move { dr.verify_payment(db, c, m).await }
        )
        .bind_with_processor(
            move |db, dr, c, m| async move { dr.validate_allocation(db, c, m).await }
        )
        .bind_with_processor(
            move |db, dr, c, m| async move { dr.sign_payment(db, c, m).await }
        )
        .bind_with_processor(
            move |db, dr, c, m| async move { dr.verify_signature(db, c, m).await }
        )
        .bind_with_processor(
            move |db, dr, c, m| async move { dr.shut_down(db, c, m).await }
        );

    log::debug!("Successfully bound payment driver service to service bus.");

    log::debug!("Subscribing to identity events...");
    let message = identity::Subscribe { endpoint: bus_id };
    service(identity::BUS_ID).send(message).await??;
    log::debug!("Successfully subscribed payment driver service to identity events.");

    log::debug!("Registering driver in payment service...");
    let message = payment_srv::RegisterDriver {
        driver_name: driver.get_name(),
        details: DriverDetails {
            default_network: driver.get_default_network(),
            networks: driver.get_networks(),
            recv_init_required: driver.recv_init_required(),
        },
    };
    service(payment_srv::BUS_ID).send(message).await?.unwrap(); // Unwrap on purpose because it's NoError
    log::debug!("Successfully registered driver in payment service.");

    Ok(())
}

pub async fn list_unlocked_identities() -> Result<Vec<NodeId>, GenericError> {
    log::debug!("list_unlocked_identities");
    let message = identity::List {};
    let result = service(identity::BUS_ID)
        .send(message)
        .await
        .map_err(GenericError::new)?
        .map_err(GenericError::new)?;
    let unlocked_list = result
        .iter()
        .filter(|n| !n.is_locked)
        .map(|n| n.node_id)
        .collect();
    log::debug!(
        "list_unlocked_identities completed. result={:?}",
        unlocked_list
    );
    Ok(unlocked_list)
}

pub async fn register_account(
    driver: &(dyn PaymentDriver),
    address: &str,
    network: &str,
    token: &str,
    mode: AccountMode,
) -> Result<(), GenericError> {
    let msg = payment_srv::RegisterAccount {
        address: address.to_string(),
        driver: driver.get_name(),
        network: network.to_string(),
        token: token.to_string(),
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

pub async fn notify_payment(
    driver_name: &str,
    platform: &str,
    order_ids: Vec<String>,
    details: &PaymentDetails,
    confirmation: Vec<u8>,
) -> Result<(), GenericError> {
    let msg = payment_srv::NotifyPayment {
        driver: driver_name.to_string(),
        platform: platform.to_string(),
        amount: details.amount.clone(),
        sender: details.sender.clone(),
        recipient: details.recipient.clone(),
        order_ids,
        confirmation: PaymentConfirmation { confirmation },
    };
    service(payment_srv::BUS_ID)
        .send(msg)
        .await
        .map_err(GenericError::new)?
        .map_err(GenericError::new)?;
    Ok(())
}
