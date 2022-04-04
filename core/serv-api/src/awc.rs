use actix_connect::resolver::{AsyncResolver, ResolverConfig, ResolverOpts};
pub use awc::{Client, ClientBuilder};

pub async fn client_builder() -> anyhow::Result<ClientBuilder> {
    let tcp_connector = actix_connect::new_connector(
        AsyncResolver::tokio(ResolverConfig::google(), ResolverOpts::default()).await?,
    );
    let http_connection = actix_http::client::Connector::new()
        .connector(tcp_connector)
        .finish();
    Ok(ClientBuilder::new().connector(http_connection))
}
