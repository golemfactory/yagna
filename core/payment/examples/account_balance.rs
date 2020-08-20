use bigdecimal::BigDecimal;
use chrono::Utc;
use std::time::Duration;
use ya_client::payment::{PaymentProviderApi, PaymentRequestorApi};
use ya_client::web::WebClient;
use ya_client_model::payment::{
    Acceptance, DocumentStatus, EventType, NewAllocation, NewDebitNote, NewInvoice,
};
use ya_core_model::payment::local as pay;
use ya_service_bus::typed as bus;

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    let log_level = std::env::var("RUST_LOG").unwrap_or("info".to_owned());
    std::env::set_var("RUST_LOG", log_level);
    env_logger::init();

    let client = WebClient::builder().build();
    let provider: PaymentProviderApi = client.interface()?;
    let requestor: PaymentRequestorApi = client.interface()?;

    let payer_status = bus::service(pay::BUS_ID)
        .call(pay::GetStatus {
            platform: payment_platform.to_string(),
            address: payer_addr.to_string(),
        })
        .await??;
    info!("Balance amount: {}", payer_status);

    Ok(())
}
