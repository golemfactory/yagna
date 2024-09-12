use std::collections::BTreeMap;
// External crates
use anyhow::anyhow;
use chrono::{DateTime, Utc};
use erc20_payment_lib::rpc_pool::{VerifyEndpointResult, Web3ExternalSources, Web3FullNodeData};
use serde_json::json;
use std::str::FromStr;
use ya_core_model::payment::local::{AccountCli, DriverName, NetworkName};

// Workspace uses
use ya_core_model::payment::local as pay;
use ya_service_api::{CliCtx, CommandOutput};
use ya_service_bus::typed as bus;

use crate::cli::resolve_address;
use structopt::StructOpt;

#[derive(StructOpt, Debug, Clone)]
pub struct RpcCommandParams {
    #[structopt(long, help = "Show info for all networks")]
    pub all: bool,

    #[structopt(long, help = "Additionally force check endpoints")]
    pub verify: bool,

    #[structopt(long, help = "Additionally force resolve sources")]
    pub resolve: bool,

    #[structopt(long, help = "Don't wait for check or resolve")]
    pub no_wait: bool,
}

pub fn run_command_rpc_entry(
    driver: &DriverName,
    network: &NetworkName,
    sources: &Option<&Web3ExternalSources>,
    node_infos: &Vec<Web3FullNodeData>,
    ctx: &CliCtx,
    params: &RpcCommandParams,
) -> CommandOutput {
    let mut values = Vec::with_capacity(node_infos.len());
    let last_chosen_el = node_infos
        .iter()
        .max_by_key(|node| node.info.last_chosen.unwrap_or(DateTime::<Utc>::MIN_UTC));

    for node in node_infos {
        let mut ping_ms = "".to_string();
        let seconds_behind = match &node.info.verify_result {
            Some(ver) => match ver {
                VerifyEndpointResult::Ok(res) => {
                    ping_ms = format!(" / ({:.2}ms)", res.check_time_ms);
                    format!("{}s", res.head_seconds_behind)
                }
                VerifyEndpointResult::NoBlockInfo => "N/A".to_string(),
                VerifyEndpointResult::WrongChainId => "ChainID mismatch".to_string(),
                VerifyEndpointResult::RpcWeb3Error(err) => "Err-rpc".to_string(),
                VerifyEndpointResult::OtherNetworkError(one) => "Err-Other".to_string(),
                VerifyEndpointResult::HeadBehind(beh) => {
                    format!("Behind - {}", beh)
                }
                VerifyEndpointResult::Unreachable => "Unreachable".to_string(),
            },
            None => "-".to_string(),
        };

        let is_last_used = if let (Some(ll), Some(lr)) = (node.info.last_chosen, last_chosen_el) {
            if Some(ll) == lr.info.last_chosen {
                "Y"
            } else {
                "N"
            }
        } else {
            "N"
        };

        let source_id = node.params.source_id;
        let source_info = if let Some(sources) = sources {
            if let Some(source_id) = source_id {
                let mut source_info = None;
                for source in &sources.dns_sources {
                    if source.unique_source_id == source_id {
                        source_info = Some(format!("dns source:\n{}", source.dns_url));
                    }
                }
                for source in &sources.json_sources {
                    if source.unique_source_id == source_id {
                        source_info = Some(format!("json source:\n{}", source.url));
                    }
                }
                source_info.unwrap_or("not found".to_string())
            } else {
                "config".to_string()
            }
        } else {
            "N/A".to_string()
        };

        let v = [
            format!("{}\n({})", node.params.name, node.params.endpoint),
            format!(
                "{} / {}",
                if node.info.is_allowed { "Y" } else { "N" },
                is_last_used
            ),
            node.info
                .last_verified
                .map(|t| t.to_string().as_str()[0..19].to_string())
                .unwrap_or("-".to_string()),
            format!("{}{}", seconds_behind, ping_ms),
            source_info,
        ];
        values.push(json!(v));
    }
    CommandOutput::Table {
        columns: [
            "Name\n(URL)",
            "Active\n/ Leading",
            "Last Verified",
            "Head seconds\nbehind / ping",
            "Source",
        ]
        .iter()
        .map(ToString::to_string)
        .collect(),
        values,
        summary: vec![json!(["", "", "", "", ""])],
        header: Some(format!(
            "Web3 RPC endpoints for driver {} and network {}",
            driver, network
        )),
    }
}

pub async fn run_command_rpc(
    ctx: &CliCtx,
    account: AccountCli,
    params: RpcCommandParams,
) -> anyhow::Result<CommandOutput> {
    let address = resolve_address(account.address()).await?;
    let driver = DriverName::from_str(&account.driver())
        .map_err(|e| anyhow::anyhow!("Invalid driver name: {}. Error: {}", account.driver(), e))?;

    //let network = network.to_string();
    if driver != DriverName::Erc20 {
        log::error!("Only ERC20 driver is supported for now");
        return Err(anyhow::anyhow!(
            "Only ERC20 driver is supported for this command"
        ));
    }

    let result = bus::service(pay::BUS_ID)
        .call(pay::GetRpcEndpoints {
            address,
            driver: driver.clone(),
            network: if params.all {
                None
            } else {
                Some(account.network)
            },
            verify: params.verify,
            resolve: params.resolve,
            no_wait: params.no_wait,
        })
        .await??;

    let endpoints: BTreeMap<String, Vec<Web3FullNodeData>> =
        serde_json::from_value(result.endpoints).unwrap();
    let sources: BTreeMap<String, Web3ExternalSources> =
        serde_json::from_value(result.sources).unwrap();
    if ctx.json_output {
        return CommandOutput::object(json!({"endpoints": endpoints, "sources": sources}));
    }

    let v = endpoints
        .iter()
        .map(|(network, node_infos)| {
            let sources = sources.get(network);
            let network = NetworkName::from_str(network)
                .map_err(|_| anyhow!("Invalid network name {network}"))?;
            Ok(run_command_rpc_entry(
                &driver, &network, &sources, node_infos, ctx, &params,
            ))
        })
        .collect::<anyhow::Result<Vec<CommandOutput>>>()?;

    Ok(CommandOutput::MultiTable { tables: v })
}
