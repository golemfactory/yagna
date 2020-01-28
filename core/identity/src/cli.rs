mod appkey;
mod identity;

pub use appkey::AppKeyCommand;
pub use identity::IdentityCommand;
use structopt::StructOpt;
use ya_service_api::{CliCtx, CommandOutput};

#[derive(StructOpt, Debug)]
pub enum Command {
    #[structopt(setting = structopt::clap::AppSettings::DeriveDisplayOrder)]
    AppKey(AppKeyCommand),
    #[structopt(setting = structopt::clap::AppSettings::DeriveDisplayOrder)]
    Identity(IdentityCommand),
}

impl Command {
    pub async fn run_command(self, ctx: &CliCtx) -> anyhow::Result<CommandOutput> {
        match self {
            Command::AppKey(command) => command.run_command(ctx).await,
            Command::Identity(command) => command.run_command(ctx).await,
        }
    }
}
