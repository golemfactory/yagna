use metrics::counter;
use structopt::StructOpt;

use ya_core_model::version;
use ya_service_api::{CliCtx, CommandOutput};
use ya_service_bus::{typed as bus, RpcEndpoint};

use crate::notifier::ReleaseMessage;

// Yagna version management.
#[derive(StructOpt, Debug)]
pub enum UpgradeCLI {
    /// Stop logging warnings about latest Yagna release availability.
    Skip,
    /// Checks if there is new Yagna version available and shows it.
    Check,
}

impl UpgradeCLI {
    pub async fn run_command(self, _ctx: &CliCtx) -> anyhow::Result<CommandOutput> {
        match self {
            UpgradeCLI::Skip => match bus::service(version::BUS_ID)
                .send(version::Skip {})
                .await??
            {
                Some(r) => {
                    counter!("version.skip", 1);
                    CommandOutput::object(ReleaseMessage::Skipped(&r).to_string())
                }
                None => CommandOutput::object("No pending release to skip."),
            },
            UpgradeCLI::Check => match bus::service(version::BUS_ID)
                .send(version::Check {})
                .await??
            {
                Some(r) => CommandOutput::object(ReleaseMessage::Available(&r).to_string()),
                None => CommandOutput::object(format!(
                    "Your Yagna is up to date -- {}",
                    ya_compile_time_utils::version_describe!()
                )),
            },
        }
    }
}
