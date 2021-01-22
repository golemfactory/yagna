use metrics::counter;
use structopt::StructOpt;

use ya_core_model::version;
use ya_service_api::{CliCtx, CommandOutput};
use ya_service_bus::{typed as bus, RpcEndpoint};

const UPDATE_CURL: &'static str = "curl -sSf https://join.golem.network/as-provider | bash -";
const SILENCE_CMD: &'static str = "yagna version skip";

#[derive(thiserror::Error, Debug, Clone)]
pub(crate) enum ReleaseMessage<'a> {
    #[error("New Yagna release is available: '{}' (v{}).\n\
    Update via\n\t`{}`\nor skip\n\t`{}`", .0.name, .0.version, UPDATE_CURL, SILENCE_CMD)]
    Available(&'a version::Release),
    #[error("Your Yagna is up to date -- '{}' (v{})", .0.name, .0.version)]
    UpToDate(&'a version::Release),
    #[error("Release skipped: '{}' (v{})", .0.name, .0.version)]
    Skipped(&'a version::Release),
    #[error("No pending release to skip")]
    NotSkipped,
}

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
            UpgradeCLI::Show => show(version::Get::show_only(), ctx).await,
            UpgradeCLI::Check => show(version::Get::with_check(), ctx).await,
            UpgradeCLI::Skip => CommandOutput::object(
                match bus::service(version::BUS_ID)
                    .send(version::Skip())
                    .await??
                {
                    Some(r) => {
                        counter!("version.skip", 1);
                        ReleaseMessage::Skipped(&r).to_string()
                    }
                    None => ReleaseMessage::NotSkipped.to_string(),
                },
            ),
        }
    }
}

async fn show(msg: version::Get, ctx: &CliCtx) -> anyhow::Result<CommandOutput> {
    let version_info = bus::service(version::BUS_ID).send(msg).await??;
    if ctx.json_output {
        return CommandOutput::object(version_info);
    }
    CommandOutput::object(match &version_info.pending {
        Some(r) => ReleaseMessage::Available(r).to_string(),
        None => ReleaseMessage::UpToDate(&version_info.current).to_string(),
    })
}
