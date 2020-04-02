use actix::prelude::*;
use anyhow::{anyhow, Error, Result};
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use log;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use super::agreement::{compute_cost, ActivityPayment, AgreementPayment, CostInfo};
use super::model::PaymentModel;
use crate::execution::{ActivityCreated, ActivityDestroyed};
use crate::market::provider_market::AgreementApproved;

use ya_client::activity::ActivityProviderApi;
use ya_client::payment::provider::ProviderApi;
use ya_model::payment::{DebitNote, Invoice, InvoiceStatus, NewDebitNote, NewInvoice, Payment};
use ya_utils_actix::actix_handler::{send_message, ResultTypeGetter};
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
#[rtype(result = "Result<()>")]
pub struct FinalizeActivity {
    pub debit_info: DebitNoteInfo,
    pub cost_summary: CostInfo,
}

/// TODO: We should get this message from external world.
///       Current code assumes, that we have only one activity per agreement.
/// Computes costs for all activities and sends invoice to Requestor.
#[derive(Message, Clone)]
#[rtype(result = "Result<()>")]
pub struct AgreementClosed {
    pub agreement_id: String,
}

/// Checks if requestor accepted Invoice.
#[derive(Message, Clone)]
#[rtype(result = "Result<()>")]
struct CheckInvoiceAcceptance {
    pub invoice_id: String,
}

/// Message for checking if new payments were made by Requestor.
/// Handler will send InvoicesPaid in response for all Payment objects.
#[derive(Message, Clone)]
#[rtype(result = "Result<()>")]
struct CheckInvoicePayments {
    pub since: DateTime<Utc>,
}

/// Message sent when we got payment confirmation for Invoice.
#[derive(Message, Clone)]
#[rtype(result = "Result<()>")]
struct InvoicesPaid {
    pub invoices: Vec<String>,
    pub payment: Payment,
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
    payment_api: Arc<ProviderApi>,

    credit_account: String,

