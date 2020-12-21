use actix::prelude::*;
use anyhow::{anyhow, Error, Result};
use bigdecimal::{BigDecimal, Zero};
use chrono::Utc;
use log;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use super::agreement::{compute_cost, ActivityPayment, AgreementPayment, CostInfo};
use super::model::PaymentModel;
use crate::execution::{ActivityCreated, ActivityDestroyed};
use crate::market::provider_market::AgreementApproved;
use crate::tasks::{AgreementBroken, AgreementClosed};

use ya_client::activity::ActivityProviderApi;
use ya_client::model::payment::{DebitNote, Invoice, NewDebitNote, NewInvoice};
use ya_client::payment::PaymentApi;
use ya_client_model::payment::InvoiceEventType;
use ya_utils_actix::actix_handler::ResultTypeGetter;
use ya_utils_actix::forward_actix_handler;

// =========================================== //
// Internal messages
// =========================================== //

/// Checks activity usage counters and updates service
/// cost. Sends debit note to requestor.
#[derive(Message, Clone)]
#[rtype(result = "Result<()>")]
pub struct UpdateCost {
    pub invoice_info: DebitNoteInfo,
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
}

/// Yagna APIs and payments information about provider.
struct ProviderCtx {
    activity_api: Arc<ActivityProviderApi>,
    payment_api: Arc<PaymentApi>,

    get_events_timeout: Option<Duration>,
    get_events_error_timeout: Duration,
    invoice_reissue_interval: Duration,
    invoice_resend_interval: Duration,
}

/// Computes charges for tasks execution.
/// Sends payments events to Requestor through payment API.
pub struct Payments {
    context: Arc<ProviderCtx>,
    agreements: HashMap<String, AgreementPayment>,

    invoices_to_pay: Vec<Invoice>,
    earnings: BigDecimal,
}

impl Payments {
    pub fn new(activity_api: ActivityProviderApi, payment_api: PaymentApi) -> Payments {
        let provider_ctx = ProviderCtx {
            activity_api: Arc::new(activity_api),
            payment_api: Arc::new(payment_api),
            get_events_timeout: Some(Duration::from_secs(60)),
            get_events_error_timeout: Duration::from_secs(5),
            invoice_reissue_interval: Duration::from_secs(5),
            invoice_resend_interval: Duration::from_secs(50),
        };

        Payments {
            agreements: HashMap::new(),
            context: Arc::new(provider_ctx),
            invoices_to_pay: vec![],
            earnings: BigDecimal::zero(),
        }
    }

