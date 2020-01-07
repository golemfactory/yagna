use actix::prelude::*;
use futures::lock::Mutex;
use std::path::PathBuf;
use structopt::StructOpt;
use ya_appkey::cli::AppKeyCommand;
use ya_appkey::error::Error;
use ya_appkey::service::{bind, AppKeyService};
use ya_persistence::executor::DbExecutor;
use ya_service_api::{CliCtx, Command};
use ya_service_bus::connection;

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

fn main() -> Result<(), anyhow::Error> {
    let bus_addr = "127.0.0.1:8245".parse().unwrap();
    let args = Args::from_args();

    match args {
        Args::Server => {
            System::run(move || {
                let fut = connection::tcp(&bus_addr)
                    .and_then(|_| {
                        bind(&APP_KEY_SERVICE);
                        Ok(())
                    })
                    .map_err(|e| eprintln!("Error: {:?}", e));
                Arbiter::spawn(fut)
            })
            .unwrap();

            eprintln!("done");
            Ok(())
        }
        Args::Client(cmd) => {
            let cli_ctx = CliCtx {
                data_dir: PathBuf::new(),
                address: ("127.0.0.1".to_string(), 65535),
                json_output: false,
                interactive: false,
                sys: std::sync::Mutex::new(Some(System::new("client"))),
            };

            cmd.run_command(&cli_ctx)?.print(false);
            Ok(())
        }
    }
}
