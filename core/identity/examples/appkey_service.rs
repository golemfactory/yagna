use structopt::StructOpt;

use ya_identity::cli::{AppKeyCommand, IdentityCommand};
use ya_persistence::executor::DbExecutor;
use ya_service_api::{CliCtx, CommandOutput};
use ya_service_api_derive::services;

#[derive(StructOpt)]
enum Args {
    Server,
    ClientAK(AppKeyCommand),
    ClientID(IdentityCommand),
}

#[services(DbExecutor)]
enum Service {
    #[enable(gsb)]
    Identity(ya_identity::service::Identity),
}

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    std::env::set_var("RUST_LOG", "debug");
    env_logger::init();
    let args = Args::from_args();

    match args {
        Args::Server => {
            let db = DbExecutor::new(":memory:")?;
            ya_sb_router::bind_gsb_router(None).await?;
            Service::gsb(&db).await?;

            actix_rt::signal::ctrl_c().await?;
            println!();
            log::info!("SIGINT received, exiting");
        }
        Args::ClientAK(cmd) => {
            let ctx = CliCtx::default();
            ctx.output(cmd.run_command(&ctx).await?)?;
        }
        Args::ClientID(cmd) => {
            let ctx = CliCtx::default();
            ctx.output(cmd.run_command(&ctx).await?)?;
        }
    }
    Ok(())
}
