use std::collections::HashMap;
use std::convert::TryFrom;
use std::sync::Arc;
use std::time::Duration;

use actix::prelude::*;
use anyhow::{anyhow, Error, Result};
use backoff::backoff::Backoff;
use bigdecimal::{BigDecimal, Zero};
use chrono::{DateTime, Utc};
use futures_util::FutureExt;
use humantime;
use log;
use serde_json::json;
use structopt::StructOpt;
use ya_client::activity::ActivityProviderApi;
use ya_client::model::payment::{DebitNote, Invoice, NewDebitNote, NewInvoice};
use ya_client::model::payment::{DebitNoteEvent, DebitNoteEventType, InvoiceEventType};
use ya_client::payment::PaymentApi;

use ya_std_utils::LogErr;
use ya_utils_actix::actix_handler::ResultTypeGetter;
use ya_utils_actix::actix_signal::{SignalSlot, Subscribe};
use ya_utils_actix::deadline_checker::{
    DeadlineChecker, DeadlineElapsed, StopTracking, StopTrackingCategory, TrackDeadline,
};
use ya_utils_actix::{actix_signal_handler, forward_actix_handler};

use crate::execution::{ActivityDestroyed, CreateActivity};
use crate::interval::RelativeInterval;
use crate::market::provider_market::NewAgreement;
use crate::market::termination_reason::BreakReason;
use crate::tasks::{AgreementBroken, AgreementClosed, BreakAgreement};

use super::agreement::{compute_cost, ActivityPayment, AgreementPayment, CostInfo};
use super::model::PaymentModel;

// =========================================== //
// Internal messages
// =========================================== //

/// Checks activity usage counters and updates service
/// cost. Sends debit note to requestor.
#[derive(Message)]
#[rtype(result = "Result<()>")]
pub struct UpdateCost {
    pub invoice_info: DebitNoteInfo,
    pub interval_ctx: RelativeInterval,
}

/// Changes activity state to Finalized and computes final cost.
/// Sent by ActivityDestroyed handler after last debit note was sent to Requestor.
#[derive(Message, Clone)]
#[rtype("()")]
pub struct FinalizeActivity {
    pub debit_info: DebitNoteInfo,
    pub cost_summary: CostInfo,
}

/// Message for issuing an invoice. Sent after agreement is closed.
#[derive(Message, Clone)]
#[rtype(result = "Result<Invoice>")]
struct IssueInvoice {
    costs_summary: CostsSummary,
    payment_timeout: Option<chrono::Duration>,
}

/// Message for sending invoice to the requestor. Sent after invoice is issued.
#[derive(Message, Clone)]
#[rtype(result = "Result<()>")]
struct SendInvoice {
    invoice_id: String,
}

/// Message sent when invoice is accepted.
#[derive(Message, Clone)]
#[rtype(result = "Result<()>")]
struct InvoiceAccepted {
    pub invoice_id: String,
}

/// Message sent when invoice is settled (fully paid).
#[derive(Message, Clone)]
#[rtype(result = "Result<()>")]
struct InvoiceSettled {
    pub invoice_id: String,
}

/// Gets costs summary for agreement.
#[derive(Message, Clone)]
#[rtype(result = "Result<CostsSummary>")]
struct GetAgreementSummary {
    pub agreement_id: String,
}

/// Cost summary for agreement.
#[derive(Clone)]
struct CostsSummary {
    pub agreement_id: String,
    pub cost_summary: CostInfo,
    pub activities: Vec<String>,
}

// =========================================== //
// Payments implementation
// =========================================== //

#[derive(Clone)]
pub struct DebitNoteInfo {
    pub agreement_id: String,
    pub activity_id: String,
    pub accept_timeout: Option<chrono::Duration>,
    pub payment_timeout: Option<chrono::Duration>,
}

/// Configuration for Payments actor.
#[derive(StructOpt, Clone, Debug)]
pub struct PaymentsConfig {
    #[structopt(env = "PAYMENT_EVENTS_TIMEOUT", parse(try_from_str = humantime::parse_duration), default_value = "50s")]
    pub get_events_timeout: Duration,
    #[structopt(parse(try_from_str = humantime::parse_duration), default_value = "5s")]
    pub get_events_error_timeout: Duration,
    #[structopt(long, env, parse(try_from_str = humantime::parse_duration), default_value = "5s")]
    pub invoice_reissue_interval: Duration,
    #[structopt(skip = "you-forgot-to-set-session-id")]
    pub session_id: String,
    #[structopt(env = "PAYMENT_DUE_TIMEOUT", parse(try_from_str = humantime::parse_duration), default_value = "24h")]
    pub payment_due_timeout: Duration,
}

