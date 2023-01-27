use crate::services::{Bind, Find, Listen, Services, Unbind};
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
    actix_web::web::scope(crate::GSB_API_PATH)
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
        let listen_on = listen.on.clone();
        let bind = Bind {
            components: components.clone(),
            addr_prefix: listen_on.clone(),
        };
        let _ = services.send(bind).await?;
        let listen_on_encoded = base64::encode(&listen_on);
        let services = ServicesBody {
            listen: Some(ServicesListenBody {
                on: listen_on,
                components: components,
                links: Some(ServicesLinksBody {
                    messages: format!("gsb-api/v1/services/{listen_on_encoded}"),
                }),
            }),
        };
        return Ok(web::Json(services)
            .customize()
            .with_status(StatusCode::CREATED));
    }
    Err(GsbApiError::BadRequest)
}

#[actix_web::delete("/services/{key}")]
async fn delete_services(
    path: web::Path<ServicesPath>,
    _id: Identity,
    services: Data<Addr<Services>>,
) -> Result<impl Responder, GsbApiError> {
    log::debug!("DELETE /services/{}", path.key);
    //TODO some prefix/sufix
    let unbind = Unbind {
        addr: path.key.to_string(),
    };
    let _x = services.send(unbind).await??;
    Ok(web::Json(()))
}

#[actix_web::get("/services/{key}")]
async fn get_service_messages(
    path: web::Path<ServicesPath>,
    req: HttpRequest,
    stream: web::Payload,
    services: Data<Addr<Services>>,
) -> Result<impl Responder, GsbApiError> {
    //TODO handle decode error
    let key = base64::decode(&path.key).unwrap();
    let key = String::from_utf8_lossy(&key);
    let service = services
        .send(Find {
            addr: key.to_string(),
        })
        .await??;
    let handler = WsMessagesHandler {
        service: service.clone(),
    };
    let (addr, resp) = ws::WsResponseBuilder::new(handler, &req, stream).start_with_addr()?;
    service.send(Listen { listener: addr }).await??;
    Ok(resp)
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
    use actix::prelude::*;
    use actix_http::ws::Codec;
    use actix_test;
    use actix_web::{body, get, test, web, App, Error, HttpRequest, HttpResponse, Responder};
    use actix_web_actors::ws;
    use bytes::Bytes;
    use futures::{SinkExt, StreamExt};
    use ya_core_model::gftp::GetMetadata;
    use ya_core_model::NodeId;
    use ya_service_api_interfaces::Provider;
    use ya_service_api_web::middleware::auth::dummy::DummyAuth;

    use crate::{GsbApiService, GSB_API_PATH};

    use super::*;
    struct TestContext;
    impl Provider<GsbApiService, ()> for TestContext {
        fn component(&self) -> () {
            todo!("NYI")
        }
    }

    #[actix_web::test]
    async fn happy_path_test() {
        let mut server = actix_test::start(|| {
            App::new()
                .service(GsbApiService::rest(&TestContext {}))
                .wrap(dummy_auth())
        });

        let mut bind_resp = server
            .post(&format!("{}/{}", GSB_API_PATH, "services"))
            .send_json(&ServicesBody {
                listen: Some(ServicesListenBody {
                    components: vec!["GetChunk".to_string()],
                    on: "/public/gftp/123".to_string(),
                    links: None,
                }),
            })
            .await
            .unwrap();

        assert_eq!(bind_resp.status(), StatusCode::CREATED);

        let body = bind_resp.body().await.unwrap();
        let body: ServicesBody = serde_json::de::from_slice(&body.to_vec()).unwrap();
        assert_eq!(
            body,
            ServicesBody {
                listen: Some(ServicesListenBody {
                    components: vec!["GetChunk".to_string()],
                    on: "/public/gftp/123".to_string(),
                    links: Some(ServicesLinksBody {
                        messages: format!(
                            "gsb-api/v1/services/{}",
                            base64::encode("/public/gftp/123")
                        )
                    }),
                })
            }
        );

        let services_path = body.listen.unwrap().links.unwrap().messages;
        let ws_frames = server.ws_at(&services_path).await.unwrap();

        //TODO handle WS msgs in background

        let gsb_endpoint = ya_service_bus::typed::service("/public/gftp/123");

        let resp = gsb_endpoint.call(GetMetadata {}).await;

        // ws_api
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
