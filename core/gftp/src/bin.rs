use structopt::StructOpt;
use std::path::PathBuf;
use anyhow::Result;
use log::info;

use gftp::{GftpConfig, GftpService};


#[derive(StructOpt)]
pub enum CmdLine {
    Publish {
        path: PathBuf,
    }
}


#[actix_rt::main]
async fn main() -> Result<()> {
    env_logger::init();

    let cmd_args = CmdLine::from_args();
    match cmd_args {
        CmdLine::Publish {path} => {
            let config = GftpConfig{chunk_size: 4096};
            let gftp_service = GftpService::new(config);

            let hash = GftpService::publish_file(gftp_service, &path).await?;
            info!("Published file [{}], hash [{}].", &path.display(), &hash);
            Ok(())
        }
    }
}