/// Yagna APIs and payments information about provider.
struct ProviderCtx {
    activity_api: Arc<ActivityProviderApi>,
    payment_api: Arc<PaymentApi>,
    debit_checker: Addr<DeadlineChecker>,
    payment_checker: Addr<DeadlineChecker>,
    config: PaymentsConfig,
}

/// Computes charges for tasks execution.
/// Sends payments events to Requestor through payment API.
pub struct Payments {
    context: Arc<ProviderCtx>,
    agreements: HashMap<String, AgreementPayment>,

    invoices_to_pay: Vec<Invoice>,
    earnings: BigDecimal,

    break_agreement_signal: SignalSlot<BreakAgreement>,
}

actix_signal_handler!(Payments, BreakAgreement, break_agreement_signal);

impl Payments {
    pub fn new(
        activity_api: ActivityProviderApi,
        payment_api: PaymentApi,
        config: PaymentsConfig,
    ) -> Payments {
        let provider_ctx = ProviderCtx {
            activity_api: Arc::new(activity_api),
            payment_api: Arc::new(payment_api),
            debit_checker: DeadlineChecker::new().start(),
            payment_checker: DeadlineChecker::new().start(),
            config,
        };

        Payments {
            agreements: HashMap::new(),
            context: Arc::new(provider_ctx),
            invoices_to_pay: vec![],
            earnings: BigDecimal::zero(),
            break_agreement_signal: SignalSlot::<BreakAgreement>::new(),
        }
    }

    pub fn on_signed_agreement(
        &mut self,
        msg: NewAgreement,
        _ctx: &mut Context<Self>,
    ) -> Result<()> {
        log::info!(
            "Payments got signed agreement [{}]. Waiting for activities creation...",
            &msg.agreement.agreement_id
        );

        match AgreementPayment::new(&msg.agreement) {
            Ok(agreement) => {
                self.agreements
                    .insert(msg.agreement.agreement_id.clone(), agreement);
                Ok(())
            }
            Err(error) => {
                //TODO: What should we do? Maybe terminate agreement?
                log::error!(
                    "Failed to create payment model for agreement [{}]. Error: {}",
                    &msg.agreement.agreement_id,
                    error
                );
                Err(error)
            }
        }
    }
}

async fn send_debit_note(
    provider_context: Arc<ProviderCtx>,
    debit_note_info: DebitNoteInfo,
    cost_info: CostInfo,
    last_payable_debit_note: DateTime<Utc>,
) -> Result<DebitNote> {
    let payment_due_date = if provider_context.config.payment_due_timeout.is_zero() {
        None
    } else {
        Some(Utc::now() + chrono::Duration::from_std(provider_context.config.payment_due_timeout)?)
    };

    let debit_note = NewDebitNote {
        activity_id: debit_note_info.activity_id.clone(),
        total_amount_due: cost_info.cost,
        usage_counter_vector: Some(json!(cost_info.usage)),
        payment_due_date,
    };

    log::debug!(
        "Creating debit note {}.",
        serde_json::to_string(&debit_note)?
    );

    let payment_api = &provider_context.payment_api;
    let debit_note = payment_api
        .issue_debit_note(&debit_note)
        .await
        .map_err(|error| {
            anyhow!(
                "Failed to issue debit note for activity [{}]. {}",
                &debit_note_info.activity_id,
                error
            )
        })?;

    // Start deadline tracking before actually sending
    // debit_note, because debit note events can arrive
    // before debit_note.send() call actually returns.
    if let Some(deadline) = debit_note_info
        .accept_timeout
        .map(|timeout| Utc::now() + timeout)
    {
        provider_context
            .debit_checker
            .send(TrackDeadline {
                category: debit_note.agreement_id.clone(),
                deadline,
                id: note_accept_id(&debit_note.debit_note_id),
            })
            .await?;
    }
    if let Some(deadline) = debit_note.payment_due_date {
        provider_context
            .payment_checker
            .send(TrackDeadline {
                category: debit_note.agreement_id.clone(),
                deadline,
                id: note_payment_id(&debit_note.debit_note_id),
            })
            .await?;
    }

    log::debug!(
        "Sending debit note [{}] for activity [{}].",
        &debit_note.debit_note_id,
        &debit_note_info.activity_id
    );
    let send_result = payment_api
        .send_debit_note(&debit_note.debit_note_id)
        .await
        .map_err(|error| {
            anyhow!(
                "Failed to send debit note [{}] for activity [{}]. {}",
                &debit_note.debit_note_id,
                &debit_note_info.activity_id,
                error
            )
        });
    if send_result.is_err() {
        let _ = provider_context
            .debit_checker
            .send(StopTracking {
                id: note_accept_id(&debit_note.debit_note_id),
                category: Some(debit_note.agreement_id.clone()),
            })
            .await;
        let _ = provider_context
            .payment_checker
            .send(StopTracking {
                id: note_payment_id(&debit_note.debit_note_id),
                category: Some(debit_note.agreement_id.clone()),
            })
            .await;
    }
    send_result?;

    log::info!(
        "Debit note [{}] for activity [{}] sent with due date: {:?}.",
        &debit_note.debit_note_id,
        &debit_note_info.activity_id,
        &debit_note.payment_due_date
    );

    Ok(debit_note)
}

