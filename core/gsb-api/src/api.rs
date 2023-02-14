use crate::service::StartBuffering;
use crate::services::{Bind, Find, Services, Unbind};
use crate::{GsbApiError, WsDisconnect, WsMessagesHandler};
use actix::Addr;
use actix_http::ws::{CloseCode, CloseReason};
use actix_http::StatusCode;
use actix_web::web::Data;
use actix_web::Scope;
use actix_web::{web, HttpRequest, Responder, Result};
use actix_web_actors::ws::{self};
use serde::{Deserialize, Serialize};
use ya_service_api_web::middleware::Identity;

pub(crate) fn web_scope(services: Addr<Services>) -> Scope {
    actix_web::web::scope(&format!("/{}", crate::GSB_API_PATH))
        .app_data(Data::new(services))
        .service(post_services)
        .service(delete_services)
        .service(get_service_messages)
}

#[actix_web::post("/services")]
async fn post_services(
    body: web::Json<ServicesBody>,
    _id: Identity,
    services: Data<Addr<Services>>,
) -> Result<impl Responder, GsbApiError> {
    log::debug!("POST /services Body: {:?}", body);
    let listen = &body.listen;
    let components = listen.components.clone();
    let on = listen.on.clone();
    let bind = Bind {
        components: components.clone(),
        addr_prefix: on.clone(),
    };
    let response = services.send(bind).await;
    log::debug!("Service bind result: {:?}", response);
    response??;
    let listen_on_encoded = base64::encode(&on);
    let links = ServicesLinksBody {
        messages: format!("gsb-api/v1/services/{listen_on_encoded}"),
    };
    let services = ServicesBody {
        listen: ServicesListenBody {
            on,
            components,
            links: Some(links),
        },
    };
    return Ok(web::Json(services)
        .customize()
        .with_status(StatusCode::CREATED));
}

#[actix_web::delete("/services/{key}")]
async fn delete_services(
    path: web::Path<ServicesPath>,
    _id: Identity,
    services: Data<Addr<Services>>,
) -> Result<impl Responder, GsbApiError> {
    let addr = decode_addr(&path.key)?;
    log::debug!("DELETE service: {}", addr);
    let unbind = Unbind { addr };
    let response = services.send(unbind).await;
    log::debug!("Service delete result: {:?}", response);
    response??;
    Ok(web::Json(()))
}

#[actix_web::get("/services/{key}")]
async fn get_service_messages(
    path: web::Path<ServicesPath>,
    req: HttpRequest,
    stream: web::Payload,
    services: Data<Addr<Services>>,
) -> Result<impl Responder, GsbApiError> {
    let addr = decode_addr(&path.key)?;
    log::debug!("GET WS service: {}", addr);
    let service = services.send(Find { addr }).await??;
    if let Some(ws_handler) = service.send(StartBuffering).await? {
        let description = Some("Closing old WS connection on new WS connection".to_string());
        let code = CloseCode::Policy;
        let ws_disconnect = WsDisconnect(CloseReason { code, description });
        ws_handler.send(ws_disconnect).await?;
    } else {
        log::debug!("No old WS connection");
    }
    let handler = WsMessagesHandler { service };
    let (_addr, resp) = ws::WsResponseBuilder::new(handler, &req, stream).start_with_addr()?;
    Ok(resp)
}

fn decode_addr(addr_encoded: &str) -> Result<String, GsbApiError> {
    base64::decode(addr_encoded)
        .map_err(|err| {
            GsbApiError::BadRequest(format!(
                "Service address should be encoded in base64. Unable to decode. Err: {err}"
            ))
        })
        .map(String::from_utf8)?
        .map_err(|err| {
            GsbApiError::BadRequest(format!(
                "Service address should be a string. Unable to parse address. Err: {err}"
            ))
        })
}

