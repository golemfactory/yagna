use structopt::StructOpt;
use url::Url;
use ya_client::{
    activity::ActivityProviderApi, market::MarketProviderApi, web::WebClient, web::WebInterface,
    Result,
};

#[derive(StructOpt)]
pub struct StartupConfig {
    #[structopt(long = "app-key", env = "YAGNA_APPKEY", hide_env_values = true)]
    pub auth: String,
    ///
    #[structopt(long = "market-url", env = MarketProviderApi::API_URL_ENV_VAR)]
    market_url: Url,
    ///
    #[structopt(long = "activity-url", env = ActivityProviderApi::API_URL_ENV_VAR)]
    activity_url: Option<Url>,
    ///
    #[structopt(long = "exe-unit-path", env = "EXE_UNIT_PATH")]
    pub exe_unit_path: String,
}

impl StartupConfig {
    pub fn market_client(&self) -> Result<MarketProviderApi> {
        Ok(WebClient::with_token(&self.auth)?.interface_at(self.market_url.clone()))
    }

    pub fn activity_client(&self) -> Result<ActivityProviderApi> {
        let client = WebClient::with_token(&self.auth)?;
        if let Some(url) = &self.activity_url {
            Ok(client.interface_at(url.clone()))
        } else {
            Ok(client.interface()?)
        }
    }
}
