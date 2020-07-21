use actix_rt::Arbiter;
use chrono::{DateTime, Utc};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex}, // TODO: futures Mutex
    time::Duration,
};

use ya_client::model::payment;
use ya_client::payment::PaymentRequestorApi;

pub(crate) async fn allocate_funds(
    api: &PaymentRequestorApi,
    allocation_size: i64,
) -> anyhow::Result<payment::Allocation> {
    let new_allocation = payment::NewAllocation {
        address: None,
        payment_platform: None,
        total_amount: allocation_size.into(),
        timeout: None,
        make_deposit: false,
    };
    match api.create_allocation(&new_allocation).await {
        Ok(alloc) => {
            log::info!(
                "\n\n ALLOCATED {} GNT ({})",
                alloc.total_amount,
                alloc.allocation_id
            );
            Ok(alloc)
        }
        Err(err) => Err(err.into()),
    }
}

/// MOCK: log incoming debit notes, and... ignore them
pub(crate) async fn log_and_ignore_debit_notes(
    payment_api: PaymentRequestorApi,
    started_at: DateTime<Utc>,
) {
    // FIXME: should be persisted and restored upon next ya-requestor start
    let mut events_after = started_at.clone();
    let timeout = Some(Duration::from_secs(60));

    loop {
        match payment_api
            .get_debit_note_events(Some(&events_after), timeout)
            .await
        {
            Err(e) => {
                log::error!("getting debit notes events error: {}", e);
                tokio::time::delay_for(Duration::from_secs(5)).await;
            }
            Ok(events) => {
                for event in events {
                    log::info!("got debit note event {:?}", event);
                    events_after = event.timestamp;
                }
            }
        }
    }
}

pub(crate) async fn process_payments(
    payment_api: PaymentRequestorApi,
    started_at: DateTime<Utc>,
    agreement_allocation: Arc<Mutex<HashMap<String, String>>>,
    app_abort_handle: Option<futures::future::AbortHandle>,
) {
    log::info!("\n\n INVOICES processing start");
    // FIXME: should be persisted and restored upon next ya-requestor start
    let mut events_after = started_at;
    let timeout = Some(Duration::from_secs(60));

    loop {
        let events = match payment_api
            .get_invoice_events(Some(&events_after), timeout)
            .await
        {
            Err(e) => {
                log::error!("getting invoice events error: {}", e);
                tokio::time::delay_for(Duration::from_secs(5)).await;
                vec![]
            }
            Ok(events) => events,
        };

        for event in events {
            log::info!("got event {:#?}", event);
            match event.event_type {
                payment::EventType::Received => Arbiter::spawn(process_invoice(
                    payment_api.clone(),
                    event.invoice_id,
                    agreement_allocation.clone(),
                    app_abort_handle.clone(),
                )),
                _ => log::warn!(
                    "ignoring event type {:?} for: {}",
                    event.event_type,
                    event.invoice_id
                ),
            }
            events_after = event.timestamp;
        }
    }
}

async fn process_invoice(
    payment_api: PaymentRequestorApi,
    invoice_id: String,
    agreement_allocation: Arc<Mutex<HashMap<String, String>>>,
    app_abort_handle: Option<futures::future::AbortHandle>,
) {
    let mut invoice = payment_api.get_invoice(&invoice_id).await;
    while let Err(e) = invoice {
        log::error!("retry getting invoice {} after error: {}", invoice_id, e);
        tokio::time::delay_for(Duration::from_secs(5)).await;
        invoice = payment_api.get_invoice(&invoice_id).await;
    }

    let invoice = invoice.unwrap();
    log::debug!("got INVOICE: {:#?}", invoice);

    let allocation = agreement_allocation
        .lock()
        .unwrap()
        .get(&invoice.agreement_id)
        .cloned();

    match allocation {
        Some(allocation_id) => {
            let acceptance = payment::Acceptance {
                total_amount_accepted: invoice.amount,
                allocation_id: allocation_id.clone(),
            };
            match payment_api.accept_invoice(&invoice_id, &acceptance).await {
                // TODO: reconsider what to do in this case: probably retry
                Err(e) => log::error!("accepting invoice {}, error: {}", invoice_id, e),
                Ok(_) => log::info!("\n\n INVOICE ACCEPTED: {:?}", invoice_id),
            }

            agreement_allocation
                .lock()
                .unwrap()
                .remove(&invoice.agreement_id);

            // FIXME: Allocation should be released after the payment is made.
            // Doing it immediately after accepting invoice causes payment to fail.
            // match payment_api.release_allocation(&allocation_id).await {
            //     Ok(_) => log::info!("released allocation {}", allocation_id),
            //     Err(e) => log::error!("Unable to release allocation {}: {}", allocation_id, e),
            // }
        }
        None => {
            let rejection = payment::Rejection {
                rejection_reason: payment::RejectionReason::UnsolicitedService,
                total_amount_accepted: 0.into(),
                message: None,
            };
            match payment_api.reject_invoice(&invoice_id, &rejection).await {
                Err(e) => log::error!("rejecting invoice {}, error: {}", invoice_id, e),
                Ok(_) => log::warn!("invoice rejected: {:?}", invoice_id),
            }
        }
    }

    app_abort_handle.map(|h| h.abort());
}
