use structopt::{clap::AppSettings, StructOpt};

use ya_core_model::misc;
use ya_service_api::{CliCtx, CommandOutput};
use ya_service_bus::{typed as bus, RpcEndpoint};


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

async fn show(msg: misc::Get, ctx: &CliCtx) -> anyhow::Result<CommandOutput> {
    let version_info = bus::service(misc::BUS_ID).send(msg).await??;
    if ctx.json_output {
        return CommandOutput::object(version_info);
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
