use crate::server::CreateServerArgs;
use crate::{dashboard_serve, forward_gsb, me, redirect_to_dashboard, Services};
use actix_web::{middleware, web, App, HttpServer};
use anyhow::Context;
use metrics::counter;
use std::sync::Arc;
use ya_service_api_web::middleware::auth;

pub fn create_server(args: CreateServerArgs) -> anyhow::Result<actix_web::dev::Server> {
    let CreateServerArgs {
        cors,
        cors_on_auth_failure,
        context,
        number_of_workers,
        rest_address,
        max_rest_timeout,
        api_host_port,
    } = args;
    let count_started = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    Ok(HttpServer::new(move || {
        let app = App::new()
            .wrap(middleware::Logger::default())
            .wrap(auth::Auth::new(cors.cache(), cors_on_auth_failure))
            .wrap(cors.cors())
            .route("/dashboard", web::get().to(redirect_to_dashboard))
            .route("/dashboard/{_:.*}", web::get().to(dashboard_serve))
            .route("/me", web::get().to(me))
            .service(forward_gsb);
        let rest = Services::rest(app, &context);
        if count_started.fetch_add(1, std::sync::atomic::Ordering::Relaxed) == number_of_workers - 1
        {
            log::info!(
                "All {} http workers started - listening on {}",
                number_of_workers,
                rest_address
            );

            counter!("yagna.service.up", 1);

            tokio::task::spawn_local(
                async move { ya_net::hybrid::send_bcast_new_neighbour().await },
            );
        }
        rest
    })
    .workers(number_of_workers)
    // this is maximum supported timeout for our REST API
    .keep_alive(std::time::Duration::from_secs(max_rest_timeout))
    .bind(api_host_port.clone())
    .context(format!("Failed to bind http server on {:?}", api_host_port))?
    .run())
}
