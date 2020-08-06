use crate::processor::PaymentProcessor;
use futures::prelude::*;
use ya_persistence::executor::DbExecutor;
use ya_service_bus::typed::ServiceBinder;

pub fn bind_service(db: &DbExecutor, processor: PaymentProcessor) {
    log::debug!("Binding payment service to service bus");

    local::bind_service(db, processor.clone());
    public::bind_service(db, processor);

    log::debug!("Successfully bound payment service to service bus");
}

mod local {
    use super::*;
    use crate::dao::*;
    use ya_core_model::payment::local::*;

    pub fn bind_service(db: &DbExecutor, processor: PaymentProcessor) {
        log::debug!("Binding payment private service to service bus");

        ServiceBinder::new(BUS_ID, db, processor)
            .bind_with_processor(schedule_payment)
            .bind_with_processor(register_account)
            .bind_with_processor(unregister_account)
            .bind_with_processor(notify_payment)
            .bind_with_processor(get_status)
            .bind_with_processor(get_accounts);
        log::debug!("Successfully bound payment private service to service bus");
    }

    async fn schedule_payment(
        db: DbExecutor,
        processor: PaymentProcessor,
        sender: String,
        msg: SchedulePayment,
    ) -> Result<(), GenericError> {
        processor.schedule_payment(msg).await?;
        Ok(())
    }

    async fn register_account(
        db: DbExecutor,
        processor: PaymentProcessor,
        sender: String,
        msg: RegisterAccount,
    ) -> Result<(), RegisterAccountError> {
        processor.register_account(msg).await
    }

    async fn unregister_account(
        db: DbExecutor,
        processor: PaymentProcessor,
        sender: String,
        msg: UnregisterAccount,
    ) -> Result<(), UnregisterAccountError> {
        processor.unregister_account(msg).await
    }

    async fn get_accounts(
        db: DbExecutor,
        processor: PaymentProcessor,
        sender: String,
        msg: GetAccounts,
    ) -> Result<Vec<Account>, GenericError> {
        Ok(processor.get_accounts().await)
    }

    async fn notify_payment(
        db: DbExecutor,
        processor: PaymentProcessor,
        sender: String,
        msg: NotifyPayment,
    ) -> Result<(), GenericError> {
        processor.notify_payment(msg).await?;
        Ok(())
    }

    async fn get_status(
        db: DbExecutor,
        processor: PaymentProcessor,
        _caller: String,
        msg: GetStatus,
    ) -> Result<StatusResult, GenericError> {
        log::info!("get status: {:?}", msg);
        let GetStatus { platform, address } = msg;

        let incoming_fut = async {
            db.as_dao::<AgreementDao>()
                .incoming_transaction_summary(platform.clone(), address.clone())
                .await
        }
        .map_err(GenericError::new);

        let outgoing_fut = async {
            db.as_dao::<AgreementDao>()
                .outgoing_transaction_summary(platform.clone(), address.clone())
                .await
        }
        .map_err(GenericError::new);

        let reserved_fut = async {
            db.as_dao::<AllocationDao>()
                .total_remaining_allocation(platform.clone(), address.clone())
                .await
        }
        .map_err(GenericError::new);

        let amount_fut = processor
            .get_status(platform.clone(), address.clone())
            .map_err(GenericError::new);

        let (incoming, outgoing, amount, reserved) =
            future::try_join4(incoming_fut, outgoing_fut, amount_fut, reserved_fut).await?;

        Ok(StatusResult {
            amount,
            reserved,
            outgoing,
            incoming,
        })
    }
}

mod public {
    use super::*;

    use crate::dao::*;
    use crate::error::DbError;
    use crate::utils::*;

    use crate::error::processor::VerifyPaymentError;
    use ya_client_model::payment::*;
    use ya_client_model::NodeId;
    use ya_core_model::payment::public::*;
    use ya_persistence::types::Role;

    pub fn bind_service(db: &DbExecutor, processor: PaymentProcessor) {
        log::debug!("Binding payment public service to service bus");

        ServiceBinder::new(BUS_ID, db, processor)
            .bind(send_debit_note)
            .bind(accept_debit_note)
            .bind(reject_debit_note)
            .bind(cancel_debit_note)
            .bind(send_invoice)
            .bind(accept_invoice)
            .bind(reject_invoice)
            .bind(cancel_invoice)
            .bind_with_processor(send_payment);

        log::debug!("Successfully bound payment public service to service bus");
    }

    // ************************** DEBIT NOTE **************************

