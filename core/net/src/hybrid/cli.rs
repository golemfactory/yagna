use anyhow::anyhow;
use futures::future::join_all;
use futures::TryFutureExt;
use std::time::{Duration, Instant};

use ya_core_model::net as ya_net;
use ya_core_model::net::local::{GsbPingResponse, StatusError};
use ya_core_model::net::{local as model, GsbRemotePing, RemoteEndpoint, DIAGNOSTIC};
use ya_relay_client::ChannelMetrics;
use ya_service_bus::timeout::IntoTimeoutFuture;
use ya_service_bus::typed::ServiceBinder;
use ya_service_bus::{typed as bus, RpcEndpoint};

use crate::hybrid::Net;

pub(crate) fn bind_service() {
    let _ = bus::bind(model::BUS_ID, |_: model::GsbPing| {
        cli_ping().map_err(status_err)
    });

    ServiceBinder::new(DIAGNOSTIC, &(), ())
        .bind(move |_, _caller: String, _msg: GsbRemotePing| async move { Ok(GsbRemotePing {}) });

    let _ = bus::bind(model::BUS_ID, move |_: model::Status| {
        async move {
            let client = Net::client().await?;

            Ok(model::StatusResponse {
                node_id: client.node_id().await?,
                listen_address: client.bind_addr().await?,
                public_address: client.public_addr().await?,
                sessions: client.sessions().await?.len(),
                metrics: to_status_metrics(&mut client.metrics().await?),
            })
        }
        .map_err(status_err)
    });
    let _ = bus::bind(model::BUS_ID, move |_: model::Sessions| {
        async move {
            let client = Net::client().await?;
            let mut responses = Vec::new();
            let now = Instant::now();

            for session in client.sessions().await? {
                let node_id = client.remote_id(session.remote).await?;
                let kind = match node_id {
                    Some(id) => {
                        let is_p2p = client.is_p2p(id).await?;
                        is_p2p.then(|| "p2p").unwrap_or("relay")
                    }
                    None => "server",
                };

                responses.push(model::SessionResponse {
                    node_id,
                    id: session.id.to_string(),
                    session_type: kind.to_string(),
                    remote_address: session.remote,
                    seen: now - session.last_seen,
                    duration: now - session.created,
                    ping: session.last_ping,
                });
            }

            Ok(responses)
        }
        .map_err(status_err)
    });
    let _ = bus::bind(model::BUS_ID, move |_: model::Sockets| {
        async move {
            let client = Net::client().await?;

            let sockets = client
                .sockets()
                .await?
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
        }
        .map_err(status_err)
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

pub async fn cli_ping() -> anyhow::Result<Vec<GsbPingResponse>> {
    let client = Net::client().await?;

    // This will update sessions ping. We don't display them in this view
    // but I think it is good place to enforce this.
    client.ping_sessions().await?;

    let nodes = client.connected_nodes().await?;
    let our_node_id = client.node_id().await?;
    let ping_timeout = Duration::from_secs(10);

    log::debug!("Ping: Num connected nodes: {}", nodes.len());

    let mut results = join_all(
        nodes
            .iter()
            .map(|(id, alias)| {
                let target_id = *id;

                async move {
                    let udp_before = Instant::now();

                    ya_net::from(our_node_id)
                        .to(target_id)
                        .service_udp(ya_net::DIAGNOSTIC)
                        .send(GsbRemotePing {})
                        .timeout(Some(ping_timeout))
                        .await???;

                    anyhow::Ok(udp_before.elapsed())
                }
                .map_err(|e| anyhow!("(Udp ping). {}", e))
                .and_then(move |udp_ping| {
                    async move {
                        let tcp_before = Instant::now();

                        ya_net::from(our_node_id)
                            .to(target_id)
                            .service(ya_net::DIAGNOSTIC)
                            .send(GsbRemotePing {})
                            .timeout(Some(ping_timeout))
                            .await???;

                        let tcp_ping = tcp_before.elapsed();

                        anyhow::Ok(GsbPingResponse {
                            node_id: target_id,
                            node_alias: alias.clone(),
                            tcp_ping,
                            udp_ping,
                            is_p2p: false, // Updated later
                        })
                    }
                    .map_err(|e| anyhow!("(Tcp ping). {}", e))
                })
            })
            .collect::<Vec<_>>(),
    )
    .await
    .into_iter()
    .enumerate()
    .map(|(idx, result)| match result {
        Ok(ping) => ping,
        Err(e) => {
            log::warn!("Failed to ping node: {} {}", nodes[idx].0, e);
            GsbPingResponse {
                node_id: nodes[idx].0,
                node_alias: nodes[idx].1,
                tcp_ping: ping_timeout,
                udp_ping: ping_timeout,
                is_p2p: false, // Updated later
            }
        }
    })
    .collect::<Vec<_>>();

    for result in &mut results {
        let main_id = match result.node_alias {
            Some(id) => id,
            None => result.node_id,
        };
        result.is_p2p = client.is_p2p(main_id).await?;
    }
    Ok(results)
}

#[inline]
fn status_err(e: anyhow::Error) -> StatusError {
    StatusError::RuntimeException(e.to_string())
}
