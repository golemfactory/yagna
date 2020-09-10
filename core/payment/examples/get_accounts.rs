use structopt::StructOpt;
use ya_client::payment::{PaymentProviderApi, PaymentRequestorApi};
use ya_client::web::WebClient;
use ya_client_model::payment::Account;

#[derive(Clone, Debug, StructOpt)]
struct Args {
    #[structopt(short, long, default_value = "NGNT")]
    platform: String,
    #[structopt()]
    provider_addr: String,
    #[structopt()]
    requestor_addr: String,
}

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    let log_level = std::env::var("RUST_LOG").unwrap_or("info".to_owned());
    std::env::set_var("RUST_LOG", log_level);
    env_logger::init();

    let args: Args = Args::from_args();

    let client = WebClient::builder().build();
    let provider: PaymentProviderApi = client.interface()?;
    let requestor: PaymentRequestorApi = client.interface()?;

    log::info!("Checking provider account...");
    let provider_account = Account {
        platform: args.platform.clone(),
        address: args.provider_addr,
    };
    let provider_accounts = provider.get_accounts().await?;
    assert!(provider_accounts
        .iter()
        .any(|account| account == &provider_account));
    log::info!("OK.");

    log::info!("Checking requestor account...");
    let requestor_account = Account {
        platform: args.platform.clone(),
        address: args.requestor_addr,
    };
    let requestor_accounts = requestor.get_accounts().await?;
    assert!(requestor_accounts
        .iter()
        .any(|account| account == &requestor_account));
    log::info!("OK.");

    Ok(())
}
