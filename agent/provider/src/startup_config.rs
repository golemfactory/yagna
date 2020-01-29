use structopt::StructOpt;
use url::Url;
use ya_client::web::{WebAuth, WebClient, WebClientBuilder};
use ya_client::{market ,activity};

#[derive(StructOpt)]
pub struct StartupConfig {
    #[structopt(long = "app-key", env = "YAGNA_APPKEY", hide_env_values = true)]
    pub auth: String,
    ///
    #[structopt(long = "market-url", env = "YAGNA_MARKET_URL")]
    market_url: Url,
    ///
    #[structopt(long = "activity-url", env = "YAGNA_ACTIVITY_URL")]
    activity_url: Url,
    ///
    #[structopt(long = "exe-unit-path", env = "EXE_UNIT_PATH")]
    pub exe_unit_path: String,
}

impl StartupConfig {
    pub fn market_client(&self) -> market::ProviderApi {
        WebClient::with_token(&self.auth).unwrap().interface_at(self.market_url.clone())
    }

    pub fn activity_client(&self) -> activity::ProviderApiClient {
        WebClient::with_token(&self.auth).unwrap().interface_at(self.activity_url.clone())
    }
}
