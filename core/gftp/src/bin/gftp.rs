use actix_rt::Arbiter;
use anyhow::Result;
use futures::future::FutureExt;
use gftp::rpc::{RpcBody, RpcId, RpcMessage, RpcRequest};
use std::mem;
use structopt::{clap, StructOpt};
use tokio::io;
use tokio::io::AsyncBufReadExt;

#[derive(StructOpt)]
struct Args {
    #[structopt(flatten)]
    command: Command,
    /// Increases output verbosity
    #[structopt(
        short,
        long,
        set = clap::ArgSettings::Global,
    )]
    verbose: bool,
}

#[derive(StructOpt)]
#[structopt(global_setting = clap::AppSettings::DeriveDisplayOrder)]
enum Command {
    #[structopt(flatten)]
    Request(RpcRequest),
    /// Starts in JSON RPC server mode
    Server,
}

#[derive(Debug, Clone, Copy)]
enum ExecMode {
    OneShot,
    Service,
}

async fn execute(id: Option<RpcId>, request: RpcRequest, verbose: bool) -> ExecMode {
    let id = id.as_ref();
    match execute_inner(id, request, verbose).await {
        Ok(exec_mode) => exec_mode,
        Err(error) => {
            RpcMessage::error(id, error).print(verbose);
            ExecMode::OneShot
        }
    }
}

async fn execute_inner(id: Option<&RpcId>, request: RpcRequest, verbose: bool) -> Result<ExecMode> {
    let exec_mode = match request {
        RpcRequest::Publish { files } => {
            for file in files {
                let url = gftp::publish(&file).await?;
                RpcMessage::response(id, file, url).print(verbose);
            }
            ExecMode::Service
        }
        RpcRequest::Download { url, output_file } => {
            gftp::download_from_url(&url, &output_file).await?;
            RpcMessage::response(id, output_file, url).print(verbose);
            ExecMode::OneShot
        }
        RpcRequest::AwaitUpload { output_file } => {
            let url = gftp::open_for_upload(&output_file).await?;
            RpcMessage::response(id, output_file, url).print(verbose);
            ExecMode::Service
        }
        RpcRequest::Upload { file, url } => {
            gftp::upload_file(&file, &url).await?;
            RpcMessage::response(id, file, url).print(verbose);
            ExecMode::OneShot
        }
    };

    Ok(exec_mode)
}

fn buffer_to_string(buffer: Vec<u8>) -> String {
    match String::from_utf8(buffer) {
        Ok(utf8) => utf8.to_owned(),
        Err(error) => error
            .as_bytes()
            .into_iter()
            .map(|&c| c as char)
            .collect::<String>(),
    }
    .trim_matches('\0')
    .to_string()
}

#[actix_rt::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();
    env_logger::init();

    let args = Args::from_args();
    let mut verbose = args.verbose;

    match args.command {
        Command::Request(request) => {
            if let ExecMode::Service = execute(None, request, verbose).await {
                actix_rt::signal::ctrl_c().await?;
            }
        }
        Command::Server => {
            let mut reader = io::BufReader::new(io::stdin());
            let mut buffer = Vec::<u8>::new();
            verbose = true;

            loop {
                match reader.read_until(b'\0', &mut buffer).await {
                    Ok(_) => {
                        let string = buffer_to_string(mem::replace(&mut buffer, Vec::new()));
                        match serde_json::from_str::<RpcMessage>(&string) {
                            Ok(msg) => {
                                if let Err(error) = msg.validate() {
                                    RpcMessage::error(msg.id.as_ref(), error).print(verbose);
                                    continue;
                                }
                                match msg.body {
                                    RpcBody::Request { request } => Arbiter::spawn(
                                        execute(msg.id, request, verbose).map(|_| ()),
                                    ),
                                    _ => RpcMessage::type_error(msg.id.as_ref()).print(verbose),
                                }
                            }
                            Err(err) => RpcMessage::error(None, err).print(verbose),
                        }
                    }
                    Err(err) => {
                        buffer.clear();
                        RpcMessage::error(None, err).print(verbose);
                    }
                }
            }
        }
    }
    Ok(())
}
