use anyhow::Result;
use log::info;
use std::path::PathBuf;
use structopt::StructOpt;

#[derive(StructOpt)]
pub enum CmdLine {
    Publish {
        #[structopt(short = "f", long = "file", help = "File to publish")]
        path: PathBuf,
    },
    Download {
        #[structopt(short = "u", long = "url", help = "File address on gsb")]
        gftp_address: String,
        #[structopt(short = "o", long = "output", help = "Where to place downloaded file")]
        path: PathBuf,
    },
}

#[actix_rt::main]
async fn main() -> Result<()> {
    //std::env::set_var("RUST_LOG", "debug");
    dotenv::dotenv().ok();
    env_logger::init();

    let cmd_args = CmdLine::from_args();

    let config = gftp::Config {
        chunk_size: 40 * 1024,
    };

    match cmd_args {
        CmdLine::Publish { path } => {
            let hash = config.publish(&path).await?;
            info!("Published file [{}], hash [{}].", &path.display(), &hash);

            actix_rt::signal::ctrl_c().await?;
            info!("Received ctrl-c signal. Shutting down.")
        }
        CmdLine::Download { gftp_address, path } => {
            info!(
                "Downloading file [{}], target path [{}].",
                &gftp_address,
                &path.display()
            );

            gftp::download_file(&gftp_address, &path).await?;
            info!("File downloaded.")
        }
    }
    Ok(())
}
