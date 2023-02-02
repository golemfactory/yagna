use crate::service::Relay;
use crate::services::{Bind, Find, Services, Unbind};
use crate::{GsbApiError, WsMessagesHandler};
use actix::Addr;
use actix_http::StatusCode;
use actix_web::web::Data;
use actix_web::Scope;
use actix_web::{web, HttpRequest, Responder, Result};
use actix_web_actors::ws::{self};
use serde::{Deserialize, Serialize};
use ya_service_api_web::middleware::Identity;

pub const DEFAULT_SERVICES_TIMEOUT: f32 = 60.0;

pub fn web_scope() -> Scope {
    actix_web::web::scope(&format!("/{}", crate::GSB_API_PATH))
        .app_data(Data::new(crate::services::SERVICES.clone()))
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
    if let Some(listen) = &body.listen {
        let components = listen.components.clone();
        let on = listen.on.clone();
        let bind = Bind {
            components: components.clone(),
            addr_prefix: on.clone(),
        };
        let _ = services.send(bind).await??;
        let listen_on_encoded = base64::encode(&on);
        let links = ServicesLinksBody {
            messages: format!("gsb-api/v1/services/{listen_on_encoded}"),
        };
        let services = ServicesBody {
            listen: Some(ServicesListenBody {
                on,
                components,
                links: Some(links),
            }),
        };
        return Ok(web::Json(services)
            .customize()
            .with_status(StatusCode::CREATED));
    }
    Err(GsbApiError::BadRequest("Missing listen field".to_string()))
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
    let _ = services.send(unbind).await??;
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
    let handler = WsMessagesHandler {
        service: service.clone(),
    };
    let (addr, resp) = ws::WsResponseBuilder::new(handler, &req, stream).start_with_addr()?;
    service.send(Relay { ws_handler: addr }).await?;
    log::debug!("returning GET WS service");
    Ok(resp)
}

fn decode_addr(addr_encoded: &str) -> Result<String, GsbApiError> {
    base64::decode(addr_encoded)
        .map_err(|err| GsbApiError::BadRequest(format!("Unable to read key. Err: {}", err)))
        .map(String::from_utf8)?
        .map_err(|err| GsbApiError::BadRequest(format!("Unable to parse key. Err: {}", err)))
}

#[derive(Deserialize)]
pub struct ServicesPath {
    pub key: String,
}

#[derive(Deserialize, Serialize, PartialEq, Debug)]
pub struct ServicesBody {
    listen: Option<ServicesListenBody>,
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

#[derive(Deserialize, Serialize, Debug)]
pub struct Timeout {
    #[serde(rename = "timeout", default = "default_services_timeout")]
    pub timeout: Option<f32>,
}

#[inline(always)]
pub(crate) fn default_services_timeout() -> Option<f32> {
    Some(DEFAULT_SERVICES_TIMEOUT)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::{GsbApiService, GSB_API_PATH};
    use actix_http::ws::Frame;
    use actix_test::{self, TestServer};
    use actix_web::App;
    use actix_web_actors::ws;
    use awc::SendClientRequest;
    use bytes::Bytes;
    use futures::{SinkExt, TryStreamExt};
    use ya_core_model::gftp::{GetChunk, GftpChunk};
    use ya_core_model::NodeId;
    use ya_service_api_interfaces::Provider;
    use ya_service_api_web::middleware::auth::dummy::DummyAuth;

    struct TestContext;
    impl Provider<GsbApiService, ()> for TestContext {
        fn component(&self) -> () {
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
                .service(GsbApiService::rest(&TestContext {}))
                .wrap(dummy_auth())
        })
    }

    fn bind_get_chunk_service_req(api: &mut TestServer) -> SendClientRequest {
        api.post(&format!("/{}/{}", GSB_API_PATH, "services"))
            .send_json(&ServicesBody {
                listen: Some(ServicesListenBody {
                    components: vec!["GetChunk".to_string()],
                    on: SERVICE_ADDR.to_string(),
                    links: None,
                }),
            })
    }

