use actix_web::{web, App, HttpServer, Responder};
use std::collections::HashMap;

fn get_messages() -> impl Responder {
    ""
}

fn send_message() -> impl Responder {
    ""
}

fn authorize() -> impl Responder {
    ""
}

fn deactivate_authorization() -> impl Responder {
    ""
}

fn main() {
    let mut data_for_node: HashMap<String, String> = HashMap::new();
    let _ = HttpServer::new(|| {
        App::new()
            .service(
                web::resource("/message")
                    .route(web::get().to(get_messages))
                    .route(web::post().to(send_message)),
            )
            .service(
                web::resource("/auth")
                    .route(web::post().to(authorize))
                    .route(web::delete().to(deactivate_authorization)),
            )
    })
    .bind("127.0.0.1:8080")
    .unwrap()
    .run();
}
