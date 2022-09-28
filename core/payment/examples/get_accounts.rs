use structopt::StructOpt;
use ya_client::payment::PaymentApi;
use ya_client::web::{rest_api_url, WebClient};

#[derive(Clone, Debug, StructOpt)]
struct Args {
    #[structopt(short, long, default_value = "dummy-glm")]
    platform: String,
    #[structopt()]
    provider_addr: String,
    #[structopt()]
    requestor_addr: String,
}

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    let log_level = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_owned());
    std::env::set_var("RUST_LOG", log_level);
    env_logger::init();

    let args: Args = Args::from_args();

    // Create requestor / provider PaymentApi
    let provider_url = format!("{}provider/", rest_api_url()).parse().unwrap();
    let provider: PaymentApi = WebClient::builder()
        .api_url(provider_url)
        .build()
        .interface()?;
    let requestor_url = format!("{}requestor/", rest_api_url()).parse().unwrap();
    let requestor: PaymentApi = WebClient::builder()
        .api_url(requestor_url)
        .build()
        .interface()?;

    log::info!("Checking provider account...");
    let provider_accounts = provider.get_provider_accounts().await?;
    log::info!("provider_accounts: {:?}", &provider_accounts);
    assert!(provider_accounts
        .iter()
        .any(|account| account.platform == args.platform && account.address == args.provider_addr));
    log::info!("OK.");

    log::info!("Checking requestor account...");
    let requestor_accounts = requestor.get_requestor_accounts().await?;
    log::info!("requestor_accounts: {:?}", &requestor_accounts);
    assert!(
        requestor_accounts
            .iter()
            .any(|account| account.platform == args.platform
                && account.address == args.requestor_addr)
    );
    log::info!("OK.");

    log::info!(" 👍🏻 Example completed successfully ❤️");
    Ok(())
}
