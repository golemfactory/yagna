use actix_web::http::header;
use actix_web::{middleware, web, App, HttpResponse, HttpServer};

use awc::Client;
use futures::TryFutureExt;
use structopt::StructOpt;

use ya_core_model::identity as idm;
use ya_persistence::executor::DbExecutor;
use ya_service_api::{CliCtx, CommandOutput};
use ya_service_api_derive::services;
use ya_service_api_web::middleware::cors::{AppKeyCors, CorsConfig};
use ya_service_api_web::{middleware::auth, rest_api_addr, rest_api_url};
use ya_service_bus::RpcEndpoint;

#[services(DbExecutor)]
enum Service {
    #[enable(gsb)]
    Identity(ya_identity::service::Identity),
}

async fn response() -> HttpResponse {
    let body = "Works fine".to_string();
    HttpResponse::Ok().body(body)
}

async fn server() -> anyhow::Result<()> {
    let db = DbExecutor::new(":memory:")?;
    ya_sb_router::bind_gsb_router(None).await?;
    Service::gsb(&db).await?;

    let cors = AppKeyCors::new(&CorsConfig::default()).await?;
    HttpServer::new(move || {
        App::new()
            .wrap(middleware::Logger::default())
            .wrap(auth::Auth::new(cors.cache()))
            .service(web::resource("/").route(web::get().to(response)))
    })
    .bind(rest_api_addr())?
    .run()
    .await?;

    Ok(())
}

#[derive(StructOpt)]
enum ClientCommand {
    CreateKey { name: String },
    Request { key: String },
}

#[derive(StructOpt)]
enum Command {
    Server,
    Client(ClientCommand),
}

fn map_err<E: std::fmt::Debug>(e: E) -> anyhow::Error {
    anyhow::anyhow!("{:?}", e)
}

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    match Command::from_args() {
        Command::Server => server().await?,
        Command::Client(cmd) => {
            use ya_core_model::appkey as model;
            use ya_service_bus::typed as bus;

            match cmd {
                ClientCommand::CreateKey { name } => {
                    let identity = bus::service(idm::BUS_ID)
                        .send(idm::Get::ByDefault)
                        .await
                        .map_err(map_err)?
                        .map_err(map_err)?
                        .ok_or_else(|| anyhow::Error::msg("Identity not found"))?
                        .node_id;

                    let create = model::Create {
                        name,
                        role: model::DEFAULT_ROLE.to_string(),
                        identity,
                        allow_origins: vec![],
                    };

                    let app_key = bus::service(model::BUS_ID)
                        .send(create)
                        .await
                        .map_err(map_err)?
                        .map_err(map_err)?;

                    println!("{:?}", app_key);
                }
                ClientCommand::Request { key } => {
                    let mut resp = Client::default()
                        .get(rest_api_url().to_string())
                        .insert_header((header::AUTHORIZATION, format!("Bearer {}", key)))
                        .send()
                        .map_err(map_err)
                        .await?;

                    let status = resp.status();
                    let body = resp.body().map_err(map_err).await?.to_vec();

                    println!("{}\n{}", status, String::from_utf8(body)?);
                }
            }
        }
    }

    Ok(())
}
