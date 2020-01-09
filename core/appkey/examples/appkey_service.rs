use futures::lock::Mutex;
use std::path::PathBuf;
use structopt::StructOpt;

use ya_appkey::cli::AppKeyCommand;
use ya_appkey::error::Error;
use ya_appkey::service::AppKeyService;
use ya_persistence::executor::DbExecutor;
use ya_service_api::CliCtx;

lazy_static::lazy_static! {
    pub static ref APP_KEY_SERVICE: AppKeyService = {
        let db_executor: DbExecutor<Error> = DbExecutor::from_env().unwrap();
        AppKeyService::new(Mutex::new(db_executor))
    };
}

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
            ya_sb_router::bind_router("127.0.0.1:8245".parse()?).await?;
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