    invoice_paid_check_interval: Duration,
    invoice_accept_check_interval: Duration,
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
    pub fn new(
        activity_api: ActivityProviderApi,
        payment_api: ProviderApi,
        credit_address: &str,
    ) -> Payments {
        log::info!(
            "Payments will be sent to account address {}.",
            credit_address
        );

        let provider_ctx = ProviderCtx {
            activity_api: Arc::new(activity_api),
            payment_api: Arc::new(payment_api),
            credit_account: credit_address.to_string(),
            invoice_paid_check_interval: Duration::from_secs(10),
            invoice_accept_check_interval: Duration::from_secs(10),
            invoice_resend_interval: Duration::from_secs(50),
        };

        Payments {
            agreements: HashMap::new(),
            context: Arc::new(provider_ctx),
            invoices_to_pay: vec![],
            earnings: BigDecimal::from(0.0),
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
        agreement_id: debit_note_info.agreement_id.clone(),
        activity_id: Some(debit_note_info.activity_id.clone()),
        total_amount_due: cost_info.cost,
        usage_counter_vector: Some(json!(cost_info.usage)),
        credit_account_id: provider_context.credit_account.clone(),
        payment_platform: None,
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

async fn send_invoice(
    provider_context: &Arc<ProviderCtx>,
    agreement_id: &str,
    cost_summary: &CostInfo,
    activities: &Vec<String>,
) -> Result<Invoice> {
    log::info!(
        "Final cost for agreement [{}]: {}, usage {:?}.",
        agreement_id,
        &cost_summary.cost,
        &cost_summary.usage
    );

    let invoice = NewInvoice {
        agreement_id: agreement_id.to_string(),
        activity_ids: Some(activities.clone()),
        amount: cost_summary.clone().cost,
        // TODO: This is temporary. In the future we won't need to set these fields.
        usage_counter_vector: Some(json!(cost_summary.usage)),
        credit_account_id: provider_context.credit_account.clone(),
        payment_platform: None,
        // TODO: Change this date to meaningful value.
        //  Now all our invoices are immediately outdated.
        payment_due_date: Utc::now(),
    };

    log::debug!("Creating invoice {}.", serde_json::to_string(&invoice)?);

    let payment_api = &provider_context.payment_api;
    let invoice = payment_api.issue_invoice(&invoice).await.map_err(|error| {
        anyhow!(
            "Failed to issue debit note for agreement [{}]. {}",
            &agreement_id,
            error
        )
    })?;

    log::debug!(
        "Sending invoice [{}] for agreement [{}].",
        &invoice.invoice_id,
        &agreement_id
    );
    payment_api
        .send_invoice(&invoice.invoice_id)
        .await
        .map_err(|error| {
            anyhow!(
                "Failed to send invoice [{}] for agreement [{}]. {}",
                &invoice.invoice_id,
                &agreement_id,
                error
            )
        })?;

    log::info!(
        "Invoice [{}] sent for agreement [{}].",
        &invoice.invoice_id,
        &agreement_id
    );

    Ok(invoice)
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
            ActorResponse::reply(Err(anyhow!(
                "Agreement [{}] wasn't registered.",
                &msg.agreement_id
            )))
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

            address.do_send(msg);
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
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: FinalizeActivity, ctx: &mut Context<Self>) -> Self::Result {
        if let Some(agreement) = self.agreements.get_mut(&msg.debit_info.agreement_id) {
            log::info!("Activity [{}] finished.", &msg.debit_info.activity_id);

            let result = agreement.finish_activity(&msg.debit_info.activity_id, msg.cost_summary);

            // Temporary. Requestor should close agreement, but for now we
            // treat destroying activity as closing agreement.
            send_message(
                ctx.address(),
                AgreementClosed {
                    agreement_id: msg.debit_info.agreement_id.clone(),
                },
            );

            return ActorResponse::reply(result);
        } else {
            log::warn!(
                "Not my activity - agreement [{}].",
                &msg.debit_info.agreement_id
            );
            return ActorResponse::reply(Err(anyhow!("")));
        }
    }
}

impl Handler<AgreementClosed> for Payments {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: AgreementClosed, _ctx: &mut Context<Self>) -> Self::Result {
        if let Some(agreement) = self.agreements.get_mut(&msg.agreement_id) {
            log::info!(
                "Payments - agreement [{}] closed. Computing cost summary...",
                &msg.agreement_id
            );

            let cost_summary = agreement.cost_summary();
            let activities = agreement.list_activities();
            let provider_context = self.context.clone();
            let agreement_id = msg.agreement_id.clone();

            let future = async move {
                // Resend invoice until it will reach provider.
                // Note: that we don't remove invoices that were issued but not sent.
                loop {
                    match send_invoice(&provider_context, &agreement_id, &cost_summary, &activities)
                        .await
                    {
                        Ok(invoice) => return invoice,
                        Err(error) => {
                            let interval = provider_context.invoice_resend_interval;

                            log::error!("{} Invoice will be resent after {:#?}.", error, interval);
                            tokio::time::delay_for(interval).await
                        }
                    }
                }
            }
            .into_actor(self)
            .map(|invoice, myself, context| {
                // Wait until Requestor accepts (or rejects) Invoice. This message will
                // be resent until Invoice state will change.
                let msg = CheckInvoiceAcceptance {
                    invoice_id: invoice.invoice_id.clone(),
                };
                context.notify_later(msg, myself.context.invoice_accept_check_interval);
                Ok(())
            });

            return ActorResponse::r#async(future);
        }

        return ActorResponse::reply(Err(anyhow!("Not my agreement {}.", &msg.agreement_id)));
    }
}

impl Handler<CheckInvoiceAcceptance> for Payments {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: CheckInvoiceAcceptance, _ctx: &mut Context<Self>) -> Self::Result {
        let pay_ctx = self.context.clone();
        let invoice_id = msg.invoice_id.clone();

