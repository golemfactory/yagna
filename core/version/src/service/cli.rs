use structopt::{clap::AppSettings, StructOpt};

use ya_core_model::version;
use ya_service_api::{CliCtx, CommandOutput};
use ya_service_bus::{typed as bus, RpcEndpoint};

const PROVIDER_UPDATE_CMD: &'static str =
    "curl -sSf https://join.golem.network/as-provider | bash -";
const REQUESTOR_UPDATE_CMD: &'static str =
    "curl -sSf https://join.golem.network/as-requestor | bash -";

#[derive(thiserror::Error, Debug, Clone)]
pub(crate) enum ReleaseMessage<'a> {
    #[error("New Yagna {0} is available! Update via `{PROVIDER_UPDATE_CMD}` or `{REQUESTOR_UPDATE_CMD}`")]
    Available(&'a version::Release),
    #[error("Your Yagna is up to date: {0}")]
    UpToDate(&'a version::Release),
    #[error("Release skipped: {0}")]
    Skipped(&'a version::Release),
    #[error("No pending release to skip")]
    NotSkipped,
}

/// Yagna version management.
#[derive(StructOpt, Debug)]
pub enum VersionCLI {
    /// Show current Yagna version and updates if available.
    Show,
    /// Checks if there is new Yagna version available and shows it.
    Check,
    /// Stop logging warnings about latest Yagna release availability.
    #[structopt(setting = AppSettings::Hidden)]
    Skip,
}

impl VersionCLI {
    pub async fn run_command(self, ctx: &CliCtx) -> anyhow::Result<CommandOutput> {
        match self {
            VersionCLI::Show => show(version::Get::show_only(), ctx).await,
            VersionCLI::Check => show(version::Get::with_check(), ctx).await,
            VersionCLI::Skip => CommandOutput::object(
                match bus::service(version::BUS_ID)
                    .send(version::Skip())
                    .await??
                {
                    Some(r) => ReleaseMessage::Skipped(&r).to_string(),
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
                "New Yagna Version 0.6.1 'some code name' released 2015-10-13 is available! Update via \
                `curl -sSf https://join.golem.network/as-provider | bash -` or `curl -sSf https://join.golem.network/as-requestor | bash -`"
            )
        );
    }
}
