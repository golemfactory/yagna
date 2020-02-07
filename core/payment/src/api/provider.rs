use crate::api::*;
use actix_web::web::{get, post, Data, Json, Path, Query};
use actix_web::{HttpResponse, Scope};
use ya_model::payment::*;
use ya_persistence::executor::DbExecutor;

pub fn register_endpoints(scope: Scope) -> Scope {
    scope
        .route("/debitNotes", post().to(issue_debit_note))
        .route("/debitNotes", get().to(get_debit_notes))
        .route("/debitNotes/{debit_note_id}", get().to(get_debit_note))
        .route(
            "/debitNotes/{debit_note_id}/payments",
            get().to(get_debit_note_payments),
        )
        .route(
            "/debitNotes/{debit_note_id}/send",
            post().to(send_debit_note),
        )
        .route(
            "/debitNotes/{debit_note_id}/cancel",
            post().to(cancel_debit_note),
        )
        .route("/debitNoteEvents", get().to(get_debit_note_events))
        .route("/invoices", post().to(issue_invoice))
        .route("/invoices", get().to(get_invoices))
        .route("/invoices/{invoice_id}", get().to(get_invoice))
        .route(
            "/invoices/{invoice_id}/payments",
            get().to(get_invoice_payments),
        )
        .route("/invoices/{invoice_id}/send", post().to(send_invoice))
        .route("/invoices/{invoice_id}/cancel", post().to(cancel_invoice))
        .route("/invoiceEvents", get().to(get_invoice_events))
        .route("/payments", get().to(get_payments))
        .route("/payments/{payment_id}", get().to(get_payment))
}

// ************************** DEBIT NOTE **************************

async fn issue_debit_note(db: Data<DbExecutor>, body: Json<DebitNote>) -> HttpResponse {
    HttpResponse::NotImplemented().finish() // TODO
}

async fn get_debit_notes(db: Data<DbExecutor>) -> HttpResponse {
    HttpResponse::NotImplemented().finish() // TODO
}

async fn get_debit_note(db: Data<DbExecutor>, path: Path<DebitNoteId>) -> HttpResponse {
    HttpResponse::NotImplemented().finish() // TODO
}

async fn get_debit_note_payments(db: Data<DbExecutor>, path: Path<DebitNoteId>) -> HttpResponse {
    HttpResponse::NotImplemented().finish() // TODO
}

async fn send_debit_note(
    db: Data<DbExecutor>,
    path: Path<DebitNoteId>,
    query: Query<Timeout>,
) -> HttpResponse {
    HttpResponse::NotImplemented().finish() // TODO
}

async fn cancel_debit_note(
    db: Data<DbExecutor>,
    path: Path<DebitNoteId>,
    query: Query<Timeout>,
) -> HttpResponse {
    HttpResponse::NotImplemented().finish() // TODO
}

async fn get_debit_note_events(db: Data<DbExecutor>, query: Query<EventParams>) -> HttpResponse {
    HttpResponse::NotImplemented().finish() // TODO
}

// *************************** INVOICE ****************************

async fn issue_invoice(db: Data<DbExecutor>, body: Json<Invoice>) -> HttpResponse {
    HttpResponse::NotImplemented().finish() // TODO
}

async fn get_invoices(db: Data<DbExecutor>) -> HttpResponse {
    HttpResponse::NotImplemented().finish() // TODO
}

async fn get_invoice(db: Data<DbExecutor>, path: Path<InvoiceId>) -> HttpResponse {
    HttpResponse::NotImplemented().finish() // TODO
}

async fn get_invoice_payments(db: Data<DbExecutor>, path: Path<InvoiceId>) -> HttpResponse {
    HttpResponse::NotImplemented().finish() // TODO
}

async fn send_invoice(
    db: Data<DbExecutor>,
    path: Path<InvoiceId>,
    query: Query<Timeout>,
) -> HttpResponse {
    HttpResponse::NotImplemented().finish() // TODO
}

async fn cancel_invoice(
    db: Data<DbExecutor>,
    path: Path<InvoiceId>,
    query: Query<Timeout>,
) -> HttpResponse {
    HttpResponse::NotImplemented().finish() // TODO
}

async fn get_invoice_events(db: Data<DbExecutor>, query: Query<EventParams>) -> HttpResponse {
    HttpResponse::NotImplemented().finish() // TODO
}

// *************************** PAYMENT ****************************

async fn get_payments(db: Data<DbExecutor>, query: Query<EventParams>) -> HttpResponse {
    HttpResponse::NotImplemented().finish() // TODO
}

async fn get_payment(db: Data<DbExecutor>, path: Path<PaymentId>) -> HttpResponse {
    HttpResponse::NotImplemented().finish() // TODO
}
