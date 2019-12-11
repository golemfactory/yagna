use actix_web::{web, App, HttpServer, Responder};
use std::collections::{HashMap, VecDeque};
use std::default::Default;
use std::sync::Mutex;
use ya_net::{Message, MessageAddress, NodeID};

#[derive(Default)]
struct ServerData {
    messages_to: HashMap<NodeID, VecDeque<Message>>,
}

fn get_messages(state: web::Data<Mutex<ServerData>>, path: web::Path<String>) -> impl Responder {
    let server_data = state.lock().unwrap();
    match server_data.messages_to.get(&path.into_inner()) {
        Some(queue) => format!("{:#?}", queue),
        None => "TODO 404".into(),
    }
}

fn send_message(
    state: web::Data<Mutex<ServerData>>,
    message: web::Json<Message>,
) -> impl Responder {
    eprintln!("Sending... {:?}", message);
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
    "Message sent." /* TODO */
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
    .bind("127.0.0.1:8080")
    .unwrap()
    .run();
}