async fn check_invoice_events(provider_ctx: Arc<ProviderCtx>, payments_addr: Addr<Payments>) -> () {
    let config = &provider_ctx.config;
    let timeout = config.get_events_timeout.clone();
    let error_timeout = config.get_events_error_timeout.clone();
    let mut after_timestamp = Utc::now();

    loop {
        let events = match provider_ctx
            .payment_api
            .get_invoice_events(
                Some(&after_timestamp),
                Some(timeout),
                None,
                Some(config.session_id.clone()),
            )
            .await
        {
            Ok(events) => events,
            Err(e) => {
                log::error!("Can't query invoice events: {}", e);
                tokio::time::delay_for(error_timeout).await;
                vec![]
            }
        };

        for event in events {
            let invoice_id = event.invoice_id;
            match event.event_type {
                InvoiceEventType::InvoiceAcceptedEvent => {
                    log::info!("Invoice [{}] accepted by requestor.", invoice_id);
                    payments_addr.do_send(InvoiceAccepted { invoice_id })
                }
                InvoiceEventType::InvoiceSettledEvent => {
                    log::info!("Invoice [{}] settled by requestor.", invoice_id);
                    payments_addr.do_send(InvoiceSettled { invoice_id })
                }
                // InvoiceEventType::InvoiceRejectedEvent {} => {
                //     log::warn!("Invoice [{}] rejected by requestor.", invoice_id)
                //     // TODO: Send signal to other provider's modules to react to this situation.
                //     //       Probably we don't want to cooperate with this Requestor anymore.
                // }
                _ => log::warn!("Unexpected event received: {:?}", event.event_type),
            }
            after_timestamp = event.event_date;
        }
    }
}

async fn check_debit_notes_events(
    provider_ctx: Arc<ProviderCtx>,
    provider_signal: SignalSlot<BreakAgreement>,
) {
    let config = &provider_ctx.config;
    let timeout = config.get_events_timeout;
    let error_timeout = config.get_events_error_timeout;
    let mut lather_than = Utc::now();

    loop {
        match provider_ctx
            .payment_api
            .get_debit_note_events(
                Some(&lather_than),
                Some(timeout),
                None,
                Some(config.session_id.clone()),
            )
            .await
        {
            Ok(events) => {
                for event in events {
                    lather_than = event.event_date;
                    handle_debit_note_event(event, &provider_ctx, &provider_signal).await;
                }
            }
            Err(e) => {
                log::error!("Can't query debit note events: {}", e);
                tokio::time::delay_for(error_timeout).await;
            }
        };
    }
}

