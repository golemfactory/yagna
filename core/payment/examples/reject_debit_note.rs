use bigdecimal::BigDecimal;
use chrono::Utc;
use std::time::Duration;
use structopt::StructOpt;
use ya_client::payment::PaymentApi;
use ya_client::web::{rest_api_url, WebClient};
use ya_client_model::payment::{
    Acceptance, DebitNoteEventType, DocumentStatus, Rejection, RejectionReason,
};

#[derive(Clone, Debug, StructOpt)]
struct Args {
    /// ID of a debit note received by the requestor.
    debit_note_id: String,
    /// If provided, accept the rejected debit note using this allocation.
    #[structopt(long)]
    allocation_id: Option<String>,
    #[structopt(long)]
    app_session_id: Option<String>,
}

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();
    let args = Args::from_args();

    let provider: PaymentApi = WebClient::builder()
        .api_url(format!("{}provider/", rest_api_url()).parse()?)
        .build()
        .interface()?;
    let requestor: PaymentApi = WebClient::builder()
        .api_url(format!("{}requestor/", rest_api_url()).parse()?)
        .build()
        .interface()?;

    let after = Utc::now();
    let rejection = Rejection {
        rejection_reason: RejectionReason::IncorrectAmount,
        total_amount_accepted: BigDecimal::from(0),
        message: Some("amount requires verification".to_owned()),
    };

    requestor
        .reject_debit_note(&args.debit_note_id, &rejection)
        .await?;

    let requestor_note = requestor.get_debit_note(&args.debit_note_id).await?;
    let provider_note = provider.get_debit_note(&args.debit_note_id).await?;
    assert_eq!(requestor_note.status, DocumentStatus::Rejected);
    assert_eq!(provider_note.status, DocumentStatus::Rejected);

    let events = provider
        .get_debit_note_events(
            Some(&after),
            Some(Duration::from_secs(5)),
            None,
            args.app_session_id,
        )
        .await?;
    assert!(events.iter().any(|event| matches!(
        &event.event_type,
        DebitNoteEventType::DebitNoteRejectedEvent { rejection: event_rejection }
            if event_rejection == &rejection
    )));

    if let Some(allocation_id) = args.allocation_id {
        requestor
            .accept_debit_note(
                &args.debit_note_id,
                &Acceptance {
                    total_amount_accepted: requestor_note.total_amount_due,
                    allocation_id,
                },
            )
            .await?;

        assert!(requestor
            .reject_debit_note(&args.debit_note_id, &rejection)
            .await
            .is_err());
    }

    Ok(())
}
