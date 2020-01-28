pub struct DbExecutor;
pub struct CommandOutput;
pub struct CliCtx;

pub mod activity {
    pub use super::DbExecutor;

    pub struct ActivityService;

    impl ya_service_api_interfaces::Service for ActivityService {
        type Db = DbExecutor;
        type Cli = ();
    }
}

pub mod identity {
    pub use super::DbExecutor;
    use structopt::StructOpt;

    pub struct IdentityService;

    impl ya_service_api_interfaces::Service for IdentityService {
        type Db = DbExecutor;
        type Cli = Commands;
    }

    #[derive(StructOpt, Debug)]
    pub enum Commands {
        AppKey(AppKeyCommand),
        Identity(IdentityCommand),
    }

    impl Commands {
        pub async fn run_command(
            self,
            ctx: &super::CliCtx,
        ) -> anyhow::Result<super::CommandOutput> {
            match self {
                Commands::AppKey(command) => command.run_command(ctx).await,
                Commands::Identity(command) => command.run_command(ctx).await,
            }
        }
    }

    #[derive(StructOpt, Debug)]
    pub enum AppKeyCommand {
        Yes,
    }

    impl AppKeyCommand {
        pub async fn run_command(self, _: &super::CliCtx) -> anyhow::Result<super::CommandOutput> {
            Ok(super::CommandOutput {})
        }
    }

    #[derive(StructOpt, Debug)]
    pub enum IdentityCommand {
        Yes,
    }

    impl IdentityCommand {
        pub async fn run_command(self, _: &super::CliCtx) -> anyhow::Result<super::CommandOutput> {
            Ok(super::CommandOutput {})
        }
    }
}

use ya_service_api_derive::services;

#[services]
enum Services {
    #[enable(gsb, rest)]
    Activity(activity::ActivityService),
    #[enable(cli, gsb)]
    Identity(identity::IdentityService),
}