async fn handle_debit_note_event(
    event: DebitNoteEvent,
    provider_ctx: &Arc<ProviderCtx>,
    provider_signal: &SignalSlot<BreakAgreement>,
) {
    match &event.event_type {
        DebitNoteEventType::DebitNoteAcceptedEvent => provider_ctx
            .debit_checker
            .send(StopTracking {
                id: note_accept_id(&event.debit_note_id),
                category: None,
            })
            .await
            .map(|_| log::debug!("DebitNote [{}] accepted.", event.debit_note_id))
            .map_err(|_| {
                log::warn!(
                    "Failed to notify about accepted DebitNote {}",
                    event.debit_note_id
                )
            })
            .ok(),
        DebitNoteEventType::DebitNoteSettledEvent => provider_ctx
            .payment_checker
            .send(StopTracking {
                id: note_payment_id(&event.debit_note_id),
                category: None,
            })
            .await
            .map(|_| log::debug!("DebitNote [{}] paid.", event.debit_note_id))
            .map_err(|_| {
                log::warn!(
                    "Failed to notify about a paid DebitNote {}",
                    event.debit_note_id
                )
            })
            .ok(),
        DebitNoteEventType::DebitNoteCancelledEvent
        | DebitNoteEventType::DebitNoteRejectedEvent { .. } => {
            let debit_note = match provider_ctx
                .payment_api
                .get_debit_note(&event.debit_note_id)
                .await
            {
                Ok(note) => note,
                Err(err) => {
                    log::error!(
                        "Failed to break agreement for DebitNote [{}] because the DebitNote cannot be retrieved: {}",
                        event.debit_note_id,
                        err
                    );
                    return;
                }
            };

            provider_ctx.debit_checker.do_send(StopTrackingCategory {
                category: debit_note.agreement_id.clone(),
            });
            provider_ctx.payment_checker.do_send(StopTrackingCategory {
                category: debit_note.agreement_id.clone(),
            });

            let reason = BreakReason::try_from(event.event_type.clone()).unwrap();
            provider_signal
                .send_signal(BreakAgreement {
                    agreement_id: debit_note.agreement_id.clone(),
                    reason: reason.clone(),
                })
                .log_err_msg(&format!(
                    "Failed to send BreakAgreement for [{}] with reason: {}",
                    debit_note.agreement_id,
                    reason.to_string()
                ))
                .ok()
        }
        _ => None,
    };
}

async fn compute_cost_and_send_debit_note(
    provider_context: Arc<ProviderCtx>,
    payment_model: Arc<dyn PaymentModel>,
    last_payable_debit_node: DateTime<Utc>,
    invoice_info: &DebitNoteInfo,
) -> Result<(DebitNote, CostInfo)> {
    let cost_info = compute_cost(
        payment_model.clone(),
        provider_context.activity_api.clone(),
        invoice_info.activity_id.clone(),
    )
    .await?;

    log::info!(
        "Updating cost for activity [{}]: {}, usage {:?}.",
        &invoice_info.activity_id,
        &cost_info.cost,
        &cost_info.usage
    );

    let debit_note = send_debit_note(
        provider_context,
        invoice_info.clone(),
        cost_info.clone(),
        last_payable_debit_node,
    )
    .await?;
    Ok((debit_note, cost_info))
}

forward_actix_handler!(Payments, NewAgreement, on_signed_agreement);

impl Handler<CreateActivity> for Payments {
    type Result = anyhow::Result<()>;

    fn handle(&mut self, msg: CreateActivity, ctx: &mut Context<Self>) -> Self::Result {
        let agreement = self
            .agreements
            .get_mut(&msg.agreement_id)
            .ok_or(anyhow!(
                "Agreement [{}] wasn't registered.",
                &msg.agreement_id
            ))
            .log_warn_msg("[ActivityCreated]")?;

        log::info!(
            "Payments - activity [{}] created. Start computing costs.",
            &msg.activity_id
        );

        // Add activity to list and send debit note after a delay.
        agreement.add_created_activity(&msg.activity_id);

        // Start a new DebitNote chain for this activity.
        let invoice_info = DebitNoteInfo {
            agreement_id: msg.agreement_id.clone(),
            activity_id: msg.activity_id.clone(),
            accept_timeout: agreement.accept_timeout,
            payment_timeout: agreement.payment_timeout,
        };

        let mut interval_ctx =
            RelativeInterval::new(agreement.approved_ts, agreement.update_interval)?;
        let delay = interval_ctx.advance()?;

        ctx.notify_later(
            UpdateCost {
                invoice_info,
                interval_ctx,
            },
            delay,
        );

        Ok(())
    }
}

impl Handler<ActivityDestroyed> for Payments {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: ActivityDestroyed, ctx: &mut Context<Self>) -> Self::Result {
        let agreement = match self
            .agreements
            .get_mut(&msg.agreement_id)
            .ok_or_else(|| {
                anyhow!(
                    "Can't find activity [{}] and agreement [{}].",
                    &msg.activity_id,
                    &msg.agreement_id
                )
            })
            .log_warn_msg("[ActivityDestroyed]")
        {
            Ok(agreement) => agreement,
            Err(e) => return ActorResponse::reply(Err(e)),
        };