    async fn verify_bind_service_response(
        bind_req: SendClientRequest,
        components: Vec<String>,
        service_addr: &str,
    ) -> ServicesBody {
        let mut bind_resp = bind_req.await.unwrap();
        assert_eq!(bind_resp.status(), StatusCode::CREATED);
        let body = bind_resp.body().await.unwrap();
        let body: ServicesBody = serde_json::de::from_slice(&body.to_vec()).unwrap();
        assert_eq!(
            body,
            ServicesBody {
                listen: Some(ServicesListenBody {
                    components,
                    on: SERVICE_ADDR.to_string(),
                    links: Some(ServicesLinksBody {
                        messages: format!(
                            "{}/services/{}",
                            GSB_API_PATH,
                            base64::encode(service_addr)
                        )
                    }),
                })
            }
        );
        return body;
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
        assert_eq!(delete_resp.status(), StatusCode::OK);
    }

    #[actix_web::test]
    async fn happy_path_test() {
        let _ = env_logger::builder().is_test(true).try_init();
        let mut api = dummy_api();

        let bind_req = bind_get_chunk_service_req(&mut api);
        let body =
            verify_bind_service_response(bind_req, vec!["GetChunk".to_string()], SERVICE_ADDR)
                .await;

        let services_path = body.listen.unwrap().links.unwrap().messages;
        let mut ws_frames = api.ws_at(&services_path).await.unwrap();

        let gsb_endpoint = ya_service_bus::typed::service(SERVICE_ADDR);

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

        let _ = ws_res.unwrap();
        let gsb_res = gsb_res.unwrap().unwrap();
        assert_eq!(gsb_res.content, vec![7; PAYLOAD_LEN]);

        verify_delete_service(&mut api, SERVICE_ADDR).await;
    }

    #[actix_web::test]
    async fn gsb_error_on_ws_error_test() {
        panic!("NYI");
    }

    #[actix_web::test]
    async fn api_401_error_on_unauthenticated_post_test() {
        panic!("NYI");
    }

    #[actix_web::test]
    async fn api_401_error_on_unauthenticated_delete_test() {
        panic!("NYI");
    }

    #[actix_web::test]
    async fn api_404_error_on_ws_connect_to_not_existing_service() {
        panic!("NYI");
    }

    #[actix_web::test]
    async fn api_404_error_on_delete_not_existing_service_test() {
        panic!("NYI");
    }

    #[actix_web::test]
    async fn error_on_post_of_duplicated_service() {
        panic!("NYI");
    }

    #[actix_web::test]
    async fn ws_close_on_service_delete() {
        panic!("NYI");
    }

    #[actix_web::test]
    async fn gsb_msgs_before_ws_connect_buffering_test() {
        let _ = env_logger::builder().is_test(true).try_init();

        let mut api = dummy_api();

        let bind_req = bind_get_chunk_service_req(&mut api);
        let body =
            verify_bind_service_response(bind_req, vec!["GetChunk".to_string()], SERVICE_ADDR)
                .await;

        let gsb_endpoint = ya_service_bus::typed::service(SERVICE_ADDR);

        let services_path = body.listen.unwrap().links.unwrap().messages;

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
                return gsb_resp;
            },
            async {
                println!("WS sleep");
                std::thread::sleep(Duration::from_millis(100));

                println!("WS connect");
                let mut ws_frames = api.ws_at(&services_path).await.unwrap();

                println!("WS next");
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
                
                println!("WS send");
                let ws_res = ws_frames
                    .send(ws::Message::Binary(Bytes::from(ws_res)))
                    .await;
                
                println!("WS sent");
                return ws_res;
            }
        );

        let _ = ws_res.unwrap();
        let gsb_res = gsb_res.unwrap().unwrap();
        assert_eq!(gsb_res.content, vec![7; PAYLOAD_LEN]);

        verify_delete_service(&mut api, SERVICE_ADDR).await;
    }

    #[actix_web::test]
    async fn gsb_msgs_after_ws_disconnect_buffering_test() {
        panic!("NYI. Scenario: service POST, then ws GET, then ws disconnet, then send GSB messages, then GET ws");
    }

    #[actix_web::test]
    async fn gsb_error_on_delete_test() {
        panic!("NYI. Respond with GSB error on pending msg after API Delete of service");
    }

    #[actix_web::test]
    async fn gsb_buffered_msgs_errors_on_delete_test() {
        panic!("NYI. Rrespond with GSB errors on buffered msgs after API Delete of service");
    }

    #[actix_web::test]
    async fn close_old_ws_connection_on_new_ws_connection() {
        panic!()
    }

    fn dummy_auth() -> DummyAuth {
        let id = Identity {
            identity: NodeId::default(),
            name: "dummy_node".to_string(),
            role: "dummy".to_string(),
        };
        DummyAuth::new(id)
    }
}
