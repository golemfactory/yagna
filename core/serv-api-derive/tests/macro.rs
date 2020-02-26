pub struct DbExecutor;
pub struct CommandOutput;
pub struct CliCtx;

pub mod activity {
    pub use super::DbExecutor;

    pub struct ActivityService;

    impl ya_service_api_interfaces::Service for ActivityService {
        type Cli = ();
    }

    impl ActivityService {
        pub async fn gsb(_db: &DbExecutor) -> anyhow::Result<()> {
            Ok(())
        }

        pub fn rest(_db: &DbExecutor) -> actix_web::Scope {
            todo!()
        }
    }
}

pub mod identity {
    pub use super::DbExecutor;
    use structopt::StructOpt;

    pub struct IdentityService;

    impl ya_service_api_interfaces::Service for IdentityService {
        type Cli = Commands;
    }

    impl IdentityService {
        pub async fn gsb(_db: &DbExecutor) -> anyhow::Result<()> {
            Ok(())
        }
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

#[services(DbExecutor)]
enum Services {
    #[enable(gsb, rest)]
    Activity(activity::ActivityService),
    #[enable(cli, gsb)]
    Identity(identity::IdentityService),
}
