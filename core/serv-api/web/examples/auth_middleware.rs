use futures::lock::Mutex;
use std::sync::Arc;

use actix_web::http::header;
use actix_web::{middleware, web, App, HttpRequest, HttpResponse, HttpServer};

use awc::Client;
use futures::TryFutureExt;
use structopt::StructOpt;
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::auth;
use ya_service_bus::RpcEndpoint;

async fn server() -> anyhow::Result<()> {
    let db = Arc::new(Mutex::new(DbExecutor::new(":memory:")?));
    ya_appkey::migrations::run_with_output(&db.lock().await.conn()?, &mut std::io::stdout())?;

    ya_sb_router::bind_router("127.0.0.1:8245".parse()?).await?;
    ya_appkey::service::bind_gsb(db.clone());

    HttpServer::new(move || {
        App::new()
            .wrap(middleware::Logger::default())
            .wrap(auth::Auth::default())
            .service(web::resource("/").route(web::get().to(|req: HttpRequest| {
                let body = format!("{:?}", req);
                HttpResponse::Ok().body(body)
            })))
    })
    .bind("127.0.0.1:8080")?
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
    anyhow::Error::msg(format!("{:?}", e))
}

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    std::env::set_var("RUST_LOG", "debug");
    env_logger::init();

    match Command::from_args() {
        Command::Server => server().await?,
        Command::Client(cmd) => {
            use ya_core_model::appkey as model;
            use ya_service_bus::typed as bus;

            match cmd {
                ClientCommand::CreateKey { name } => {
                    let create = model::Create {
                        name,
                        role: model::DEFAULT_ROLE.to_string(),
                        identity: model::DEFAULT_IDENTITY.to_string(),
                    };

                    let app_key = bus::service(model::APP_KEY_SERVICE_ID)
                        .send(create)
                        .await
                        .map_err(map_err)?
                        .map_err(map_err)?;

                    println!("{:?}", app_key);
                }
                ClientCommand::Request { key } => {
                    let mut resp = Client::default()
                        .get("http://127.0.0.1:8080")
                        .header(header::AUTHORIZATION, key)
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
