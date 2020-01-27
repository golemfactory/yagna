use structopt::StructOpt;
use url::Url;
use ya_client::web::{WebAuth, WebClient, WebClientBuilder};

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
}

impl StartupConfig {
    pub fn market_client(&self) -> WebClientBuilder {
        let host_port = format!(
            "{}:{}",
            self.market_url.host_str().unwrap_or_default(),
            self.market_url.port_or_known_default().unwrap_or_default()
        );

        WebClient::builder()
            .auth(WebAuth::Bearer(self.auth.clone()))
            .host_port(host_port)
    }

    pub fn activity_client(&self) -> WebClientBuilder {
        let host_port = format!(
            "{}:{}",
            self.activity_url.host_str().unwrap_or_default(),
            self.activity_url
                .port_or_known_default()
                .unwrap_or_default()
        );
        WebClient::builder()
            //            .api_root(ACTIVITY_API)
            .host_port(host_port)
            .api_root(self.activity_url.path())
            .auth(WebAuth::Bearer(self.auth.clone()))
    }
}