        agreement.activity_destroyed(&msg.activity_id).unwrap();

        let payment_model = agreement.payment_model.clone();
        let last_payable_debit_node = match agreement.payment_timeout {
            // Ensure that last debit note is always payable, by
            Some(timeout) => Utc::now() - timeout,
            // Without payment timeout the last_payable_debit_node is ignored.
            None => agreement.last_payable_debit_note,
        };
        let provider_context = self.context.clone();
        let address = ctx.address();
        let debit_note_info = DebitNoteInfo {
            activity_id: msg.activity_id.clone(),
            agreement_id: msg.agreement_id.clone(),
            accept_timeout: agreement.accept_timeout,
            payment_timeout: agreement.payment_timeout,
        };

        let future = async move {
            // Computing last DebitNote can't fail, so we must repeat it until
            // it reaches Requestor. DebitNote itself is not important so much, but
            // we must ensure that we send FinalizeActivity and Invoice in consequence.

            let mut repeats = get_backoff();
            let (debit_note, cost_info) = loop {
                match compute_cost_and_send_debit_note(
                    provider_context.clone(),
                    payment_model.clone(),
                    last_payable_debit_node,
                    &debit_note_info,
                )
                .await
                {
                    Ok(debit_note) => break debit_note,
                    Err(e) => {
                        let delay = repeats.next_backoff().unwrap_or(repeats.current_interval);
                        log::warn!("Error sending debit note: {} Retry in {:#?}.", e, delay);
                        tokio::time::delay_for(delay).await
                    }
                }
            };

            log::info!(
                "Final cost for activity [{}]: {}.",
                &debit_note_info.activity_id,
                &debit_note.total_amount_due
            );

            let msg = FinalizeActivity {
                cost_summary: cost_info,
                debit_info: debit_note_info,
            };

            let _ = address.send(msg).await;
        }
        .into_actor(self);

        return ActorResponse::r#async(future.map(|_, _, _| Ok(())));
    }
}

impl Handler<UpdateCost> for Payments {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, mut msg: UpdateCost, _ctx: &mut Context<Self>) -> Self::Result {
        let agreement = match self
            .agreements
            .get(&msg.invoice_info.agreement_id)
            .ok_or(anyhow!(
                "Not my activity - agreement [{}].",
                &msg.invoice_info.agreement_id
            ))
            .log_warn_msg("[UpdateCost]")
        {
            Ok(agreement) => agreement,
            Err(e) => return ActorResponse::reply(Err(e)),
        };

        return match agreement.activities.get(&msg.invoice_info.activity_id) {
            Some(ActivityPayment::Running { .. }) => {
                let last_debit_note = agreement.last_send_debit_note;
                let last_payable_debit_node = agreement.last_payable_debit_note;
                let accept_timeout = agreement.accept_timeout;
                let invoice_info = msg.invoice_info.clone();
                let payment_model = agreement.payment_model.clone();
                let context = self.context.clone();

                let debit_note_future = async move {
                    let (debit_note, _cost) = compute_cost_and_send_debit_note(
                        context.clone(),
                        payment_model.clone(),
                        last_payable_debit_node,
                        &invoice_info,
                    )
                        .await
                        .log_err()?;
                    Ok(debit_note)
                }
                    .into_actor(self)
                    .map(move |result: Result<_, anyhow::Error>, myself, ctx| {
                        // We break Agreement, if we weren't able to send any DebitNote lately.
                        match result {
                            Err(_) => {
                                if accept_timeout.is_some() && Utc::now() > last_debit_note + accept_timeout.unwrap() {
                                    myself.break_agreement_signal
                                        .send_signal(BreakAgreement {
                                            agreement_id: msg.invoice_info.agreement_id.clone(),
                                            reason: BreakReason::RequestorUnreachable(accept_timeout.unwrap()),
                                        })
                                        .log_err_msg(&format!(
                                            "Failed to send BreakAgreement for [{}], when Requestor is unreachable.",
                                            msg.invoice_info.agreement_id
                                        ))
                                        .ok();
                                }
                            },
                            Ok(debit_note) => {
                                myself.agreements
                                    .get_mut(&msg.invoice_info.agreement_id)
                                    // Payment due date is always set _before_ sending the DebitNote.
                                    // The following synchronises the acceptance timeout check.
                                    .map(|agreement| {
                                        agreement.last_send_debit_note = debit_note.timestamp;
                                        if debit_note.payment_due_date.is_some() {
                                            agreement.last_payable_debit_note = debit_note.timestamp
                                        }
                                    });
                            }
                        }

                        // A note regarding short debit note intervals:
                        // If sending a DebitNote note takes longer than the interval duration,
                        // the next DebitNote will be scheduled at the next possible interval,
                        // relative to agreement approval date, and based on current time.
                        let delay = msg.interval_ctx.advance()?;

                        // Don't bother, if previous debit note was sent successfully or not.
                        // Schedule UpdateCost for later.
                        ctx.notify_later(msg, delay);

                        Ok(())
                    });
                ActorResponse::r#async(debit_note_future)
            }
            Some(_) => {
                // Activity is not running anymore. We don't send here new UpdateCost
                // message, what stops further updates.
                log::info!(
                    "Stopped sending debit notes, because activity [{}] was destroyed.",
                    &msg.invoice_info.activity_id
                );
                ActorResponse::reply(Ok(()))
            }
            None => ActorResponse::reply(Err(anyhow!(
                "We shouldn't be here. Activity [{}], not found.",
                &msg.invoice_info.activity_id
            ))),
        };
    }
}

