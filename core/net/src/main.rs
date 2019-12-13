use actix_web::{web, App, HttpResponse, HttpServer, Responder};
use std::collections::{HashMap, VecDeque};
use std::default::Default;
use std::sync::Mutex;
use ya_core_model::net::{Message, MessageAddress, NodeID};

pub const HUB_URL: &str = "localhost:8080";

#[derive(Default)]
struct ServerData {
    messages_to: HashMap<NodeID, VecDeque<Message>>,
}

fn get_messages(state: web::Data<Mutex<ServerData>>, path: web::Path<String>) -> impl Responder {
    let mut server_data = state.lock().unwrap();
    match server_data.messages_to.remove(&path.into_inner()) {
        Some(queue) => HttpResponse::Ok().json(queue),
        None => HttpResponse::NotFound().finish(),
    }
}

fn send_message(
    state: web::Data<Mutex<ServerData>>,
    message: web::Json<Message>,
) -> impl Responder {
    let mut server_data = state.lock().unwrap();
    match &message.destination {
        MessageAddress::Node(node_id) => {
            server_data
                .messages_to
                .entry(node_id.clone())
                .or_default()
                .push_back(message.clone());
        }
        MessageAddress::BroadcastAddress { distance: _d } => {
            for (node_id, v) in server_data.messages_to.iter_mut() {
                if *node_id != message.reply_to {
                    v.push_back(message.clone())
                }
            }
        }
    }
    HttpResponse::Ok()
}

fn authorize() -> impl Responder {
    unimplemented!()
}

fn deactivate_authorization() -> impl Responder {
    unimplemented!()
}

fn main() {
    let server_data = web::Data::new(Mutex::new(ServerData::default()));
    let _ = HttpServer::new(move || {
        App::new()
            .register_data(server_data.clone())
            .service(web::resource("/message/{node_id}").route(web::get().to(get_messages)))
            .service(web::resource("/message").route(web::post().to(send_message)))
            .service(
                web::resource("/auth")
                    .route(web::post().to(authorize))
                    .route(web::delete().to(deactivate_authorization)),
            )
    })
    .bind("localhost:8080")
    .unwrap()
    .run();
}