        let future = async move { pay_ctx.payment_api.get_invoice(&invoice_id).await }
            .into_actor(self)
            .map(move |result, myself, context| {
                match result {
                    Ok(invoice) => {
                        match invoice.status {
                            InvoiceStatus::Accepted => {
                                log::info!("Invoice [{}] accepted by requestor.", &msg.invoice_id);

                                // Wait for payment to be settled.
                                myself.invoices_to_pay.push(invoice);
                                return Ok(());
                            }
                            InvoiceStatus::Rejected => {
                                log::warn!("Invoice [{}] rejected by requestor.", &msg.invoice_id);
                                //TODO: Send signal to other provider's modules to react to this situation.
                                //      Probably we don't want to cooperate with this Requestor anymore.
                                return Ok(());
                            }
                            //TODO: What means InvoiceStatus::Failed? How should we handle it?
                            _ => (),
                        }
                    }
                    Err(error) => {
                        log::error!(
                            "Can't get Invoice [{}] status. Error: {}",
                            &msg.invoice_id,
                            error
                        );
                    }
                };

                // Check invoice acceptance later, if state didn't change.
                context.notify_later(msg, myself.context.invoice_accept_check_interval);
                return Ok(());
            });
        return ActorResponse::r#async(future);
    }
}

impl Handler<CheckInvoicePayments> for Payments {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: CheckInvoicePayments, ctx: &mut Context<Self>) -> Self::Result {
        let pay_ctx = self.context.clone();
        let self_addr = ctx.address();

        let future = async move {
            let payments = match pay_ctx.payment_api.get_payments(Some(&msg.since)).await {
                Ok(payments) => payments,
                Err(error) => {
                    log::error!("Can't query payments. Error: {}", error);
                    vec![]
                }
            };

            payments.into_iter().for_each(|payment| {
                if let Some(invoices) = payment.invoice_ids.clone() {
                    let paid_message = InvoicesPaid { payment, invoices };
                    self_addr.do_send(paid_message);
                } else {
                    // What does it mean? Payment for debit notes? It seems that something
                    // has gone wrong, so better warn about this.
                    log::warn!("Payment [{}] has no invoices listed.", &payment.payment_id)
                }
            });
            Ok(())
        };

        ctx.notify_later(
            CheckInvoicePayments { since: Utc::now() },
            self.context.invoice_paid_check_interval,
        );
        return ActorResponse::r#async(future.into_actor(self));
    }
}

impl Handler<InvoicesPaid> for Payments {
    type Result = ActorResponse<Self, (), Error>;

    fn handle(&mut self, msg: InvoicesPaid, _ctx: &mut Context<Self>) -> Self::Result {
        log::info!(
            "Got payment [{}] confirmation, details: {}",
            msg.payment.payment_id,
            msg.payment.details
        );

        let paid_agreements = self
            .invoices_to_pay
            .iter()
            .filter(|element| msg.invoices.contains(&element.invoice_id))
            .map(|invoice| {
                log::info!(
                    "Invoice [{}] for agreement [{}] was paid. Amount: {}.",
                    invoice.invoice_id,
                    invoice.agreement_id,
                    invoice.amount
                );
                invoice.agreement_id.clone()
            })
            .collect::<Vec<String>>();

        self.earnings += &msg.payment.amount;

        log::info!("Our current earnings: {}.", self.earnings);

        self.invoices_to_pay
            .retain(|invoice| !msg.invoices.contains(&invoice.invoice_id));
        self.agreements
            .retain(|agreement_id, _| !paid_agreements.contains(agreement_id));

        let left_to_pay: Vec<String> = self
            .invoices_to_pay
            .iter()
            .map(|invoice| invoice.invoice_id.clone())
            .collect();
        log::info!("Invoices waiting for payment: {:#?}", left_to_pay);

        return ActorResponse::reply(Ok(()));
    }
}

impl Actor for Payments {
    type Context = Context<Self>;

    fn started(&mut self, context: &mut Context<Self>) {
        // Start checking incoming payments.
        let msg = CheckInvoicePayments { since: Utc::now() };
        context.notify_later(msg, self.context.invoice_paid_check_interval);
    }
}