impl Handler<FinalizeActivity> for Payments {
    type Result = <FinalizeActivity as Message>::Result;

    fn handle(&mut self, msg: FinalizeActivity, _ctx: &mut Context<Self>) -> Self::Result {
        if let Ok(agreement) = self
            .agreements
            .get_mut(&msg.debit_info.agreement_id)
            .ok_or(anyhow!(
                "Not my activity - agreement [{}].",
                &msg.debit_info.agreement_id
            ))
            .log_warn_msg("[FinalizeActivity]")
        {
            agreement
                .finish_activity(&msg.debit_info.activity_id, msg.cost_summary)
                .log_err_msg("Finalizing activity failed")
                .ok();
            log::info!("Activity [{}] finished.", &msg.debit_info.activity_id)
        }
    }
}

/// Computes costs for all activities and sends invoice to Requestor.
impl Handler<AgreementClosed> for Payments {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: AgreementClosed, ctx: &mut Context<Self>) -> Self::Result {
        if let Some(agreement) = self.agreements.get_mut(&msg.agreement_id) {
            log::info!(
                "Payments - agreement [{}] closed. Computing cost summary...",
                &msg.agreement_id
            );

            let activities_watch = agreement.activities_watch.clone();
            let agreement_id = msg.agreement_id.clone();
            let payment_timeout = agreement.payment_timeout;
            let myself = ctx.address().clone();
            let ctx = self.context.clone();

            let future = async move {
                let stop_tracking = StopTrackingCategory {
                    category: agreement_id.clone(),
                };
                let _ = ctx.debit_checker.send(stop_tracking.clone()).await;
                let _ = ctx.payment_checker.send(stop_tracking).await;

                activities_watch.wait_for_finish().await;

                let costs_summary = myself.send(GetAgreementSummary { agreement_id }).await??;
                let invoice = myself
                    .send(IssueInvoice {
                        costs_summary,
                        payment_timeout,
                    })
                    .await??;
                // We do not want to wait for sending Invoice, as we are eager to start new
                // negotiations. Waiting for invoice to be sent to Requestor could result in
                // hanging Provider waiting for Requestor to appear in the net and receive the Invoice
                let invoice_id = invoice.invoice_id;
                myself.do_send(SendInvoice { invoice_id });

                Ok(())
            }
            .into_actor(self);

            return ActorResponse::r#async(future);
        }

        return ActorResponse::reply(Err(anyhow!("Not my agreement {}.", &msg.agreement_id)));
    }
}

impl Handler<IssueInvoice> for Payments {
    type Result = ResponseFuture<Result<Invoice, Error>>;

