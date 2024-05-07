use actix_http::header::HeaderMap;
use actix_web::{web, Error, HttpResponse, Responder};
use actix_web::{App, HttpServer};
use async_stream::stream;
use bytes::Bytes;
use futures::StreamExt;
use test_context::test_context;
use ya_framework_basic::async_drop::DroppableTestContext;
use ya_gsb_http_proxy::gsb_to_http::GsbToHttpProxy;
use ya_gsb_http_proxy::http_to_gsb::{BindingMode, HttpToGsbProxy};

// #[cfg_attr(not(feature = "framework-test"), ignore)]
#[test_context(DroppableTestContext)]
#[serial_test::serial]
pub async fn test_gsb_http_proxy(ctx: &mut DroppableTestContext) {
    start_proxy_http_server(ctx).await;
    start_target_server(ctx).await;

    ya_sb_router::bind_gsb_router(None)
        .await
        .expect("should bind to gsb");

    let mut gsb_proxy = GsbToHttpProxy::new("http://127.0.0.1:8082/".into());
    gsb_proxy.bind(ya_gsb_http_proxy::BUS_ID);

    let response = reqwest::get("http://127.0.0.1:8081/proxy")
        .await
        .expect("request should succeed");
    let r: String = response.text().await.expect("response text expected");
    assert_eq!(r, "correct");
}

// #[cfg_attr(not(feature = "framework-test"), ignore)]
#[test_context(DroppableTestContext)]
#[serial_test::serial]
pub async fn test_gsb_http_streaming_proxy(ctx: &mut DroppableTestContext) {
    start_proxy_http_server(ctx).await;
    start_target_server(ctx).await;

    ya_sb_router::bind_gsb_router(None)
        .await
        .expect("should bind to gsb");

    let mut gsb_proxy = GsbToHttpProxy::new("http://127.0.0.1:8082/".into());
    gsb_proxy.bind_streaming(ya_gsb_http_proxy::BUS_ID);

    let client = reqwest::Client::new();
    let response = client
        .get("http://127.0.0.1:8081/proxy_stream")
        .header(reqwest::header::ACCEPT, "application/octet-stream")
        .send()
        .await
        .expect("request should succeed");

    let r: String = response.text().await.expect("response text expected");
    assert_eq!(r, "All the chunks... 0 1 2 3 4");
}

async fn start_proxy_http_server(ctx: &mut DroppableTestContext) {
    async fn proxy_endpoint() -> impl Responder {
        let mut http_to_gsb = HttpToGsbProxy::new(BindingMode::Local);

        let response = http_to_gsb
            .pass(
                "GET".to_string(),
                "target-endpoint".to_string(),
                HeaderMap::default(),
                None,
            )
            .await;

        if let Ok(r) = response.body {
            if let Ok(s) = String::from_utf8(r.to_vec()) {
                return s;
            }
        }
        "failed".to_string()
    }

    async fn proxy_stream_endpoint() -> impl Responder {
        let mut http_to_gsb = HttpToGsbProxy::new(BindingMode::Local);

        let response = http_to_gsb
            .pass_streaming(
                "GET".to_string(),
                "target-stream-endpoint".to_string(),
                HeaderMap::default(),
                None,
            )
            .await;

        if let Ok(r) = response.body {
            return HttpResponse::Ok().streaming(r);
        }
        HttpResponse::InternalServerError().body("failed".to_string())
    }

    let proxy_server = HttpServer::new(move || {
        App::new()
            .route("/proxy", web::get().to(proxy_endpoint))
            .route("/proxy_stream", web::get().to(proxy_stream_endpoint))
    })
    .bind(("127.0.0.1", 8081))
    .expect("should bind correctly")
    .run();

    ctx.register(proxy_server.handle());
    tokio::task::spawn_local(async move { anyhow::Ok(proxy_server.await?) });
}

async fn start_target_server(ctx: &mut DroppableTestContext) {
    let responder = HttpServer::new(|| {
        App::new()
            .route(
                "/target-endpoint",
                web::get().to(|| async { HttpResponse::Ok().body("correct") }),
            )
            .route(
                "/target-stream-endpoint",
                web::get().to(|| async {
                    let stream = stream! {
                        yield Ok::<Bytes, Error>(Bytes::from("All the chunks..."));
                        for i in 0..5 {
                            yield Ok::<Bytes, Error>(Bytes::from(format!(" {i}")))
                        }
                    };
                    HttpResponse::Ok().streaming(stream)
                }),
            )
    })
    .bind(("127.0.0.1", 8082))
    .expect("should bind correctly")
    .run();

    ctx.register(responder.handle());
    tokio::task::spawn_local(async move { anyhow::Ok(responder.await?) });
}
