use std::cmp::Ordering;
use std::fmt::Display;
use std::time::Duration;

use humantime::format_duration;
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
    /// Ping connected nodes
    Ping {},
}

impl NetCommand {
    pub async fn run_command(self, ctx: &CliCtx) -> anyhow::Result<CommandOutput> {
        let is_json = ctx.json_output;

        match self {
            NetCommand::Status {} => {
                let status = bus::service(model::BUS_ID).send(model::Status {}).await??;

                CommandOutput::object(serde_json::json!({
                    "nodeId": status.node_id,
                    "listenAddress": status.listen_address,
                    "publicAddress": status.public_address,
                    "sessions": status.sessions,
                    "bandwidth": {
                        "outKiBps": to_kib(status.metrics.tx_current, is_json),
                        "outAvgKiBps": to_kib(status.metrics.tx_avg, is_json),
                        "outMib": to_mib(status.metrics.tx_total, is_json),
                        "inKiBps": to_kib(status.metrics.rx_current, is_json),
                        "inAvgKiBps": to_kib(status.metrics.rx_avg, is_json),
                        "inMib": to_mib(status.metrics.rx_total, is_json),
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
                        "ping".into(),
                    ],
                    values: sessions
                        .into_iter()
                        .map(|s| {
                            let seen = Duration::from_secs(s.seen.as_secs());
                            let duration = Duration::from_secs(s.duration.as_secs());
                            let ping = Duration::from_millis(s.ping.as_millis() as u64);

                            serde_json::json! {[
                                s.node_id.map(|id| id.to_string()).unwrap_or_default(),
                                s.remote_address.to_string(),
                                s.session_type,
                                format_duration(seen).to_string(),
                                format_duration(duration).to_string(),
                                format_duration(ping).to_string(),
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
                                to_kib(s.metrics.tx_current, is_json),
                                to_kib(s.metrics.rx_current, is_json),
                            ]}
                        })
                        .collect(),
                }
                .into())
            }
            NetCommand::Ping { .. } => {
                let pings = bus::service(model::BUS_ID)
                    .send(model::GsbPing {})
                    .await
                    .map_err(|e| anyhow::Error::msg(e))??;

                Ok(ResponseTable {
                    columns: vec![
                        "nodeId".into(),
                        "alias".into(),
                        "p2p".into(),
                        "ping (tcp)".into(),
                        "ping (udp)".into(),
                    ],
                    values: pings
                        .into_iter()
                        .map(|s| {
                            let tcp_ping = Duration::from_millis(s.tcp_ping.as_millis() as u64);
                            let udp_ping = Duration::from_millis(s.udp_ping.as_millis() as u64);
                            serde_json::json! {[
                                s.node_id,
                                s.node_alias,
                                s.is_p2p,
                                format_duration(tcp_ping).to_string(),
                                format_duration(udp_ping).to_string(),
                            ]}
                        })
                        .collect(),
                }
                .into())
            }
        }
    }
}

#[inline]
fn to_kib(value: f32, is_json: bool) -> serde_json::Value {
    format_number(value / 1024., is_json)
}

#[inline]
fn to_mib(value: usize, is_json: bool) -> serde_json::Value {
    format_number(value as f64 / (1024. * 1024.), is_json)
}

fn format_number<T>(value: T, is_json: bool) -> serde_json::Value
where
    T: Display,
    f64: From<T>,
{
    let value: f64 = value.into();
    if is_json {
        return serde_json::Value::Number(
            serde_json::Number::from_f64(value).unwrap_or_else(|| serde_json::Number::from(0)),
        );
    }
    serde_json::Value::String(format!("{:.2}", value))
}