    fn handle(&mut self, msg: IssueInvoice, _ctx: &mut Context<Self>) -> Self::Result {
        let agreement_id = msg.costs_summary.agreement_id;
        let activity_ids = msg.costs_summary.activities;
        let cost_info = msg.costs_summary.cost_summary;
        log::info!(
            "Final cost for agreement [{}]: {}, usage {:?}.",
            agreement_id,
            cost_info.cost,
            cost_info.usage,
        );
        let payment_due_date = Utc::now()
            + chrono::Duration::from_std(self.context.config.payment_due_timeout)
                .unwrap_or_else(|_| chrono::Duration::days(1));

        let payment_timeout = msg
            .payment_timeout
            .unwrap_or_else(|| chrono::Duration::days(1));
        let invoice = NewInvoice {
            agreement_id,
            activity_ids: Some(activity_ids),
            amount: cost_info.cost,
            payment_due_date,
        };

        let provider_ctx = self.context.clone();
        async move {
            log::debug!("Issuing invoice {}.", serde_json::to_string(&invoice)?);

            loop {
                match provider_ctx.payment_api.issue_invoice(&invoice).await {
                    Ok(invoice) => {
                        log::info!("Invoice [{}] issued.", invoice.invoice_id);
                        return Ok(invoice);
                    }
                    Err(e) => {
                        let interval = provider_ctx.config.invoice_reissue_interval;
                        log::error!("Error issuing invoice: {} Retry in {:#?}.", e, interval);
                        tokio::time::delay_for(interval).await
                    }
                }
            }
        }
        .boxed_local()
    }
}

impl Handler<SendInvoice> for Payments {
    type Result = ResponseFuture<Result<(), Error>>;

    fn handle(&mut self, msg: SendInvoice, _ctx: &mut Context<Self>) -> Self::Result {
        let provider_ctx = self.context.clone();
        async move {
            log::info!("Sending invoice [{}] to requestor...", msg.invoice_id);

            let mut repeats = get_backoff();
            loop {
                match provider_ctx.payment_api.send_invoice(&msg.invoice_id).await {
                    Ok(_) => {
                        log::info!("Invoice [{}] sent.", msg.invoice_id);
                        return Ok(());
                    }
                    Err(e) => {
                        let delay = repeats.next_backoff().unwrap_or(repeats.current_interval);
                        log::warn!("Error sending invoice: {} Retry in {:#?}.", e, delay);
                        tokio::time::delay_for(delay).await
                    }
                }
            }
        }
        .boxed_local()
    }
}

/// If Agreement was broken, we should behave like it was closed.
impl Handler<AgreementBroken> for Payments {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: AgreementBroken, ctx: &mut Context<Self>) -> Self::Result {
        if !self.agreements.contains_key(&msg.agreement_id) {
            log::warn!(
                "Payments - agreement [{}] does not exist -- not broken.",
                &msg.agreement_id
            );
            return ActorResponse::reply(Ok(()));
        }

        let address = ctx.address().clone();
        let future = async move {
            let msg = AgreementClosed {
                agreement_id: msg.agreement_id,
                send_terminate: false,
            };
            Ok(address.send(msg).await??)
        };

        return ActorResponse::r#async(future.into_actor(self));
    }
}

impl Handler<InvoiceAccepted> for Payments {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: InvoiceAccepted, _ctx: &mut Context<Self>) -> Self::Result {
        let provider_ctx = self.context.clone();

        let future = async move { provider_ctx.payment_api.get_invoice(&msg.invoice_id).await }
            .into_actor(self)
            .map(|result, myself, _ctx| match result {
                Ok(invoice) => {
                    myself.invoices_to_pay.push(invoice);
                    Ok(())
                }
                Err(e) => Err(anyhow!("Cannot get invoice: {}", e)),
            });

        return ActorResponse::r#async(future);
    }
}

impl Handler<InvoiceSettled> for Payments {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: InvoiceSettled, _ctx: &mut Context<Self>) -> Self::Result {
        let provider_ctx = self.context.clone();

        let future = async move { provider_ctx.payment_api.get_invoice(&msg.invoice_id).await }
            .into_actor(self)
            .map(|result, myself, _ctx| match result {
                Ok(invoice) => {
                    log::info!(
                        "Invoice [{}] for agreement [{}] was paid. Amount: {}.",
                        invoice.invoice_id,
                        invoice.agreement_id,
                        invoice.amount
                    );
                    myself.agreements.remove(&invoice.agreement_id);
                    myself
                        .invoices_to_pay
                        .retain(|x| x.invoice_id != invoice.invoice_id);
                    myself.earnings += invoice.amount;
                    log::info!("Current earnings: {}", myself.earnings);
                    Ok(())
                }
                Err(e) => Err(anyhow!("Cannot get invoice: {}", e)),
            });

