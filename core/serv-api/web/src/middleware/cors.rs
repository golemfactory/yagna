use actix_cors::Cors;
use actix_web::dev::RequestHead;
use actix_web::error::{Error, ErrorUnauthorized, ParseError};
use actix_web::http::header::HeaderValue;
use actix_web::HttpMessage;
use actix_web_httpauth::headers::authorization::{Bearer, Scheme};

use actix_web::http::header;
use anyhow::anyhow;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use structopt::StructOpt;
use url::Url;

use crate::middleware::auth::resolver::AppKeyResolver;
use crate::middleware::Auth;

use ya_core_model::appkey as model;
use ya_service_api_cache::AutoResolveCache;

pub const BUS_ID: &str = "/local/middleware/cors";

pub type Cache = AutoResolveCache<AppKeyResolver>;

#[derive(Clone, Default, StructOpt)]
pub struct CorsConfig {
    #[structopt(long)]
    allowed_origin: Url,
    /// Set a maximum time (in seconds) for which this CORS request may be cached.
    #[structopt(long, default_value = "3600")]
    max_age: usize,
}

#[derive(Clone, Default)]
pub struct AppKeyCors {
    /// Holds AppKey and Allowed Origins pairs.
    cors: Arc<RwLock<HashMap<String, String>>>,
}

impl AppKeyCors {
    pub fn get(&self, key: &str) -> Option<String> {
        match self.cors.read() {
            Ok(cors) => cors.get(key).cloned(),
            Err(_) => None,
        }
    }

    pub fn update(&self, key: &str, origins: Option<String>) {
        if let Ok(mut cors) = self.cors.write() {
            match origins {
                None => cors.remove(key),
                Some(origins) => cors.insert(key.to_string(), origins.to_string()),
            }
        }
    }

    pub async fn listen_events(&self) -> anyhow::Result<()> {
        let this = self.clone();
        let endpoint = BUS_ID.to_string();

        let _ = bus::bind(&endpoint, move |event: model::event::Event| async move {
            match event {
                model::event::Event::NewKey(appkey) => this.update(&appkey.key, None),
                model::event::Event::DroppedKey(appkey) => this.update(&appkey.key, None),
            };
            Ok(())
        });
        bus::service(model::BUS_ID)
            .send(model::Subscribe { endpoint })
            .await??;
        Ok(())
    }

    pub fn verify_origin(self, header: &HeaderValue, request: &RequestHead) -> bool {
        let key = Bearer::parse(header).ok().map(|b| b.token().to_string());
        match key {
            Some(key) => match appkey_cache.get(&key) {
                None => false,
                Some(origins) => {}
            },
            None => false,
        }
    }
}

pub async fn build_cors(config: CorsConfig) -> anyhow::Result<Cors> {
    let appkey_cache = AppKeyCors::default();
    appkey_cache
        .listen_events()
        .await
        .map_err(|e| anyhow!("Can't build cors middleware: {e}"))?;

    Ok(Cors::default()
        .allowed_origin(&config.allowed_origin.to_string())
        .allowed_origin_fn(move |header, request| appkey_cache.verify_origin(header, request))
        .allowed_methods(vec!["GET", "POST", "DELETE"])
        .allowed_headers(vec![header::AUTHORIZATION, header::ACCEPT])
        .allowed_header(header::CONTENT_TYPE)
        .max_age(config.max_age))
}

//
// pub struct CorsBuilder {
//     cache: Arc<Mutex<Cache>>,
//     //default: Url,
// }
//
// impl CorsBuilder {
//     pub fn from_shared_cache(auth_middleware: Auth) -> CorsBuilder {
//         CorsBuilder {
//             cache: auth_middleware.cache.clone(),
//         }
//     }
// }
//
// impl Default for CorsBuilder {
//     fn default() -> Self {
//         let cache = Arc::new(Mutex::new(Cache::default()));
//         CorsBuilder { cache }
//     }
// }
//
// impl<S, B> Transform<S, ServiceRequest> for CorsBuilder
// where
//     S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
//     S::Future: 'static,
//     B: 'static,
// {
//     type Response = ServiceResponse<B>;
//     type Error = Error;
//     type Transform = CorsMiddlewareWrapper<S>;
//     type InitError = ();
//     type Future = Ready<Result<Self::Transform, Self::InitError>>;
//
//     fn new_transform(&self, service: S) -> Self::Future {
//         ok(CorsMiddlewareWrapper {
//             service: Rc::new(RefCell::new(service)),
//             cache: self.cache.clone(),
//             corses: Arc::new(Default::default()),
//         })
//     }
// }
//
// pub struct CorsMiddlewareWrapper<S> {
//     service: Rc<RefCell<S>>,
//     cache: Arc<Mutex<Cache>>,
//     corses: Arc<Mutex<HashMap<String, CorsMiddleware<S>>>>,
// }
//
// impl<S, B> Service<ServiceRequest> for CorsMiddlewareWrapper<S>
// where
//     S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
//     S::Future: 'static,
// {
//     type Response = ServiceResponse<B>;
//     type Error = Error;
//     type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;
//
//     fn poll_ready(&self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
//         self.service.borrow_mut().poll_ready(cx)
//     }
//
//     fn call(&self, req: ServiceRequest) -> Self::Future {
//         let header = parse_auth::<Bearer, _>(&req)
//             .ok()
//             .map(|b| b.token().to_string());
//
//         let cache = self.cache.clone();
//         let service = self.service.clone();
//
//         Box::pin(async move {
//             match header {
//                 Some(key) => {
//                     let cached = cache.lock().await.get(&key);
//                     let resolved = match cached {
//                         Some(opt) => opt,
//                         None => cache.lock().await.resolve(&key).await,
//                     };
//
//                     match resolved {
//                         Some(app_key) => {
//                             let cors = Cors::default()
//                                 .allowed_origin("https://www.rust-lang.org")
//                                 .allowed_methods(vec!["GET", "POST", "DELETE"])
//                                 .allowed_headers(vec![header::AUTHORIZATION, header::ACCEPT])
//                                 .allowed_header(header::CONTENT_TYPE)
//                                 .max_age(3600);
//                             let cors = cors.new_transform(service).await?
//                         }
//                         None => Err(ErrorUnauthorized("Invalid application key")),
//                     }
//
//                     Err(ErrorUnauthorized("Invalid application key"))
//                 }
//                 None => Err(ErrorUnauthorized("Missing application key")),
//             }
//         })
//     }
// }
