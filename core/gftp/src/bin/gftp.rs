use anyhow::Result;
use log::info;
use std::path::PathBuf;
use structopt::StructOpt;
use url::Url;

#[derive(StructOpt)]
pub enum CmdLine {
    Publish { files: Vec<PathBuf> },
    Download { url: Url, output_file: PathBuf },
    Upload { file: PathBuf, url: Url },
    AwaitUpload { filepath: PathBuf },
}

#[actix_rt::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();
    env_logger::init();

    let cmd_args = CmdLine::from_args();

    match cmd_args {
        CmdLine::Publish { files } => {
            for path in files {
                let url = gftp::publish(&path).await?;
                info!("Published file [{}] as {}.", &path.display(), url);
            }

            actix_rt::signal::ctrl_c().await?;
            info!("Received ctrl-c signal. Shutting down.")
        }
        CmdLine::Download { url, output_file } => {
            info!(
                "Downloading file from [{}], target path [{}].",
                url,
                output_file.display()
            );

            gftp::download_from_url(&url, &output_file).await?;
            info!("File downloaded.")
        }
        CmdLine::Upload { file, url } => {
            info!(
                "Uploading file [{}] to address [{}].",
                &file.display(),
                &url
            );
            gftp::upload_file(&file, &url).await?;
        }
        CmdLine::AwaitUpload { filepath } => {
            let url = gftp::open_for_upload(&filepath).await?;
            info!(
                "Waiting for file upload [{}] on url [{}].",
                &filepath.display(),
                &url
            );

            actix_rt::signal::ctrl_c().await?;
            info!("Received ctrl-c signal. Shutting down.")
        }
    }
    Ok(())
}
