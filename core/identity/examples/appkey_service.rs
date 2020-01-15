use structopt::StructOpt;
use ya_identity::cli::AppKeyCommand;
use ya_persistence::executor::DbExecutor;
use ya_service_api::CliCtx;

#[derive(StructOpt)]
enum Args {
    Server,
    Client(AppKeyCommand),
}

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    std::env::set_var("RUST_LOG", "debug");
    env_logger::init();
    let args = Args::from_args();

    match args {
        Args::Server => {
            ya_sb_router::bind_router("127.0.0.1:8245".parse()?).await?;
            //service::bind_gsb(Arc::new(Mutex::new(DbExecutor::from_env()?)));
            actix_rt::signal::ctrl_c().await?;
            log::info!("SIGINT received, exiting");
        }
        Args::Client(cmd) => {
            cmd.run_command(&CliCtx::default()).await?;
        }
    }
    Ok(())
}
