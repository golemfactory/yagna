use actix_http::header::HeaderMap;
use actix_web::{web, HttpResponse, Responder};
use actix_web::{App, HttpServer};
use chrono::Utc;
use futures::StreamExt;
use test_context::test_context;
use ya_framework_basic::async_drop::DroppableTestContext;
use ya_framework_basic::log::enable_logs;
use ya_gsb_http_proxy::http_to_gsb::HttpToGsbProxy;
use ya_gsb_http_proxy::message::GsbHttpCallMessage;
use ya_gsb_http_proxy::response::GsbHttpCallResponseEvent;
use ya_service_bus::typed as bus;

#[test_context(DroppableTestContext)]
#[serial_test::serial]
pub async fn test_gsb_http_proxy(ctx: &mut DroppableTestContext) -> anyhow::Result<()> {
    enable_logs(true);

    async fn proxy_endpoint() -> impl Responder {
        let http_to_gsb = HttpToGsbProxy {
            method: "GET".to_string(),
            path: "target".to_string(),
            body: None,
            headers: HeaderMap::default(),
        };

        let mut stream = http_to_gsb
            .pass(move |msg| bus::service(ya_gsb_http_proxy::BUS_ID).call_streaming(msg));
        if let Ok(_r) = stream.next().await.unwrap() {
            if let Ok(s) = String::from_utf8(_r.to_vec()) {
                return s;
            }
        }
        "failed".to_string()
    }

    let requestor = HttpServer::new(|| App::new().route("/proxy", web::get().to(proxy_endpoint)))
        .bind(("127.0.0.1", 8081))?
        .run();

    ctx.register(requestor.handle());
    tokio::task::spawn_local(async move { anyhow::Ok(requestor.await?) });

    ya_sb_router::bind_gsb_router(None).await?;

    let _stream_handle = bus::bind(
        ya_gsb_http_proxy::BUS_ID,
        move |msg: GsbHttpCallMessage| async move {
            let url = format!("http://127.0.0.1:8082/{}", msg.path.clone().as_str());
            let response = reqwest::get(url)
                .await
                .expect("internal call should succeed");
            let msg = response.text().await.expect("text expected");
            let response = GsbHttpCallResponseEvent {
                index: 0,
                timestamp: Utc::now().naive_local().to_string(),
                msg_bytes: msg.into_bytes(),
            };
            Ok(response)
        },
    );
    let responder = HttpServer::new(|| {
        App::new().route(
            "/target",
            web::get().to(|| async { HttpResponse::Ok().body("correct") }),
        )
    })
    .bind(("127.0.0.1", 8082))?
    .run();

    ctx.register(responder.handle());
    tokio::task::spawn_local(async move { anyhow::Ok(responder.await?) });

    let response = reqwest::get("http://127.0.0.1:8081/proxy").await?;
    let r: String = response.text().await?;
    log::info!("{}", r);
    assert_eq!(r, "correct");
    Ok(())
}
