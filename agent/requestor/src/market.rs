use actix_rt::Arbiter;
use chrono::Utc;
use futures::{channel::mpsc, prelude::*};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use ya_agreement_utils::{constraints, ConstraintKey, Constraints};
use ya_client::cli::RequestorApi;
use ya_client::model::market::{
    proposal::State, AgreementProposal, Demand, Proposal, RequestorEvent,
};

use crate::payment::allocate_funds;

pub(crate) fn build_demand(
    node_name: &str,
    task_package: &str,
    expires: chrono::Duration,
    subnet: &Option<String>,
) -> Demand {
    let expiration = Utc::now() + expires;
    let mut properties = serde_json::json!({
        "golem": {
            "node.id.name": node_name,
            "srv.comp.task_package": task_package,
            "srv.comp.expiration": expiration.timestamp_millis(),
        },
    });

    let mut cnts = constraints![
        "golem.inf.mem.gib" > 0.5,
        "golem.inf.storage.gib" > 1,
        "golem.com.pricing.model" == "linear",
    ];
    if let Some(subnet) = subnet {
        log::info!("Using subnet: {}", subnet);
        properties.as_object_mut().unwrap().insert(
            "golem.node.debug.subnet".to_string(),
            serde_json::Value::String(subnet.clone()),
        );
        cnts = cnts.and(constraints!["golem.node.debug.subnet" == subnet.clone(),]);
    };

    Demand {
        properties,
        constraints: cnts.to_string(),

        demand_id: Default::default(),
        requestor_id: Default::default(),
    }
}

enum ProcessOfferResult {
    ProposalId(String),
    AgreementId(String),
}

pub(crate) async fn spawn_negotiations(
    api: &RequestorApi,
    subscription_id: &str,
    my_demand: &Demand,
    allocation_size: i64,
    agreement_allocation: Arc<Mutex<HashMap<String, String>>>,
    agreement_tx: mpsc::Sender<String>,
) -> anyhow::Result<()> {
    loop {
        let events = api
            .market
            .collect(&subscription_id, Some(5.0), Some(5))
            .await?;

        if !events.is_empty() {
            log::debug!("got {} market events", events.len());
        }
        for event in events {
            match event {
                RequestorEvent::ProposalEvent {
                    event_date: _,
                    proposal: offer,
                } => {
                    log::debug!(
                        "\n\n got ProposalEvent [{}]; state: {:?}",
                        offer.proposal_id()?,
                        offer.state
                    );
                    log::trace!("offer proposal: {:#?}", offer);
                    let mut agreement_tx = agreement_tx.clone();
                    let api = api.clone();
                    let subscription_id = subscription_id.to_string();
                    let my_demand = my_demand.clone();
                    let agreement_allocation = agreement_allocation.clone();
                    Arbiter::spawn(async move {
                        match negotiate_offer(
                            api,
                            offer,
                            &subscription_id,
                            my_demand,
                            allocation_size,
                            agreement_allocation,
                        )
                        .await
                        {
                            Ok(ProcessOfferResult::ProposalId(id)) => {
                                log::info!("\n\n ACCEPTED via counter proposal [{}]", id);
                            }
                            Ok(ProcessOfferResult::AgreementId(id)) => {
                                agreement_tx.send(id).await.unwrap();
                            }
                            Err(e) => {
                                log::error!("unable to process offer: {}", e);
                                return;
                            }
                        }
                    });
                }
                _ => {
                    log::warn!("invalid response");
                }
            }
        }
    }
}

async fn negotiate_offer(
    api: RequestorApi,
    offer: Proposal,
    subscription_id: &str,
    my_demand: Demand,
    allocation_size: i64,
    agreement_allocation: Arc<Mutex<HashMap<String, String>>>,
) -> anyhow::Result<ProcessOfferResult> {
    let proposal_id = offer.proposal_id()?.clone();

    if offer.state.unwrap_or(State::Initial) == State::Initial {
        if offer.prev_proposal_id.is_some() {
            anyhow::bail!("Proposal in Initial state but with prev id: {:#?}", offer)
        }
        let bespoke_proposal = offer.counter_demand(my_demand)?;
        let new_proposal_id = api
            .market
            .counter_proposal(&bespoke_proposal, subscription_id)
            .await?;
        return Ok(ProcessOfferResult::ProposalId(new_proposal_id));
    }

    let new_agreement =
        AgreementProposal::new(proposal_id.clone(), Utc::now() + chrono::Duration::hours(2));
    log::info!("\n\n creating new AGREEMENT");
    let new_agreement_id = api.market.create_agreement(&new_agreement).await?;

    log::info!("\n\n allocating funds for agreement: {}", new_agreement_id);
    match allocate_funds(&api.payment, allocation_size).await {
        Ok(alloc) => {
            agreement_allocation
                .lock()
                .unwrap()
                .insert(new_agreement_id.clone(), alloc.allocation_id);
            log::info!("\n\n confirming agreement: {}", new_agreement_id);
            api.market.confirm_agreement(&new_agreement_id).await?;
        }
        Err(err) => {
            log::error!(
                "unable to allocate {} NGNT: {:?};\n\n cancelling agreement...",
                allocation_size,
                err
            );
            match api.market.cancel_agreement(&new_agreement_id).await {
                Ok(_) => log::warn!("\n\n agreement {} CANCELLED", new_agreement_id),
                Err(e) => log::error!("unable to cancel agreement {}: {}", new_agreement_id, e),
            }
            anyhow::bail!("unable to allocate {} NGNT: {:?}", allocation_size, err);
        }
    }

    log::info!("\n\n waiting for agreement approval: {}", new_agreement_id);
    let result = api
        .market
        .wait_for_approval(&new_agreement_id, Some(7.879))
        .await?;

    match &result[..] {
        "Approved" => {
            log::info!("\n\n AGREEMENT APPROVED: {} !", new_agreement_id);
            Ok(ProcessOfferResult::AgreementId(new_agreement_id))
        }
        "Rejected" => {
            log::info!("\n\n AGREEMENT REJECTED: {} !", new_agreement_id);
            anyhow::bail!("Agreement rejected by provider: {} !", new_agreement_id)
        }
        r => anyhow::bail!(
            "Unknown response: '{}' for agreement: {} !",
            r,
            new_agreement_id
        ),
    }
}
