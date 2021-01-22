use metrics::counter;
use structopt::StructOpt;

use ya_core_model::version;
use ya_service_api::{CliCtx, CommandOutput};
use ya_service_bus::{typed as bus, RpcEndpoint};

use crate::notifier::ReleaseMessage;

// Yagna version management.
#[derive(StructOpt, Debug)]
pub enum UpgradeCLI {
    /// Show current Yagna version and updates if available.
    Show,
    /// Checks if there is new Yagna version available and shows it.
    Check,
    /// Stop logging warnings about latest Yagna release availability.
    Skip,
}

impl UpgradeCLI {
    pub async fn run_command(self, ctx: &CliCtx) -> anyhow::Result<CommandOutput> {
        match self {
            UpgradeCLI::Show => UpgradeCLI::show(version::Get::show_only(), ctx).await,
            UpgradeCLI::Check => UpgradeCLI::show(version::Get::with_check(), ctx).await,
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
        }
    }

    async fn show(msg: version::Get, ctx: &CliCtx) -> anyhow::Result<CommandOutput> {
        let version_info = bus::service(version::BUS_ID).send(msg).await??;
        if ctx.json_output {
            return CommandOutput::object(version_info);
        }
        match &version_info.pending {
            Some(r) => CommandOutput::object(ReleaseMessage::Available(r).to_string()),
            None => CommandOutput::object(format!(
                "Your Yagna is up to date -- {}",
                ya_compile_time_utils::version_describe!()
            )),
        }
    }
}
