use anyhow::Result;
use env_logger::{Builder, Env, Target};
use gftp::rpc::{
    BenchmarkCommands, RpcBody, RpcId, RpcMessage, RpcRequest, RpcResult, RpcStatusResult,
};

use structopt::{clap, StructOpt};
use tokio::io;
use tokio::io::AsyncBufReadExt;
use tokio::time::Duration;

#[derive(StructOpt)]
#[structopt(version = ya_compile_time_utils::version_describe!())]
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
    Command(RpcRequest),
    /// Starts in JSON RPC server mode
    Server,
}

#[derive(Debug, Clone, Copy)]
enum ExecMode {
    OneShot,
    Service,
    Shutdown,
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
        RpcRequest::Version {} => {
            let version = ya_compile_time_utils::version_describe!().to_string();
            RpcMessage::response(id, RpcResult::String(version)).print(verbose);
            ExecMode::OneShot
        }
        RpcRequest::Benchmark(benchmark_commands) => match benchmark_commands {
            BenchmarkCommands::Publish => {
                let result = gftp::publish_benchmark("benchmark").await?;
                RpcMessage::benchmark_response(id, result).print(verbose);
                ExecMode::Service
            }
            BenchmarkCommands::Download(bench_options) => {
                gftp::download_benchmark_from_url(&bench_options.url, &bench_options).await?;
                ExecMode::OneShot
            }
        },
        RpcRequest::Publish { files } => {
            let mut result = Vec::new();
            if files.is_empty() {
                log::error!("Empty file list provided.");
                return Err(anyhow::anyhow!("Empty file list provided."));
            }
            for file in files {
                let url = gftp::publish(&file).await?;
                result.push((file, url));
            }
            match result.len() {
                0 => RpcMessage::request_error(id),
                _ => RpcMessage::files_response(id, result),
            }
            .print(verbose);
            ExecMode::Service
        }
        RpcRequest::Close { urls } => {
            let mut statuses = Vec::with_capacity(urls.len());
            for url in urls {
                let result = gftp::close(&url).await?;
                statuses.push(result.into())
            }
            match statuses.len() {
                0 => RpcMessage::request_error(id),
                _ => RpcMessage::response(id, RpcResult::Statuses(statuses)),
            }
            .print(verbose);
            ExecMode::OneShot
        }
        RpcRequest::Download { url, output_file } => {
            gftp::download_from_url(&url, &output_file).await?;
            RpcMessage::file_response(id, output_file, url).print(verbose);
            ExecMode::OneShot
        }
        RpcRequest::Receive { output_file } => {
            let url = gftp::open_for_upload(&output_file).await?;
            RpcMessage::file_response(id, output_file, url).print(verbose);
            ExecMode::Service
        }
        RpcRequest::Upload { file, url } => {
            gftp::upload_file(&file, &url).await?;
            RpcMessage::file_response(id, file, url).print(verbose);
            ExecMode::OneShot
        }
        RpcRequest::Shutdown {} => {
            RpcMessage::response(id, RpcResult::Status(RpcStatusResult::Ok)).print(verbose);
            ExecMode::Shutdown
        }
    };

    Ok(exec_mode)
}

async fn server_loop() {
    let mut reader = io::BufReader::new(io::stdin());
    let mut buffer = String::new();
    let verbose = true;

    loop {
        let string = match reader.read_line(&mut buffer).await {
            Ok(read) => match read {
                0 => break,
                _ => match buffer.trim().is_empty() {
                    true => continue,
                    _ => std::mem::take(&mut buffer),
                },
            },
            Err(error) => {
                log::error!("Error reading from stdin: {:?}", error);
                break;
            }
        };

        match serde_json::from_str::<RpcMessage>(&string) {
            Ok(msg) => {
                let id = msg.id.clone();
                if let Err(error) = msg.validate() {
                    RpcMessage::error(id.as_ref(), error).print(verbose);
                    continue;
                }
                match msg.body {
                    RpcBody::Request { request } => {
                        tokio::task::spawn_local(async move {
                            if let ExecMode::Shutdown = execute(id, request, verbose).await {
                                tokio::time::sleep(Duration::from_secs(1)).await;
                                std::process::exit(0);
                            }
                        });
                    }
                    _ => RpcMessage::request_error(id.as_ref()).print(verbose),
                }
            }
            Err(err) => RpcMessage::error(None, err).print(verbose),
        }
    }
}

#[actix_rt::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();

    let mut builder = Builder::from_env(Env::new());
    builder.target(Target::Stderr);
    builder.init();

    let args = Args::from_args();
    match args.command {
        Command::Command(request) => match execute(None, request, args.verbose).await {
            ExecMode::Service => actix_rt::signal::ctrl_c().await?,
            _ => log::debug!("Shutting down"),
        },
        Command::Server => server_loop().await,
    }

    Ok(())
}