        ActorResponse::r#async(future)
    }
}

impl Handler<DeadlineElapsed> for Payments {
    type Result = ();

    fn handle(&mut self, msg: DeadlineElapsed, _ctx: &mut Context<Self>) -> Self::Result {
        let agreement = match self.agreements.get_mut(&msg.category) {
            Some(agreement) => {
                // If at least one deadline elapses, we don't want to generate any
                // new unnecessary events.
                if agreement.deadline_elapsed {
                    return;
                }
                agreement
            }
            None => {
                log::error!(
                    "DeadlineElapsed for not existing Agreement [{}].",
                    msg.category
                );
                return;
            }
        };
        let reason = if msg.id.starts_with(ACCEPT_PREFIX) {
            match agreement.accept_timeout {
                Some(timeout) => {
                    log::warn!(
                        "Deadline {} elapsed for accepting DebitNote [{}] for Agreement [{}]",
                        msg.deadline,
                        msg.id,
                        msg.category,
                    );

                    agreement.deadline_elapsed = true;
                    BreakReason::DebitNotesDeadline(timeout)
                }
                None => return,
            }
        } else if msg.id.starts_with(PAYMENT_PREFIX) {
            match agreement.payment_timeout {
                Some(timeout) => {
                    log::warn!(
                        "Deadline {} elapsed for DebitNote [{}] payment for Agreement [{}]",
                        msg.deadline,
                        msg.id,
                        msg.category,
                    );
                    BreakReason::DebitNoteNotPaid(timeout)
                }
                None => return,
            }
        } else {
            log::error!(
                "DeadlineElapsed for Agreement [{}] is of an unknown type",
                msg.category
            );
            return;
        };

        self.break_agreement_signal
            .send_signal(BreakAgreement {
                agreement_id: msg.category.clone(),
                reason: reason.clone(),
            })
            .log_err_msg(&format!(
                "Failed to send BreakAgreement for [{}] with reason: {}",
                msg.category,
                reason.to_string(),
            ))
            .ok();
    }
}

impl Handler<GetAgreementSummary> for Payments {
    type Result = anyhow::Result<CostsSummary>;

    fn handle(&mut self, msg: GetAgreementSummary, _ctx: &mut Context<Self>) -> Self::Result {
        if let Some(agreement) = self.agreements.get_mut(&msg.agreement_id) {
            let cost_summary = agreement.cost_summary();
            let activities = agreement.list_activities();

            let summary = CostsSummary {
                agreement_id: msg.agreement_id,
                cost_summary,
                activities,
            };
            return Ok(summary);
        }
        return Err(anyhow!("Not my agreement {}.", &msg.agreement_id));
    }
}

impl Actor for Payments {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
        // Start checking incoming payments.
        let provider_signal = self.break_agreement_signal.clone();
        let provider_ctx = self.context.clone();
        let payment_addr = ctx.address();

        Arbiter::spawn(check_invoice_events(
            provider_ctx.clone(),
            payment_addr.clone(),
        ));
        Arbiter::spawn(async move {
            for checker in vec![&provider_ctx.debit_checker, &provider_ctx.payment_checker] {
                let _ = checker
                    .send(Subscribe(payment_addr.clone().recipient()))
                    .await
                    .map_err(|_| log::error!("Subscribing to DebitNotes deadline checker failed."));
            }
            check_debit_notes_events(provider_ctx, provider_signal).await;
        });
    }
}

fn get_backoff() -> backoff::ExponentialBackoff {
    backoff::ExponentialBackoff {
        current_interval: std::time::Duration::from_secs(3),
        initial_interval: std::time::Duration::from_secs(3),
        multiplier: 1.5f64,
        max_interval: std::time::Duration::from_secs(5 * 60 * 60),
        max_elapsed_time: Some(std::time::Duration::from_secs(u64::MAX)),
        ..Default::default()
    }
}

const ACCEPT_PREFIX: &'static str = "debit-";
const PAYMENT_PREFIX: &'static str = "payment-";

#[inline(always)]
fn note_accept_id(id: impl AsRef<str>) -> String {
    format!("{}{}", ACCEPT_PREFIX, id.as_ref())
}

#[inline(always)]
fn note_payment_id(id: impl AsRef<str>) -> String {
    format!("{}{}", PAYMENT_PREFIX, id.as_ref())
}
