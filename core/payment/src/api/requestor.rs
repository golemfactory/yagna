use crate::api::*;
use actix_web::web::{delete, get, post, put, Data, Json, Path, Query};
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
            "/debitNotes/{debit_note_id}/accept",
            post().to(accept_debit_note),
        )
        .route(
            "/debitNotes/{debit_note_id}/reject",
            post().to(reject_debit_note),
        )
        .route("/debitNoteEvents", get().to(get_debit_note_events))
        .route("/invoices", post().to(issue_invoice))
        .route("/invoices", get().to(get_invoices))
        .route("/invoices/{invoice_id}", get().to(get_invoice))
        .route(
            "/invoices/{invoice_id}/payments",
            get().to(get_invoice_payments),
        )
        .route("/invoices/{invoice_id}/accept", post().to(accept_invoice))
        .route("/invoices/{invoice_id}/reject", post().to(reject_invoice))
        .route("/invoiceEvents", get().to(get_invoice_events))
        .route("/allocations", post().to(create_allocation))
        .route("/allocations", get().to(get_allocations))
        .route("/allocations/{allocation_id}", get().to(get_allocation))
        .route("/allocations/{allocation_id}", put().to(amend_allocation))
        .route(
            "/allocations/{allocation_id}",
            delete().to(release_allocation),
        )
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

async fn accept_debit_note(
    db: Data<DbExecutor>,
    path: Path<DebitNoteId>,
    query: Query<Timeout>,
    body: Json<Acceptance>,
) -> HttpResponse {
    HttpResponse::NotImplemented().finish() // TODO
}

async fn reject_debit_note(
    db: Data<DbExecutor>,
    path: Path<DebitNoteId>,
    query: Query<Timeout>,
    body: Json<Rejection>,
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

async fn accept_invoice(
    db: Data<DbExecutor>,
    path: Path<InvoiceId>,
    query: Query<Timeout>,
    body: Json<Acceptance>,
) -> HttpResponse {
    HttpResponse::NotImplemented().finish() // TODO
}

async fn reject_invoice(
    db: Data<DbExecutor>,
    path: Path<InvoiceId>,
    query: Query<Timeout>,
    body: Json<Rejection>,
) -> HttpResponse {
    HttpResponse::NotImplemented().finish() // TODO
}

async fn get_invoice_events(db: Data<DbExecutor>, query: Query<EventParams>) -> HttpResponse {
    HttpResponse::NotImplemented().finish() // TODO
}

// ************************** ALLOCATION **************************

async fn create_allocation(db: Data<DbExecutor>, body: Json<Allocation>) -> HttpResponse {
    HttpResponse::NotImplemented().finish() // TODO
}

async fn get_allocations(db: Data<DbExecutor>) -> HttpResponse {
    HttpResponse::NotImplemented().finish() // TODO
}

async fn get_allocation(db: Data<DbExecutor>, path: Path<AllocationId>) -> HttpResponse {
    HttpResponse::NotImplemented().finish() // TODO
}

async fn amend_allocation(
    db: Data<DbExecutor>,
    path: Path<AllocationId>,
    body: Json<Allocation>,
) -> HttpResponse {
    HttpResponse::NotImplemented().finish() // TODO
}

async fn release_allocation(db: Data<DbExecutor>, path: Path<AllocationId>) -> HttpResponse {
    HttpResponse::NotImplemented().finish() // TODO
}

// *************************** PAYMENT ****************************

async fn get_payments(db: Data<DbExecutor>, query: Query<EventParams>) -> HttpResponse {
    HttpResponse::NotImplemented().finish() // TODO
}

async fn get_payment(db: Data<DbExecutor>, path: Path<PaymentId>) -> HttpResponse {
    HttpResponse::NotImplemented().finish() // TODO
}