    async fn send_debit_note(
        db: DbExecutor,
        sender_id: String,
        msg: SendDebitNote,
    ) -> Result<Ack, SendError> {
        let debit_note = msg.0;
        let debit_note_id = debit_note.debit_note_id.clone();
        let activity_id = debit_note.activity_id.clone();
        let agreement_id = debit_note.agreement_id.clone();

        let agreement = match get_agreement(agreement_id.clone()).await {
            Err(e) => {
                return Err(SendError::ServiceError(e.to_string()));
            }
            Ok(None) => {
                return Err(SendError::BadRequest(format!(
                    "Agreement {} not found",
                    debit_note.agreement_id
                )));
            }
            Ok(Some(agreement)) => agreement,
        };

        let offeror_id = agreement.offer.provider_id.clone().unwrap(); // FIXME: provider_id shouldn't be an Option
        let issuer_id = debit_note.issuer_id.to_string();
        if sender_id != offeror_id || sender_id != issuer_id {
            return Err(SendError::BadRequest("Invalid sender node ID".to_owned()));
        }

        // FIXME: requestor_id should be non-optional NodeId field
        let node_id: NodeId = agreement
            .demand
            .requestor_id
            .clone()
            .unwrap()
            .parse()
            .unwrap();
        match async move {
            db.as_dao::<AgreementDao>()
                .create_if_not_exists(agreement, node_id, Role::Requestor)
                .await?;
            db.as_dao::<ActivityDao>()
                .create_if_not_exists(activity_id, node_id, Role::Requestor, agreement_id)
                .await?;
            db.as_dao::<DebitNoteDao>()
                .insert_received(debit_note)
                .await?;
            Ok(())
        }
        .await
        {
            Ok(_) => Ok(Ack {}),
            Err(DbError::Query(e)) => return Err(SendError::BadRequest(e.to_string())),
            Err(e) => return Err(SendError::ServiceError(e.to_string())),
        }
    }

    async fn accept_debit_note(
        db: DbExecutor,
        sender_id: String,
        msg: AcceptDebitNote,
    ) -> Result<Ack, AcceptRejectError> {
        let debit_note_id = msg.debit_note_id;
        let acceptance = msg.acceptance;
        let node_id = msg.issuer_id;

        let dao: DebitNoteDao = db.as_dao();
        let debit_note: DebitNote = match dao.get(debit_note_id.clone(), node_id).await {
            Ok(Some(debit_note)) => debit_note.into(),
            Ok(None) => return Err(AcceptRejectError::ObjectNotFound),
            Err(e) => return Err(AcceptRejectError::ServiceError(e.to_string())),
        };

        if sender_id != debit_note.recipient_id.to_string() {
            return Err(AcceptRejectError::Forbidden);
        }

        if debit_note.total_amount_due != acceptance.total_amount_accepted {
            let msg = format!(
                "Invalid amount accepted. Expected: {} Actual: {}",
                debit_note.total_amount_due, acceptance.total_amount_accepted
            );
            return Err(AcceptRejectError::BadRequest(msg));
        }

        match debit_note.status {
            DocumentStatus::Accepted => return Ok(Ack {}),
            DocumentStatus::Settled => return Ok(Ack {}),
            DocumentStatus::Cancelled => {
                return Err(AcceptRejectError::BadRequest(
                    "Cannot accept cancelled debit note".to_owned(),
                ))
            }
            _ => (),
        }

        match dao.accept(debit_note_id, node_id).await {
            Ok(_) => Ok(Ack {}),
            Err(DbError::Query(e)) => Err(AcceptRejectError::BadRequest(e.to_string())),
            Err(e) => Err(AcceptRejectError::ServiceError(e.to_string())),
        }
    }

    async fn reject_debit_note(
        db: DbExecutor,
        sender: String,
        msg: RejectDebitNote,
    ) -> Result<Ack, AcceptRejectError> {
        unimplemented!() // TODO
    }

    async fn cancel_debit_note(
        db: DbExecutor,
        sender: String,
        msg: CancelDebitNote,
    ) -> Result<Ack, CancelError> {
        unimplemented!() // TODO
    }

    // *************************** INVOICE ****************************

    async fn send_invoice(
        db: DbExecutor,
        sender_id: String,
        msg: SendInvoice,
    ) -> Result<Ack, SendError> {
        let invoice = msg.0;
        let invoice_id = invoice.invoice_id.clone();
        let agreement_id = invoice.agreement_id.clone();
        let activity_ids = invoice.activity_ids.clone();

        let agreement = match get_agreement(agreement_id.clone()).await {
            Err(e) => {
                return Err(SendError::ServiceError(e.to_string()));
            }
            Ok(None) => {
                return Err(SendError::BadRequest(format!(
                    "Agreement {} not found",
                    invoice.agreement_id
                )));
            }
            Ok(Some(agreement)) => agreement,
        };

        for activity_id in activity_ids.iter() {
            match provider::get_agreement_id(activity_id.clone()).await {
                Ok(Some(id)) if id != agreement_id => {
                    return Err(SendError::BadRequest(format!(
                        "Activity {} belongs to agreement {} not {}",
                        activity_id, id, agreement_id
                    )));
                }
                Ok(None) => {
                    return Err(SendError::BadRequest(format!(
                        "Activity not found: {}",
                        activity_id
                    )))
                }
                Err(e) => return Err(SendError::ServiceError(e.to_string())),
                _ => (),
            }
        }

        let offeror_id = agreement.offer.provider_id.clone().unwrap(); // FIXME: provider_id shouldn't be an Option
        let issuer_id = invoice.issuer_id.to_string();
        if sender_id != offeror_id || sender_id != issuer_id {
            return Err(SendError::BadRequest("Invalid sender node ID".to_owned()));
        }

        // FIXME: requestor_id should be non-optional NodeId field
        let node_id: NodeId = agreement
            .demand
            .requestor_id
            .clone()
            .unwrap()
            .parse()
            .unwrap();
        match async move {
            db.as_dao::<AgreementDao>()
                .create_if_not_exists(agreement, node_id, Role::Requestor)
                .await?;

            let dao: ActivityDao = db.as_dao();
            for activity_id in activity_ids {
                dao.create_if_not_exists(
                    activity_id,
                    node_id,
                    Role::Requestor,
                    agreement_id.clone(),
                )
                .await?;
            }

            db.as_dao::<InvoiceDao>().insert_received(invoice).await?;
            Ok(())
        }
        .await
        {
            Ok(_) => Ok(Ack {}),
            Err(DbError::Query(e)) => return Err(SendError::BadRequest(e.to_string())),
            Err(e) => return Err(SendError::ServiceError(e.to_string())),
        }
    }

