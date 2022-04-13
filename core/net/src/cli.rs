use humantime::format_duration;
use std::cmp::Ordering;
use std::time::Duration;
use structopt::*;

use ya_core_model::net::local as model;
use ya_service_api::{CliCtx, CommandOutput, ResponseTable};
use ya_service_bus::{typed as bus, RpcEndpoint};

#[derive(StructOpt, Debug)]
#[structopt(setting = clap::AppSettings::DeriveDisplayOrder)]
/// Network management
pub enum NetCommand {
    /// Show network status
    Status {},
    /// List network sessions
    Sessions {},
    /// List virtual sockets
    Sockets {},
}

impl NetCommand {
    pub async fn run_command(self, _ctx: &CliCtx) -> anyhow::Result<CommandOutput> {
        match self {
            NetCommand::Status {} => {
                let status = bus::service(model::BUS_ID).send(model::Status {}).await??;

                CommandOutput::object(serde_json::json!({
                    "nodeId": status.node_id,
                    "listenAddress": status.listen_address,
                    "publicAddress": status.public_address,
                    "sessions": status.sessions,
                    "bandwidth": {
                        "out": to_kib(status.metrics.tx_current),
                        "outAvg": to_kib(status.metrics.tx_avg),
                        "outMib": to_mib(status.metrics.tx_total),
                        "in": to_kib(status.metrics.rx_current),
                        "inAvg": to_kib(status.metrics.rx_avg),
                        "inMib": to_mib(status.metrics.rx_total),
                    }
                }))
            }
            NetCommand::Sessions {} => {
                let mut sessions: Vec<model::SessionResponse> = bus::service(model::BUS_ID)
                    .send(model::Sessions {})
                    .await
                    .map_err(|e| anyhow::Error::msg(e))??;

                sessions.sort_by_key(|s| s.node_id.unwrap_or_default().into_array());

                Ok(ResponseTable {
                    columns: vec![
                        "nodeId".into(),
                        "address".into(),
                        "type".into(),
                        "seen".into(),
                        "time".into(),
                    ],
                    values: sessions
                        .into_iter()
                        .map(|s| {
                            let seen = Duration::from_secs(s.seen.as_secs());
                            let duration = Duration::from_secs(s.duration.as_secs());

                            serde_json::json! {[
                                s.node_id.map(|id| id.to_string()).unwrap_or_default(),
                                s.remote_address.to_string(),
                                s.session_type,
                                format_duration(seen).to_string(),
                                format_duration(duration).to_string(),
                            ]}
                        })
                        .collect(),
                }
                .into())
            }
            NetCommand::Sockets {} => {
                let mut sockets: Vec<model::SocketResponse> = bus::service(model::BUS_ID)
                    .send(model::Sockets {})
                    .await
                    .map_err(|e| anyhow::Error::msg(e))??;

                sockets.sort_by(|l, r| match l.remote_addr.cmp(&r.remote_addr) {
                    Ordering::Equal => l.remote_port.cmp(&r.remote_port),
                    result => result,
                });

                Ok(ResponseTable {
                    columns: vec![
                        "type".into(),
                        "port".into(),
                        "to addr".into(),
                        "to port".into(),
                        "state".into(),
                        "out [KiB/s]".into(),
                        "in [KiB/s]".into(),
                    ],
                    values: sockets
                        .into_iter()
                        .map(|s| {
                            serde_json::json! {[
                                s.protocol.to_string(),
                                s.local_port.to_string(),
                                s.remote_addr,
                                s.remote_port,
                                s.state,
                                to_kib(s.metrics.tx_current),
                                to_kib(s.metrics.rx_current),
                            ]}
                        })
                        .collect(),
                }
                .into())
            }
        }
    }
}

/// format floats for display purposes
fn to_kib(value: f32) -> String {
    format!("{:.2}", value / 1024.)
}

fn to_mib(value: usize) -> String {
    format!("{:.2}", value as f64 / (1024. * 1024.))
}
