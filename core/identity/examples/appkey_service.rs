use structopt::StructOpt;
use ya_identity::cli::{AppKeyCommand, IdentityCommand};
use ya_persistence::executor::DbExecutor;
use ya_service_api::{constants::YAGNA_BUS_ADDR, CliCtx};

#[derive(StructOpt)]
enum Args {
    Server,
    ClientAK(AppKeyCommand),
    ClientID(IdentityCommand),
}

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    std::env::set_var("RUST_LOG", "debug");
    env_logger::init();
    let args = Args::from_args();

    match args {
        Args::Server => {
            let db = DbExecutor::new(":memory:")?;
            ya_sb_router::bind_router(*YAGNA_BUS_ADDR).await?;
            ya_identity::service::activate(&db).await?;

            actix_rt::signal::ctrl_c().await?;
            println!();
            log::info!("SIGINT received, exiting");
        }
        Args::ClientAK(cmd) => {
            let ctx = CliCtx::default();
            ctx.output(cmd.run_command(&ctx).await?);
        }
        Args::ClientID(cmd) => {
            let ctx = CliCtx::default();
            ctx.output(cmd.run_command(&ctx).await?);
        }
    }
    Ok(())
}
