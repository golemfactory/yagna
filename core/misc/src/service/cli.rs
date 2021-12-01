use structopt::{clap::AppSettings, StructOpt};

use ya_core_model::misc;
use ya_service_api::{CliCtx, CommandOutput};
use ya_service_bus::{typed as bus, RpcEndpoint};
use ya_service_bus::router_error::{get_last_router_error};

use anyhow::anyhow;
use ya_core_model::misc::MiscInfo;
use serde::{Serialize, Deserialize};

/// Yagna version management.
#[derive(StructOpt, Debug)]
pub enum MiscCLI {
    /// Show current Yagna version and updates if available.
    Show,
    /// Checks if there is new Yagna version available and shows it.
    Check
}

impl MiscCLI {
    pub async fn run_command(self, ctx: &CliCtx) -> anyhow::Result<CommandOutput> {
        match self {
            MiscCLI::Show => show(misc::Get::show_only(), ctx).await,
            MiscCLI::Check => show(misc::Get::with_check(), ctx).await,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct JsonResponse {
    success: bool,
    value: Option<MiscInfo>,
    error: Option<String>,
    router_error: Option<String>,
}

async fn show(msg: misc::Get, ctx: &CliCtx) -> anyhow::Result<CommandOutput> {
    let bus_endpoint = bus::service(misc::BUS_ID); /*{
        Ok(bus_endpoint) => bus_endpoint,
        Err(err) => {
            log::error!("Bus endpoint cannot be created");
            return Err(err);
        }
    };*/
    let bus_response = match bus_endpoint.send(msg).await {
        Ok(bus_response) => bus_response,
        Err(err) => {
            log::error!("Error when sending message to bus: {:?}", err);

            let last_error = get_last_router_error();

            if ctx.json_output {
                let jsonResponse = JsonResponse{error:Some(err.to_string()), router_error: last_error, success:false, value:None};
                return CommandOutput::object(jsonResponse);
            }
            return Err(anyhow!(err));
        }
    };

    let misc_info = match bus_response {
        Ok(miscInfo) => miscInfo,
        Err(err) => {
            log::error!("Misc info returned error: {:?}", err);
            if ctx.json_output {
                let json_response = JsonResponse{error:Some(err.to_string()), router_error: None, success:false, value:None};
                return CommandOutput::object(json_response);
            }
            return Err(anyhow!(err));
        }
    };

    if ctx.json_output {
        return CommandOutput::object(misc_info);
    }
    CommandOutput::object("tesciki".to_string())
}

#[cfg(test)]
mod test {
    use super::*;
    use chrono::NaiveDateTime;
    use ya_core_model::version::Release;

    #[test]
    fn test_release_available_to_string() {
        let now = NaiveDateTime::parse_from_str("2015-10-13T15:43:00GMT+2", "%Y-%m-%dT%H:%M:%S%Z")
            .unwrap();
        let r = Release {
            version: "0.6.1".to_string(),
            name: "some code name".to_string(),
            seen: false,
            release_ts: now,
            insertion_ts: None,
            update_ts: None,
        };

        assert_eq!(
            ReleaseMessage::Available(&r).to_string(),
            format!(
                "New Yagna Version 0.6.1 'some code name' released 2015-10-13 is available! Update via `curl -sSf https://join.golem.network/as-provider | bash -`"
            )
        );
    }
}
