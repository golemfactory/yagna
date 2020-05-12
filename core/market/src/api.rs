use actix_web::{web, HttpResponse, Scope};
use jsonwebtoken::{encode, Header};
use serde::{Deserialize, Serialize};
use url::Url;

use ya_core_model::ethaddr::NodeId;
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;

/// implementation note: every request will timeout after 5s.
pub fn web_scope(_db: &DbExecutor) -> Scope {
    let central_market_url = ya_client::market::service_url().unwrap();
    Scope::new(crate::MARKET_API_PATH)
        .data(central_market_url)
        .data(awc::Client::new()) // has default timeout of 5s
        .service(web::resource("*").to(forward))
}

/// inspired by https://github.com/actix/examples/blob/master/http-proxy/src/main.rs
async fn forward(
    req: web::HttpRequest,
    body: web::Bytes,
    id: Identity,
    central_market_url: web::Data<Url>,
    client: web::Data<awc::Client>,
) -> std::result::Result<HttpResponse, actix_web::Error> {
    let mut forward_url = central_market_url.get_ref().clone();
    forward_url.set_path(req.uri().path());
    forward_url.set_query(req.uri().query());

    let forwarded_req = client
        .request_from(forward_url.as_str(), req.head())
        .set_header(
            awc::http::header::AUTHORIZATION,
            format!("Bearer {}", encode_jwt(id.identity)),
        );

    let mut res = forwarded_req
        .send_body(body)
        .await
        .map_err(actix_web::Error::from)?;

    let mut client_resp = HttpResponse::build(res.status());
    // Remove `Connection` as per
    // https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Connection#Directives
    for (header_name, header_value) in res.headers().iter().filter(|(h, _)| *h != "connection") {
        client_resp.header(header_name.clone(), header_value.clone());
    }

    Ok(client_resp.body(res.body().await?))
}

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    aud: String,
    sub: String,
}

fn encode_jwt(node_id: NodeId) -> String {
    let claims = Claims {
        aud: String::from("GolemNetHub"),
        sub: String::from(serde_json::json!(node_id).as_str().unwrap_or("unknown")),
    };

    encode(&Header::default(), &claims, "secret".as_ref()).unwrap_or(String::from("error"))
}
