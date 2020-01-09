use futures::lock::Mutex;
use std::path::PathBuf;
use structopt::StructOpt;

use std::sync::Arc;
use ya_appkey::cli::AppKeyCommand;
use ya_appkey::error::Error;
use ya_appkey::service;
use ya_persistence::executor::DbExecutor;
use ya_service_api::CliCtx;

#[derive(StructOpt)]
enum Args {
    Server,
    Client(AppKeyCommand),
}

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::from_args();

    match args {
        Args::Server => {
            let db_executor: DbExecutor<Error> = DbExecutor::from_env().unwrap();
            // FIXME: bind _after_ router starts
            service::bind_gsb(Arc::new(Mutex::new(db_executor)));
            ya_sb_router::bind_router("127.0.0.1:8245".parse()?)
                .await
                .unwrap();
            eprintln!("done")
        }
        Args::Client(cmd) => {
            let cli_ctx = CliCtx {
                data_dir: PathBuf::new(),
                http_address: ("127.0.0.1".to_string(), 65535),
                router_address: ("127.0.0.1".to_string(), 0),
                json_output: false,
                interactive: false,
            };
            cmd.run_command(&cli_ctx).await?;
        }
    }
    Ok(())
}
