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
    /// Network status
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
                CommandOutput::object(bus::service(model::BUS_ID).send(model::Status {}).await??)
            }
            NetCommand::Sessions {} => {
                let mut sessions: Vec<model::SessionResponse> = bus::service(model::BUS_ID)
                    .send(model::Sessions {})
                    .await
                    .map_err(|e| anyhow::Error::msg(e))??;

                sessions.sort_by_key(|s| s.node_id.unwrap_or_default().into_array());

                Ok(ResponseTable {
                    columns: vec![
                        "node".into(),
                        "address".into(),
                        "type".into(),
                        "time".into(),
                    ],
                    values: sessions
                        .into_iter()
                        .map(|s| {
                            let time = Duration::from_secs(s.duration.as_secs());
                            serde_json::json! {[
                                s.node_id.map(|id| id.to_string()).unwrap_or_default(),
                                s.remote_address.to_string(),
                                s.session_type,
                                format_duration(time).to_string(),
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
                        "socket".into(),
                        "address".into(),
                        "port".into(),
                        "state".into(),
                    ],
                    values: sockets
                        .into_iter()
                        .map(|s| {
                            serde_json::json! {[
                                format!("{}:{}", s.protocol, s.local_port),
                                s.remote_addr,
                                s.remote_port,
                                s.state,
                            ]}
                        })
                        .collect(),
                }
                .into())
            }
        }
    }
}
