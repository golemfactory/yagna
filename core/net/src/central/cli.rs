use ya_core_model::net::local as model;
use ya_service_bus::typed as bus;

pub(crate) fn bind_service() {
    let error =
        model::StatusError::RuntimeException("Not implemented for central network".to_string());

    let err = error.clone();
    let _ = bus::bind(model::BUS_ID, move |_: model::Status| {
        futures::future::err(err.clone())
    });
    let err = error.clone();
    let _ = bus::bind(model::BUS_ID, move |_: model::Sessions| {
        futures::future::err(err.clone())
    });
    let err = error.clone();
    let _ = bus::bind(model::BUS_ID, move |_: model::Sockets| {
        futures::future::err(err.clone())
    });
    let err = error.clone();
    let _ = bus::bind(model::BUS_ID, move |_: model::GsbPing| {
        futures::future::err(err.clone())
    });
}
