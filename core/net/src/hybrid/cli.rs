use std::time::Instant;
use ya_core_model::net::local as model;
use ya_service_bus::typed as bus;

use crate::hybrid::service::CLIENT;

pub(crate) fn bind_service() {
    let _ = bus::bind(model::BUS_ID, move |_: model::Status| async move {
        let client = {
            CLIENT.with(|c| c.borrow().clone()).ok_or_else(|| {
                model::StatusError::RuntimeException("client not initialized".to_string())
            })?
        };

        Ok(model::StatusResponse {
            node_id: client.node_id(),
            listen_address: client.bind_addr().await.ok(),
            public_address: client.public_addr().await,
            sessions: client.sessions().await.len(),
        })
    });
    let _ = bus::bind(model::BUS_ID, move |_: model::Sessions| async move {
        let client = CLIENT.with(|c| c.borrow().clone()).ok_or_else(|| {
            model::StatusError::RuntimeException("client not initialized".to_string())
        })?;

        let mut responses = Vec::new();
        for session in client.sessions().await {
            let node_id = client.remote_id(&session.remote).await;
            let kind = match node_id {
                Some(id) => {
                    let is_p2p = client.sessions.is_p2p(&id).await;
                    is_p2p.then(|| "p2p").unwrap_or("relay")
                }
                None => "",
            };

            responses.push(model::SessionResponse {
                node_id,
                id: session.id.to_string(),
                session_type: kind.to_string(),
                remote_address: session.remote,
                duration: Instant::now() - session.created,
            });
        }

        Ok(responses)
    });
    let _ = bus::bind(model::BUS_ID, move |_: model::Sockets| async move {
        let client = CLIENT.with(|c| c.borrow().clone()).ok_or_else(|| {
            model::StatusError::RuntimeException("client not initialized".to_string())
        })?;

        let sockets = client
            .sockets()
            .into_iter()
            .map(|(desc, state)| model::SocketResponse {
                protocol: desc.protocol.to_string().to_lowercase(),
                state: state.to_string(),
                local_port: desc.local.port_repr(),
                remote_addr: desc.remote.addr_repr(),
                remote_port: desc.remote.port_repr(),
            })
            .collect();

        Ok(sockets)
    });
}