    async fn accept_invoice(
        db: DbExecutor,
        sender_id: String,
        msg: AcceptInvoice,
    ) -> Result<Ack, AcceptRejectError> {
        let invoice_id = msg.invoice_id;
        let acceptance = msg.acceptance;
        let node_id = msg.issuer_id;

        let dao: InvoiceDao = db.as_dao();
        let invoice: Invoice = match dao.get(invoice_id.clone(), node_id).await {
            Ok(Some(invoice)) => invoice.into(),
            Ok(None) => return Err(AcceptRejectError::ObjectNotFound),
            Err(e) => return Err(AcceptRejectError::ServiceError(e.to_string())),
        };

        if sender_id != invoice.recipient_id.to_string() {
            return Err(AcceptRejectError::Forbidden);
        }

        if invoice.amount != acceptance.total_amount_accepted {
            let msg = format!(
                "Invalid amount accepted. Expected: {} Actual: {}",
                invoice.amount, acceptance.total_amount_accepted
            );
            return Err(AcceptRejectError::BadRequest(msg));
        }

        match invoice.status {
            DocumentStatus::Accepted => return Ok(Ack {}),
            DocumentStatus::Settled => return Ok(Ack {}),
            DocumentStatus::Cancelled => {
                return Err(AcceptRejectError::BadRequest(
                    "Cannot accept cancelled invoice".to_owned(),
                ))
            }
            _ => (),
        }

        match dao.accept(invoice_id, node_id).await {
            Ok(_) => Ok(Ack {}),
            Err(DbError::Query(e)) => Err(AcceptRejectError::BadRequest(e.to_string())),
            Err(e) => Err(AcceptRejectError::ServiceError(e.to_string())),
        }
    }

    async fn reject_invoice(
        db: DbExecutor,
        sender: String,
        msg: RejectInvoice,
    ) -> Result<Ack, AcceptRejectError> {
        unimplemented!() // TODO
    }

    async fn cancel_invoice(
        db: DbExecutor,
        sender_id: String,
        msg: CancelInvoice,
    ) -> Result<Ack, CancelError> {
        let invoice_id = msg.invoice_id;

        let dao: InvoiceDao = db.as_dao();
        let invoice: Invoice = match dao.get(invoice_id.clone(), msg.recipient_id).await {
            Ok(Some(invoice)) => invoice.into(),
            Ok(None) => return Err(CancelError::ObjectNotFound),
            Err(e) => return Err(CancelError::ServiceError(e.to_string())),
        };

        if sender_id != invoice.issuer_id.to_string() {
            return Err(CancelError::Forbidden);
        }

        match invoice.status {
            DocumentStatus::Issued => (),
            DocumentStatus::Received => (),
            DocumentStatus::Rejected => (),
            DocumentStatus::Cancelled => return Ok(Ack {}),
            DocumentStatus::Accepted | DocumentStatus::Settled | DocumentStatus::Failed => {
                return Err(CancelError::Conflict)
            }
        }

        match dao.cancel(invoice_id, invoice.recipient_id).await {
            Ok(_) => Ok(Ack {}),
            Err(e) => Err(CancelError::ServiceError(e.to_string())),
        }
    }

    // *************************** PAYMENT ****************************

    async fn send_payment(
        db: DbExecutor,
        processor: PaymentProcessor,
        sender_id: String,
        msg: SendPayment,
    ) -> Result<Ack, SendError> {
        let payment = msg.0;
        if sender_id != payment.payer_id.to_string() {
            return Err(SendError::BadRequest("Invalid payer ID".to_owned()));
        }

        match processor.verify_payment(payment).await {
            Ok(_) => Ok(Ack {}),
            Err(e) => match e {
                VerifyPaymentError::ConfirmationEncoding => {
                    Err(SendError::BadRequest(e.to_string()))
                }
                VerifyPaymentError::Validation(e) => Err(SendError::BadRequest(e)),
                _ => Err(SendError::ServiceError(e.to_string())),
            },
        }
    }
}