#[derive(Deserialize)]
pub struct ServicesPath {
    pub key: String,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct ServicesBody {
    listen: ServicesListenBody,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
struct ServicesListenBody {
    on: String,
    components: Vec<String>,
    links: Option<ServicesLinksBody>,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
struct ServicesLinksBody {
    messages: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GsbApiService, GsbError, GSB_API_PATH};
    use actix::Actor;
    use actix_http::ws::{self, CloseCode, CloseReason, Frame};
    use actix_test::{self, TestServer};
    use actix_web::App;
    use awc::error::WsClientError;
    use awc::SendClientRequest;
    use bytes::Bytes;
    use futures::{SinkExt, StreamExt, TryStreamExt};
    use serde_json::Value;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;
    use test_case::test_case;
    use ya_core_model::gftp::{GetChunk, GftpChunk};
    use ya_core_model::NodeId;
    use ya_service_api_interfaces::Provider;
    use ya_service_api_web::middleware::auth::dummy::DummyAuth;

    static SERVICE_COUNTER: AtomicUsize = AtomicUsize::new(0);

    #[cfg(test)]
    #[ctor::ctor]
    fn init() {
        env_logger::builder().is_test(true).init();
    }

    struct TestContext;
    impl Provider<GsbApiService, ()> for TestContext {
        fn component(&self) {
            todo!("NYI")
        }
    }

    #[derive(Serialize, Deserialize, Debug)]
    struct TestWsRequest<MSG> {
        id: String,
        component: String,
        payload: MSG,
    }

    #[derive(Serialize, Deserialize, Debug)]
    struct TestWsResponse<MSG> {
        id: String,
        payload: MSG,
    }

    const SERVICE_ADDR: &str = "/public/gftp/123";
    const PAYLOAD_LEN: usize = 10;

    fn dummy_api() -> TestServer {
        actix_test::start(|| {
            App::new()
                .service(GsbApiService::rest_internal(
                    &TestContext {},
                    Services::default().start(),
                ))
                .wrap(dummy_auth())
        })
    }

    fn dummy_auth() -> DummyAuth {
        let id = Identity {
            identity: NodeId::default(),
            name: "dummy_node".to_string(),
            role: "dummy".to_string(),
        };
        DummyAuth::new(id)
    }

    /// Returns POST service request and service address.
    fn bind_get_chunk_service_req_w_address(
        api: &mut TestServer,
        service_address: String,
    ) -> (SendClientRequest, String) {
        let service_req = api
            .post(format!("/{}/{}", GSB_API_PATH, "services"))
            .send_json(&ServicesBody {
                listen: ServicesListenBody {
                    components: vec!["GetChunk".to_string()],
                    on: service_address.clone(),
                    links: None,
                },
            });
        (service_req, service_address)
    }

    /// Returns POST service request and service address.
    fn bind_get_chunk_service_req(api: &mut TestServer) -> (SendClientRequest, String) {
        let service_number = SERVICE_COUNTER.fetch_add(1, Ordering::SeqCst);
        let service_address = format!("{SERVICE_ADDR}_{service_number}");
        bind_get_chunk_service_req_w_address(api, service_address)
    }

    async fn verify_bind_service_response(
        bind_req: SendClientRequest,
        components: Vec<String>,
        service_addr: &str,
    ) -> ServicesBody {
        let mut bind_resp = bind_req.await.unwrap();
        log::debug!("Bind service response: {:?}", bind_resp);
        assert_eq!(bind_resp.status(), StatusCode::CREATED);
        let body = bind_resp.body().await.unwrap();
        let body: ServicesBody = serde_json::de::from_slice(&body).unwrap();
        assert_eq!(
            body,
            ServicesBody {
                listen: ServicesListenBody {
                    components,
                    on: service_addr.to_string(),
                    links: Some(ServicesLinksBody {
                        messages: format!(
                            "{}/services/{}",
                            GSB_API_PATH,
                            base64::encode(service_addr)
                        )
                    }),
                }
            }
        );
        body
    }

    async fn verify_delete_service(api: &mut TestServer, service_addr: &str) {
        let delete_resp = api
            .delete(&format!(
                "/{}/{}/{}",
                GSB_API_PATH,
                "services",
                base64::encode(service_addr)
            ))
            .send()
            .await
            .unwrap();
        assert_eq!(
            delete_resp.status(),
            StatusCode::OK,
            "Can delete service, but got {delete_resp:?}"
        );
    }

    #[actix_web::test]
    async fn ok_payload_test() {
        let mut api = dummy_api();

        let (bind_req, service_addr) = bind_get_chunk_service_req(&mut api);
        let body =
            verify_bind_service_response(bind_req, vec!["GetChunk".to_string()], &service_addr)
                .await;

        let services_path = body.listen.links.unwrap().messages;
        let mut ws_frames = api.ws_at(&services_path).await.unwrap();

        let gsb_endpoint = ya_service_bus::typed::service(service_addr.clone());

        let (gsb_res, ws_res) = tokio::join!(
            async {
                gsb_endpoint
                    .call(GetChunk {
                        offset: u64::MIN,
                        size: PAYLOAD_LEN as u64,
                    })
                    .await
            },
            async {
                let ws_req = ws_frames.try_next().await;
                assert!(ws_req.is_ok());
                let ws_req = ws_req.unwrap();
                let ws_req = ws_req.unwrap();
                let ws_req = match ws_req {
                    Frame::Binary(ws_req) => {
                        flexbuffers::from_slice::<TestWsRequest<GetChunk>>(&ws_req).unwrap()
                    }
                    msg => panic!("Not expected msg: {:?}", msg),
                };
                let id = ws_req.id;
                let len = ws_req.payload.size as usize;
                let res_msg = GftpChunk {
                    content: vec![7; len],
                    offset: 0,
                };
                let ws_res = TestWsResponse {
                    id,
                    payload: res_msg,
                };
                let ws_res = flexbuffers::to_vec(ws_res).unwrap();
                ws_frames
                    .send(ws::Message::Binary(Bytes::from(ws_res)))
                    .await
            }
        );

        ws_res.unwrap();
        let gsb_res = gsb_res.unwrap().unwrap();
        assert_eq!(gsb_res.content, vec![7; PAYLOAD_LEN]);

        verify_delete_service(&mut api, &service_addr).await;
    }

    #[actix_web::test]
    async fn error_payload_test() {
        let mut api = dummy_api();

        let (bind_req, service_addr) = bind_get_chunk_service_req(&mut api);
        let body =
            verify_bind_service_response(bind_req, vec!["GetChunk".to_string()], &service_addr)
                .await;

        let services_path = body.listen.links.unwrap().messages;
        let mut ws_frames = api.ws_at(&services_path).await.unwrap();

        let gsb_endpoint = ya_service_bus::typed::service(&service_addr);
        const TEST_ERROR_MESSAGE: &str = "test error msg";
        let (gsb_res, ws_res) = tokio::join!(
            async {
                let msg = GetChunk {
                    offset: u64::MIN,
                    size: PAYLOAD_LEN as u64,
                };
                gsb_endpoint.call(msg).await
            },
            async {
                let ws_req = ws_frames.try_next().await;
                assert!(ws_req.is_ok());
                let ws_req = ws_req.unwrap();
                let ws_req = ws_req.unwrap();
                let ws_req = match ws_req {
                    Frame::Binary(ws_req) => {
                        flexbuffers::from_slice::<TestWsRequest<GetChunk>>(&ws_req).unwrap()
                    }
                    msg => panic!("Not expected msg: {:?}", msg),
                };
                let id = ws_req.id;
                let ws_res = serde_json::json!({
                    "id": id,
                    "error": {
                    "InternalError": TEST_ERROR_MESSAGE
                    }
                });
                let ws_res = flexbuffers::to_vec(ws_res).unwrap();
                ws_frames
                    .send(ws::Message::Binary(Bytes::from(ws_res)))
                    .await
            }
        );

        ws_res.unwrap();
        let gsb_res = gsb_res.unwrap();
        assert!(gsb_res.is_err());
        let gsb_err = gsb_res.err().unwrap();
        let expected_err =
            ya_core_model::gftp::Error::InternalError(TEST_ERROR_MESSAGE.to_string());
        match gsb_err {
            ya_core_model::gftp::Error::InternalError(msg) => assert_eq!(msg, TEST_ERROR_MESSAGE),
            other => panic!("Expected {:?} but got {:?}", expected_err, other),
        }

        verify_delete_service(&mut api, &service_addr).await;
    }

    #[test_case(r#"{}"#, Frame::Close(Some(CloseReason { 
        code: CloseCode::Policy,
        description: Some("Failed to read response. Err: Missing root map. Err: Empty map".to_string()) })); 
        "Close when empty"
    )]
    //TODO Why None here
    #[test_case(r#"{ "not_id": "nope" }"#, Frame::Close(None);
        "Close when no id")]
    #[test_case(r#"{ "id": "some", "not_payload": { "some": "value" } }"#, Frame::Close(Some(CloseReason { 
        code: CloseCode::Policy,
        description: Some("Failed to read response. Err: Missing 'payload' and 'error' fields. Id: some.".to_string()) })); 
        "Close when no payload or error fields")]
    #[test_case(r#"{ "id": "some", "error": {} }"#, Frame::Close(Some(CloseReason { 
        code: CloseCode::Policy,
        description: Some("Failed to read response. Err: Missing 'payload' and 'error' fields. Id: some.".to_string()) })); 
        "Close when error empty (error needs at least top level error name field)"
    )]
    #[actix_web::test]
    async fn ws_close_on_invalid_response(msg: &str, expected_frame: Frame) {
        let mut api = dummy_api();
        let (bind_req, service_addr) = bind_get_chunk_service_req(&mut api);
        let body =
            verify_bind_service_response(bind_req, vec!["GetChunk".to_string()], &service_addr)
                .await;
        let services_path = body.listen.links.unwrap().messages;
        let mut ws_frames = api.ws_at(&services_path).await.unwrap();
        println!("MSG: {msg}");
        let ws_res: Value = serde_json::de::from_str(msg).unwrap();
        let ws_res = flexbuffers::to_vec(ws_res).unwrap();
        let ws_res = ws_frames
            .send(ws::Message::Binary(Bytes::from(ws_res)))
            .await;
        assert!(ws_res.is_ok());
        match ws_frames.next().await {
            Some(Ok(frame)) => assert_eq!(frame, expected_frame),
            err => panic!("Expected Close frame. Got: {:?}", err),
        }
    }

    #[actix_web::test]
    async fn gsb_error_on_ws_error_test() {
        let mut api = dummy_api();

        let (bind_req, service_addr) = bind_get_chunk_service_req(&mut api);
        let body =
            verify_bind_service_response(bind_req, vec!["GetChunk".to_string()], &service_addr)
                .await;

        let services_path = body.listen.links.unwrap().messages;
        let mut ws_frames = api.ws_at(&services_path).await.unwrap();

        let gsb_endpoint = ya_service_bus::typed::service(&service_addr);
        let (gsb_res, ws_res) = tokio::join!(
            async {
                gsb_endpoint
                    .call(GetChunk {
                        offset: u64::MIN,
                        size: PAYLOAD_LEN as u64,
                    })
                    .await
            },
            async {
                let ws_req = ws_frames.try_next().await;
                assert!(ws_req.is_ok());
                let ws_error = ws::Message::Close(Some(CloseReason {
                    code: CloseCode::Normal,
                    description: Some("test error".to_string()),
                }));
                ws_frames.send(ws_error).await
            }
        );

        ws_res.unwrap();
        let expected_gsb_err_msg = "Normal: test error".to_string();
        match gsb_res {
            Err(ya_service_bus::Error::Closed(msg)) => assert_eq!(msg, expected_gsb_err_msg),
            other => panic!(
                "Expected Error: {:?}, got {:?}",
                ya_service_bus::Error::Closed(expected_gsb_err_msg),
                other
            ),
        }

        verify_delete_service(&mut api, &service_addr).await;
    }

    #[actix_web::test]
    async fn api_404_error_on_delete_of_not_existing_service_test() {
        let api = dummy_api();
        let delete_resp = api
            .delete(&format!(
                "/{}/{}/{}",
                GSB_API_PATH,
                "services",
                base64::encode("no_such_service")
            ))
            .send()
            .await
            .unwrap();
        assert_eq!(
            delete_resp.status(),
            StatusCode::NOT_FOUND,
            "Delete of not existing service results with 404"
        );
    }

    #[actix_web::test]
    async fn api_404_error_on_ws_connect_to_not_existing_service() {
        let mut api = dummy_api();
        let (bind_req, service_addr) = bind_get_chunk_service_req(&mut api);
        let body =
            verify_bind_service_response(bind_req, vec!["GetChunk".to_string()], &service_addr)
                .await;

        let _services_path = body.listen.links.unwrap().messages;
        let services_path = format!("/services/{}", base64::encode("no_such_service_address"));
        let ws_frames = api.ws_at(&services_path).await;
        let expected_err = WsClientError::InvalidResponseStatus(StatusCode::NOT_FOUND);
        if let Some(err) = ws_frames.err() {
            match err {
                WsClientError::InvalidResponseStatus(StatusCode::NOT_FOUND) => {}
                other => panic!("Expected {:?}, got {:?}", expected_err, other),
            }
        } else {
            panic!("Expected 404 error");
        }
    }

    #[actix_web::test]
    async fn api_400_error_on_ws_connect_to_incorrectly_encoded_service_address() {
        let mut api = dummy_api();
        let (bind_req, service_addr) = bind_get_chunk_service_req(&mut api);
        let body =
            verify_bind_service_response(bind_req, vec!["GetChunk".to_string()], &service_addr)
                .await;

        let services_path = body.listen.links.unwrap().messages;
        let services_path = format!("{services_path}_broken_base64");
        let ws_frames = api.ws_at(&services_path).await;
        let expected_err_code = StatusCode::BAD_REQUEST;
        if let Some(err) = ws_frames.err() {
            match err {
                WsClientError::InvalidResponseStatus(code) => {
                    assert!(matches!(code, _expected_err_code))
                }
                other => panic!(
                    "Expected {:?}, got {:?}",
                    WsClientError::InvalidResponseStatus(expected_err_code),
                    other
                ),
            }
        } else {
            panic!("Expected 404 error");
        }
    }

    #[actix_web::test]
    async fn error_on_post_of_duplicated_service_address() {
        let mut api = dummy_api();
        let (bind_req, service_addr) = bind_get_chunk_service_req(&mut api);
        let _ = verify_bind_service_response(bind_req, vec!["GetChunk".to_string()], &service_addr)
            .await;
        let second_bind_response = bind_get_chunk_service_req_w_address(&mut api, service_addr)
            .0
            .await
            .unwrap();
        assert_eq!(second_bind_response.status(), StatusCode::BAD_REQUEST);
    }

    #[actix_web::test]
    async fn ws_close_on_service_delete() {}

    #[actix_web::test]
    async fn buffering_gsb_msgs_before_ws_connect_test() {
        let mut api = dummy_api();

        let (bind_req, service_addr) = bind_get_chunk_service_req(&mut api);
        let body =
            verify_bind_service_response(bind_req, vec!["GetChunk".to_string()], &service_addr)
                .await;

        let gsb_endpoint = ya_service_bus::typed::service(&service_addr);

        let services_path = body.listen.links.unwrap().messages;

        let (gsb_res, ws_res) = tokio::join!(
            async {
                println!("GSB req");
                let gsb_resp = gsb_endpoint
                    .call(GetChunk {
                        offset: u64::MIN,
                        size: PAYLOAD_LEN as u64,
                    })
                    .await;
                println!("GSB res");
                gsb_resp
            },
            async {
                println!("WS sleep");
                tokio::time::sleep(Duration::from_millis(10)).await;

                println!("WS connect");
                let mut ws_frames = api.ws_at(&services_path).await.unwrap();

                println!("WS next");
                let ws_req = ws_frames.try_next().await;

                assert!(ws_req.is_ok());
                let ws_req = ws_req.unwrap().unwrap();
                let ws_req = match ws_req {
                    Frame::Binary(ws_req) => {
                        flexbuffers::from_slice::<TestWsRequest<GetChunk>>(&ws_req).unwrap()
                    }
                    msg => panic!("Not expected msg: {:?}", msg),
                };
                let id = ws_req.id;
                let len = ws_req.payload.size as usize;
                let res_msg = GftpChunk {
                    content: vec![7; len],
                    offset: 0,
                };
                let ws_res = TestWsResponse {
                    id,
                    payload: res_msg,
                };
                let ws_res = flexbuffers::to_vec(ws_res).unwrap();

                println!("WS send");
                let ws_res = ws_frames
                    .send(ws::Message::Binary(Bytes::from(ws_res)))
                    .await;

                println!("WS sent");
                ws_res
            }
        );

        ws_res.unwrap();
        let gsb_res = gsb_res.unwrap().unwrap();
        assert_eq!(gsb_res.content, vec![7; PAYLOAD_LEN]);

        verify_delete_service(&mut api, &service_addr).await;
    }

    #[actix_web::test]
    async fn buffering_gsb_msgs_after_ws_close_msg_and_reconnect_test() {
        let mut api = dummy_api();

        let (bind_req, service_addr) = bind_get_chunk_service_req(&mut api);
        let body =
            verify_bind_service_response(bind_req, vec!["GetChunk".to_string()], &service_addr)
                .await;

        let gsb_endpoint = ya_service_bus::typed::service(&service_addr);

        let services_path = body.listen.links.unwrap().messages;

        println!("WS connect");
        let mut ws_frames = api.ws_at(&services_path).await.unwrap();

        println!("WS closing MSG");
        ws_frames
            .send(ws::Message::Close(Some(CloseReason {
                code: CloseCode::Abnormal,
                description: Some("Test close reason".to_string()),
            })))
            .await
            .unwrap();

        let (gsb_res, ws_res) = tokio::join!(
            async {
                tokio::time::sleep(Duration::from_millis(10)).await;
                println!("GSB req");
                let gsb_resp = gsb_endpoint
                    .call(GetChunk {
                        offset: u64::MIN,
                        size: PAYLOAD_LEN as u64,
                    })
                    .await;
                println!("GSB res");
                gsb_resp
            },
            async {
                tokio::time::sleep(Duration::from_millis(20)).await;

                println!("WS reconnect");
                let mut ws_frames = api.ws_at(&services_path).await.unwrap();

                println!("WS next");
                let ws_req = ws_frames.try_next().await;

                assert!(ws_req.is_ok());
                let ws_req = ws_req.unwrap().unwrap();
                let ws_req = match ws_req {
                    Frame::Binary(ws_req) => {
                        flexbuffers::from_slice::<TestWsRequest<GetChunk>>(&ws_req).unwrap()
                    }
                    msg => panic!("Not expected msg: {:?}", msg),
                };
                let id = ws_req.id;
                let len = ws_req.payload.size as usize;
                let res_msg = GftpChunk {
                    content: vec![7; len],
                    offset: 0,
                };
                let ws_res = TestWsResponse {
                    id,
                    payload: res_msg,
                };
                let ws_res = flexbuffers::to_vec(ws_res).unwrap();

                println!("WS send");
                let ws_res = ws_frames
                    .send(ws::Message::Binary(Bytes::from(ws_res)))
                    .await;

                println!("WS sent");
                ws_res
            }
        );

        ws_res.unwrap();
        let gsb_res = gsb_res.unwrap().unwrap();
        assert_eq!(gsb_res.content, vec![7; PAYLOAD_LEN]);

        verify_delete_service(&mut api, &service_addr).await;
    }

    #[actix_web::test]
    async fn buffering_gsb_msgs_after_ws_disconnect_and_reconnect_test() {
        let mut api = dummy_api();

        let (bind_req, service_addr) = bind_get_chunk_service_req(&mut api);
        let body =
            verify_bind_service_response(bind_req, vec!["GetChunk".to_string()], &service_addr)
                .await;

        let gsb_endpoint = ya_service_bus::typed::service(&service_addr);

        let services_path = body.listen.links.unwrap().messages;

        println!("WS connect");
        let mut ws_frames = api.ws_at(&services_path).await.unwrap();

        println!("WS closing connection");
        ws_frames.close().await.unwrap();

        println!("GSB Waiting for disconnect");
        tokio::time::sleep(Duration::from_millis(10)).await;

        let (gsb_res, ws_res) = tokio::join!(
            async {
                println!("GSB req");
                let gsb_resp = gsb_endpoint
                    .call(GetChunk {
                        offset: u64::MIN,
                        size: PAYLOAD_LEN as u64,
                    })
                    .await;
                println!("GSB res");
                gsb_resp
            },
            async {
                println!("Waiting for disconnect and GSB");
                tokio::time::sleep(Duration::from_millis(10)).await;

                println!("WS connect");
                let mut ws_frames = api.ws_at(&services_path).await.unwrap();

                println!("WS next");
                let ws_req = ws_frames.try_next().await;

                assert!(ws_req.is_ok());
                let ws_req = ws_req.unwrap().unwrap();
                let ws_req = match ws_req {
                    Frame::Binary(ws_req) => {
                        flexbuffers::from_slice::<TestWsRequest<GetChunk>>(&ws_req).unwrap()
                    }
                    msg => panic!("Not expected msg: {:?}", msg),
                };
                let id = ws_req.id;
                let len = ws_req.payload.size as usize;
                let res_msg = GftpChunk {
                    content: vec![7; len],
                    offset: 0,
                };
                let ws_res = TestWsResponse {
                    id,
                    payload: res_msg,
                };
                let ws_res = flexbuffers::to_vec(ws_res).unwrap();

                println!("WS send");
                let ws_res = ws_frames
                    .send(ws::Message::Binary(Bytes::from(ws_res)))
                    .await;

                println!("WS sent");
                ws_res
            }
        );

        ws_res.unwrap();
        let gsb_res = gsb_res.unwrap().unwrap();
        assert_eq!(gsb_res.content, vec![7; PAYLOAD_LEN]);

        verify_delete_service(&mut api, &service_addr).await;
    }

    #[actix_web::test]
    async fn gsb_buffered_msgs_errors_on_delete_test() {
        let mut api = dummy_api();

        let (bind_req, service_addr) = bind_get_chunk_service_req(&mut api);
        let body =
            verify_bind_service_response(bind_req, vec!["GetChunk".to_string()], &service_addr)
                .await;

        let gsb_endpoint = ya_service_bus::typed::service(&service_addr);

        let services_path = body.listen.links.unwrap().messages;

        println!("WS connect");
        let mut ws_frames = api.ws_at(&services_path).await.unwrap();

        println!("WS closing connection");
        ws_frames.close().await.unwrap();

        let (gsb_res, _) = tokio::join!(
            async {
                println!("GSB req");
                let gsb_resp = gsb_endpoint
                    .call(GetChunk {
                        offset: u64::MIN,
                        size: PAYLOAD_LEN as u64,
                    })
                    .await;
                println!("GSB res");
                gsb_resp
            },
            async {
                println!("Delete service");
                verify_delete_service(&mut api, &service_addr).await;
            }
        );

        match gsb_res {
            Err(GsbError::Closed(msg)) => {
                assert!(msg.starts_with("Normal: Unbinding service: /public/gftp/123"))
            }
            _ => panic!("Expected GsbError::Closed, got: {:?}", gsb_res),
        }
    }

    #[actix_web::test]
    async fn close_old_ws_connection_on_new_ws_connection() {
        let mut api = dummy_api();

        let (bind_req, service_addr) = bind_get_chunk_service_req(&mut api);
        let body =
            verify_bind_service_response(bind_req, vec!["GetChunk".to_string()], &service_addr)
                .await;

        let gsb_endpoint = ya_service_bus::typed::service(&service_addr);

        let services_path = body.listen.links.unwrap().messages;

        println!("WS 0 connect");
        let mut ws_frames_0 = api.ws_at(&services_path).await.unwrap();

        println!("WS 1 connect");
        let mut ws_frames_1 = api.ws_at(&services_path).await.unwrap();

        let (gsb_res, (ws_req_0, ws_res_1)) = tokio::join!(
            async {
                gsb_endpoint
                    .call(GetChunk {
                        offset: u64::MIN,
                        size: PAYLOAD_LEN as u64,
                    })
                    .await
            },
            async {
                println!("WS 0 next");
                let ws_req_0 = ws_frames_0.try_next().await;

                println!("WS 1 next");
                let ws_req = ws_frames_1.try_next().await;

                assert!(ws_req.is_ok());
                let ws_req = ws_req.unwrap().unwrap();
                let ws_req = match ws_req {
                    Frame::Binary(ws_req) => {
                        flexbuffers::from_slice::<TestWsRequest<GetChunk>>(&ws_req).unwrap()
                    }
                    msg => panic!("Not expected msg: {:?}", msg),
                };
                let id = ws_req.id;
                let len = ws_req.payload.size as usize;
                let res_msg = GftpChunk {
                    content: vec![7; len],
                    offset: 0,
                };
                let ws_res = TestWsResponse {
                    id,
                    payload: res_msg,
                };
                let ws_res = flexbuffers::to_vec(ws_res).unwrap();

                println!("WS 1 send");
                let ws_res = ws_frames_1
                    .send(ws::Message::Binary(Bytes::from(ws_res)))
                    .await;

                println!("WS 1 sent");
                (ws_req_0, ws_res)
            }
        );

        println!("gsb_res: {gsb_res:?}");
        println!("ws_req_0: {ws_req_0:?}");
        println!("ws_res_1: {ws_res_1:?}");

        let gsb_res = gsb_res.unwrap().unwrap();
        assert_eq!(gsb_res.content, vec![7; PAYLOAD_LEN]);

        let expected_close_reason = CloseReason {
            code: ws::CloseCode::Policy,
            description: Some("Closing old WS connection on new WS connection".to_string()),
        };
        match ws_req_0 {
            Ok(Some(Frame::Close(Some(close_reason)))) => {
                assert_eq!(close_reason, expected_close_reason)
            }
            _other => panic!(
                "Unexpected response of disconnected WS. Expected: {:?}",
                expected_close_reason
            ),
        }

        assert!(ws_res_1.is_ok());
    }
}
