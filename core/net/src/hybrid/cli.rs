use std::time::Instant;

use ya_core_model::net::local as model;
use ya_relay_client::ChannelMetrics;
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
            metrics: to_status_metrics(&mut client.metrics()),
        })
    });
    let _ = bus::bind(model::BUS_ID, move |_: model::Sessions| async move {
        let client = CLIENT.with(|c| c.borrow().clone()).ok_or_else(|| {
            model::StatusError::RuntimeException("client not initialized".to_string())
        })?;

        let mut responses = Vec::new();
        let now = Instant::now();

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
                seen: now - session.last_seen,
                duration: now - session.created,
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
            .map(|(desc, mut state)| model::SocketResponse {
                protocol: desc.protocol.to_string().to_lowercase(),
                state: state.to_string(),
                local_port: desc.local.port_repr(),
                remote_addr: desc.remote.addr_repr(),
                remote_port: desc.remote.port_repr(),
                metrics: to_status_metrics(state.inner_mut()),
            })
            .collect();

        Ok(sockets)
    });
}

fn to_status_metrics(metrics: &mut ChannelMetrics) -> model::StatusMetrics {
    let time = Instant::now();
    model::StatusMetrics {
        tx_total: metrics.tx.long.sum() as usize,
        tx_avg: metrics.tx.long.average(time),
        tx_current: metrics.tx.short.average(time),
        rx_total: metrics.rx.long.sum() as usize,
        rx_avg: metrics.rx.long.average(time),
        rx_current: metrics.rx.short.average(time),
    }
}
