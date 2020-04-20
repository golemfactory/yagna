use actix_rt::Arbiter;
use chrono::{DateTime, Utc};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex}, // TODO: futures Mutex
    time::Duration,
};

use ya_client::payment::requestor::RequestorApi;
use ya_model::payment;

pub(crate) async fn allocate_funds(
    api: &RequestorApi,
    allocation_size: i64,
) -> anyhow::Result<payment::Allocation> {
    let new_allocation = payment::NewAllocation {
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
    payment_api: RequestorApi,
    started_at: DateTime<Utc>,
) {
    // FIXME: should be persisted and restored upon next ya-requestor start
    let mut events_after = started_at.clone();

    loop {
        match payment_api.get_debit_note_events(Some(&events_after)).await {
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
    payment_api: RequestorApi,
    started_at: DateTime<Utc>,
    agreement_allocation: Arc<Mutex<HashMap<String, String>>>,
) {
    log::info!("\n\n INVOICES processing start");
    // FIXME: should be persisted and restored upon next ya-requestor start
    let mut events_after = started_at;

    loop {
        let events = match payment_api.get_invoice_events(Some(&events_after)).await {
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
    payment_api: RequestorApi,
    invoice_id: String,
    agreement_allocation: Arc<Mutex<HashMap<String, String>>>,
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
            match payment_api.release_allocation(&allocation_id).await {
                Ok(_) => log::info!("released allocation {}", allocation_id),
                Err(e) => log::error!("Unable to release allocation {}: {}", allocation_id, e),
            }
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
}
