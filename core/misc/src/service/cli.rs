use structopt::{StructOpt};

use ya_core_model::misc;
use ya_service_api::{CliCtx, CommandOutput};
use ya_service_bus::{typed as bus, RpcEndpoint};

use anyhow::anyhow;
use ya_core_model::misc::HealthInfo;
use serde::{Serialize, Deserialize};
use chrono::Utc;
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
            MiscCLI::Show => show(misc::MiscGet::show_only(), ctx).await,
            MiscCLI::Check => show(misc::MiscGet::with_check(), ctx).await,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct JsonResponse {
    success: bool,
    value: Option<HealthInfo>,
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct HumanFriendlyResponse {
    message: String
}


async fn show(msg: misc::MiscGet, ctx: &CliCtx) -> anyhow::Result<CommandOutput> {
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


            if ctx.json_output {
                let json_response = JsonResponse{error:Some(err.to_string()), success:false, value:None};
                return CommandOutput::object(json_response);
            }
            return Err(anyhow!(err));
        }
    };

    let health_info = match bus_response {
        Ok(health_info) => health_info,
        Err(err) => {
            log::error!("Misc info returned error: {:?}", err);
            if ctx.json_output {
                let json_response = JsonResponse{error:Some(err.to_string()), success:false, value:None};
                return CommandOutput::object(json_response);
            }
            return Err(anyhow!(err));
        }
    };

    if ctx.json_output {
        let json_response = JsonResponse{error:None, success:true, value:Some(health_info)};
        CommandOutput::object(json_response)
    } else {
        let mut string_output = "".to_string();
        string_output += "*********************\n";
        string_output += "*** Health report ***\n";
        string_output += "*********************\n";

        let current_time = Utc::now().timestamp();
        let connected_for : Option<i64> = health_info.last_connected_time.map(|val| {current_time - val});
        let disconnected_for : Option<i64> = health_info.last_disconnnected_time.map(|val| {current_time - val});

        if let (Some(is_net_connected), Some(connected_for)) = (health_info.is_net_connected, connected_for) {
            if is_net_connected == 1 {
                string_output += format!("[OK] - Yagna connected to GSB for {} seconds.\n", connected_for).as_str();
            } else {
                if let Some(disconnected_for) = disconnected_for {
                    string_output += format!("[FAIL] - Yagna disconnected from GSB, disconnected for {} seconds. Last connect {} seconds ago.\n", disconnected_for, connected_for).as_str();
                }
            }
        } else {
            string_output += "[FAIL] - Cannot get info about connection\n";
        }

        let last_healthcheck : Option<i64> = health_info.last_health_check_worker.map(|val| {current_time - val});
        if let Some(last_healthcheck) = last_healthcheck {
            string_output += format!("[OK] - Last healthcheck performed {} seconds ago.\n", last_healthcheck).as_str();
        }



        Ok(CommandOutput::PlainString(string_output))
    }
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