    pub fn on_signed_agreement(
        &mut self,
        msg: AgreementApproved,
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
) -> Result<DebitNote> {
    let debit_note = NewDebitNote {
        activity_id: debit_note_info.activity_id.clone(),
        total_amount_due: cost_info.cost,
        usage_counter_vector: Some(json!(cost_info.usage)),
        payment_due_date: None,
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

    log::debug!(
        "Sending debit note [{}] for activity [{}].",
        &debit_note.debit_note_id,
        &debit_note_info.activity_id
    );
    payment_api
        .send_debit_note(&debit_note.debit_note_id)
        .await
        .map_err(|error| {
            anyhow!(
                "Failed to send debit note [{}] for activity [{}]. {}",
                &debit_note.debit_note_id,
                &debit_note_info.activity_id,
                error
            )
        })?;

    log::info!(
        "Debit note [{}] for activity [{}] sent.",
        &debit_note.debit_note_id,
        &debit_note_info.activity_id
    );

    Ok(debit_note)
}

async fn check_invoice_events(provider_ctx: Arc<ProviderCtx>, payments_addr: Addr<Payments>) -> () {
    let timeout = provider_ctx.get_events_timeout.clone();
    let error_timeout = provider_ctx.get_events_error_timeout.clone();
    let mut after_timestamp = Utc::now();

    loop {
        let events = match provider_ctx
            .payment_api
            .get_invoice_events(Some(&after_timestamp), timeout, None, None)
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

async fn compute_cost_and_send_debit_note(
    provider_context: Arc<ProviderCtx>,
    payment_model: Arc<dyn PaymentModel>,
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

    let debit_note =
        send_debit_note(provider_context, invoice_info.clone(), cost_info.clone()).await?;
    Ok((debit_note, cost_info))
}

forward_actix_handler!(Payments, AgreementApproved, on_signed_agreement);

impl Handler<ActivityCreated> for Payments {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: ActivityCreated, ctx: &mut Context<Self>) -> Self::Result {
        if let Some(agreement) = self.agreements.get_mut(&msg.agreement_id) {
            log::info!(
                "Payments - activity [{}] created. Start computing costs.",
                &msg.activity_id
            );

            // Sending UpdateCost with last_debit_note: None will start new
            // DebitNotes chain for this activity.
            let msg = UpdateCost {
                invoice_info: DebitNoteInfo {
                    agreement_id: msg.agreement_id.clone(),
                    activity_id: msg.activity_id.clone(),
                },
            };

            // Add activity to list and send debit note after update_interval.
            agreement.add_created_activity(&msg.invoice_info.activity_id);
            ctx.notify_later(msg, agreement.update_interval);

            ActorResponse::reply(Ok(()))
        } else {
            let error = format!("Agreement [{}] wasn't registered.", &msg.agreement_id);
            log::warn!("{}", error);
            ActorResponse::reply(Err(anyhow!(error)))
        }
    }
}

impl Handler<ActivityDestroyed> for Payments {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: ActivityDestroyed, ctx: &mut Context<Self>) -> Self::Result {
        let agreement = match self.agreements.get_mut(&msg.agreement_id) {
            Some(agreement) => agreement,
            None => {
                log::warn!(
                    "Can't find activity [{}] and agreement [{}].",
                    &msg.activity_id,
                    &msg.agreement_id
                );
                return ActorResponse::reply(Err(anyhow!("")));
            }
        };

        agreement.activity_destroyed(&msg.activity_id).unwrap();

        let payment_model = agreement.payment_model.clone();
        let provider_context = self.context.clone();
        let address = ctx.address();
        let debit_note_info = DebitNoteInfo {
            activity_id: msg.activity_id.clone(),
            agreement_id: msg.agreement_id.clone(),
        };

        let future = async move {
            // Computing last DebitNote can't fail, so we must repeat it until
            // it reaches Requestor. DebitNote itself is not important so much, but
            // we must ensure that we send FinalizeActivity and Invoice in consequence.
            let (debit_note, cost_info) = loop {
                match compute_cost_and_send_debit_note(
                    provider_context.clone(),
                    payment_model.clone(),
                    &debit_note_info,
                )
                .await
                {
                    Ok(debit_note) => break debit_note,
                    Err(error) => {
                        let interval = provider_context.invoice_resend_interval;

                        log::error!(
                            "{} Final debit note will be resent after {:#?}.",
                            error,
                            interval
                        );
                        tokio::time::delay_for(interval).await
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

    fn handle(&mut self, msg: UpdateCost, _ctx: &mut Context<Self>) -> Self::Result {
        let agreement = match self.agreements.get(&msg.invoice_info.agreement_id) {
            Some(agreement) => agreement,
            None => {
                let err_msg = format!(
                    "Not my activity - agreement [{}].",
                    &msg.invoice_info.agreement_id
                );
                log::warn!("{}", &err_msg);

                return ActorResponse::reply(Err(anyhow!(err_msg)));
            }
        };

        if let Some(activity) = agreement.activities.get(&msg.invoice_info.activity_id) {
            if let ActivityPayment::Running { .. } = activity {
                let payment_model = agreement.payment_model.clone();
                let context = self.context.clone();
                let invoice_info = msg.invoice_info.clone();
                let update_interval = agreement.update_interval;

                let debit_note_future = async move {
                    let (debit_note, _cost) = compute_cost_and_send_debit_note(
                        context.clone(),
                        payment_model.clone(),
                        &invoice_info,
                    )
                    .await?;
                    Ok(debit_note)
                }
                .into_actor(self)
                .map(move |result: Result<DebitNote, Error>, _, ctx| {
                    if let Err(error) = result {
                        log::error!("{}", error)
                    }
                    // Don't bother, if previous debit note was sent successfully or not.
                    // Schedule UpdateCost for later.
                    ctx.notify_later(msg, update_interval);
                    Ok(())
                });
                return ActorResponse::r#async(debit_note_future);
            } else {
                // Activity is not running anymore. We don't send here new UpdateCost
                // message, what stops further updates.
                log::info!(
                    "Stopped sending debit notes, because activity [{}] was destroyed.",
                    &msg.invoice_info.activity_id
                );
                return ActorResponse::reply(Ok(()));
            }
        }
        return ActorResponse::reply(Err(anyhow!(
            "We shouldn't be here. Activity [{}], not found.",
            &msg.invoice_info.activity_id
        )));
    }
}

impl Handler<FinalizeActivity> for Payments {
    type Result = <FinalizeActivity as Message>::Result;

    fn handle(&mut self, msg: FinalizeActivity, _ctx: &mut Context<Self>) -> Self::Result {
        if let Some(agreement) = self.agreements.get_mut(&msg.debit_info.agreement_id) {
            log::info!("Activity [{}] finished.", &msg.debit_info.activity_id);

            let result = agreement.finish_activity(&msg.debit_info.activity_id, msg.cost_summary);
            if let Err(error) = result {
                log::error!("Finalizing activity failed. Error: {}", error);
            }
        } else {
            log::warn!(
                "Not my activity - agreement [{}].",
                &msg.debit_info.agreement_id
            );
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
            let myself = ctx.address().clone();

            let future = async move {
                activities_watch.wait_for_finish().await;

                let costs_summary = myself.send(GetAgreementSummary { agreement_id }).await??;
                let invoice = myself.send(IssueInvoice { costs_summary }).await??;
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
    type Result = ActorResponse<Self, Invoice, Error>;

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

        let invoice = NewInvoice {
            agreement_id,
            activity_ids: Some(activity_ids),
            amount: cost_info.cost,
            // TODO: Change this date to meaningful value.
            //  Now all our invoices are immediately outdated.
            payment_due_date: Utc::now(),
        };

        let provider_ctx = self.context.clone();
        let future = async move {
            log::debug!("Issuing invoice {}.", serde_json::to_string(&invoice)?);

            loop {
                match provider_ctx.payment_api.issue_invoice(&invoice).await {
                    Ok(invoice) => {
                        log::info!("Invoice [{}] issued.", invoice.invoice_id);
                        return Ok(invoice);
                    }
                    Err(e) => {
                        let interval = provider_ctx.invoice_reissue_interval;
                        log::error!("Error issuing invoice: {} Retry in {:#?}.", e, interval);
                        tokio::time::delay_for(interval).await
                    }
                }
            }
        };

        return ActorResponse::r#async(future.into_actor(self));
    }
}

impl Handler<SendInvoice> for Payments {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: SendInvoice, _ctx: &mut Context<Self>) -> Self::Result {
        let provider_ctx = self.context.clone();
        let future = async move {
            log::info!("Sending invoice [{}] to requestor...", msg.invoice_id);

            loop {
                match provider_ctx.payment_api.send_invoice(&msg.invoice_id).await {
                    Ok(_) => {
                        log::info!("Invoice [{}] sent.", msg.invoice_id);
                        return Ok(());
                    }
                    Err(e) => {
                        let interval = provider_ctx.invoice_resend_interval;
                        log::error!("Error sending invoice: {} Retry in {:#?}.", e, interval);
                        tokio::time::delay_for(interval).await
                    }
                }
            }
        };

        return ActorResponse::r#async(future.into_actor(self));
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

        return ActorResponse::r#async(future);
    }
}

impl Handler<GetAgreementSummary> for Payments {
    type Result = ActorResponse<Self, CostsSummary, Error>;

    fn handle(&mut self, msg: GetAgreementSummary, _ctx: &mut Context<Self>) -> Self::Result {
        if let Some(agreement) = self.agreements.get_mut(&msg.agreement_id) {
            let cost_summary = agreement.cost_summary();
            let activities = agreement.list_activities();

            let summary = CostsSummary {
                agreement_id: msg.agreement_id,
                cost_summary,
                activities,
            };
            return ActorResponse::reply(Ok(summary));
        }
        return ActorResponse::reply(Err(anyhow!("Not my agreement {}.", &msg.agreement_id)));
    }
}

impl Actor for Payments {
    type Context = Context<Self>;

    fn started(&mut self, context: &mut Context<Self>) {
        // Start checking incoming payments.
        let provider_ctx = self.context.clone();
        let payment_addr = context.address();
        Arbiter::spawn(check_invoice_events(provider_ctx, payment_addr));
    }
}
