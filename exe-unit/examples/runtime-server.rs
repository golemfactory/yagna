use anyhow::Result;
use futures::{SinkExt, StreamExt};
use rand::distributions::Alphanumeric;
use rand::Rng;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::time::Duration;
use structopt::StructOpt;
use tokio::io::AsyncWriteExt;
use tokio::time::sleep;
use tokio_util::codec::{FramedRead, FramedWrite};
use ya_runtime_api::deploy::{ContainerVolume, DeployResult, StartMode};
use ya_runtime_api::server::proto::{request, response, Request, Response};
use ya_runtime_api::server::{Codec, ErrorResponse};
use ya_utils_path::normalize_path;

// Running this example:
//
// cargo build --bin exe-unit
// cargo build --example runtime-server
//
// cargo run --example exe-unit2gsb -- --supervisor ../target/debug/exe-unit \
//   --runtime ../target/debug/examples/runtime-server \
//   -c /tmp/exe/cache/ \
//   -w /tmp/exe/work/ \
//   -a ./examples/agreement.json \
//   --script ./examples/commands-server.json \

static SEQ: AtomicU64 = AtomicU64::new(1000);

#[allow(unused)]
#[derive(StructOpt)]
enum Commands {
    Deploy {},
    Start {},
}

#[allow(unused)]
#[derive(StructOpt)]
#[structopt(rename_all = "kebab-case")]
struct CmdArgs {
    #[structopt(short, long)]
    workdir: PathBuf,
    #[structopt(short, long)]
    task_package: Option<PathBuf>,
    #[structopt(subcommand)]
    command: Commands,
}

fn rand_name() -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .map(char::from)
        .take(30)
        .collect()
}

async fn deploy(cmdargs: &CmdArgs) -> Result<()> {
    let vols = vec![
        ContainerVolume {
            name: format!("vol-{}", rand_name()),
            path: "/golem/output".to_string(),
        },
        ContainerVolume {
            name: format!("vol-{}", rand_name()),
            path: "/golem/resource".to_string(),
        },
        ContainerVolume {
            name: format!("vol-{}", rand_name()),
            path: "/golem/work".to_string(),
        },
    ];

    let workdir = normalize_path(&cmdargs.workdir)?;
    tokio::fs::create_dir_all(&workdir).await?;
    for vol in vols.iter() {
        tokio::fs::create_dir_all(workdir.join(&vol.name)).await?;
    }

    let res = DeployResult {
        valid: Ok(Default::default()),
        vols,
        start_mode: StartMode::Blocking,
    };

    let mut stdout = tokio::io::stdout();
    let json = format!("{}\n", serde_json::to_string(&res).unwrap());

    stdout.write_all(json.as_bytes()).await.unwrap();
    stdout.flush().await.unwrap();

    Ok(())
}

async fn start() -> Result<()> {
    let mut stdin = FramedRead::new(tokio::io::stdin(), Codec::<Request>::default());
    while let Some(request) = stdin.next().await {
        let (id, command) = match request {
            Ok(request) => match request.command {
                Some(command) => (request.id, command),
                _ => continue,
            },
            _ => continue,
        };

        let mut stop = false;
        let res = match command {
            request::Command::Hello(hello) => response::Command::Hello(response::Hello {
                version: hello.version,
            }),
            request::Command::Run(run) => {
                let pid = SEQ.fetch_add(1, Relaxed);
                tokio::task::spawn_local(mock_process(pid, id, run));
                response::Command::Run(response::RunProcess { pid })
            }
            request::Command::Kill(_) => {
                stop = true;
                response::Command::Kill(Default::default())
            }
            _ => response::Command::Error(ErrorResponse::msg("unknown command")),
        };

        write_response(id, res).await;
        if stop {
            break;
        }
    }

    Ok(())
}

async fn mock_process(pid: u64, id: u64, run: request::RunProcess) {
    let status = response::ProcessStatus {
        pid,
        running: true,
        return_code: 0,
        stdout: format!("Executing {} ({:?}\n", run.bin, run.args)
            .as_bytes()
            .to_vec(),
        stderr: Vec::new(),
    };

    write_status(id, status).await;
    sleep(Duration::from_secs(1)).await;

    let status = response::ProcessStatus {
        pid,
        running: false,
        return_code: 0,
        stdout: format!("Done executing {} ({:?})\n", run.bin, run.args)
            .as_bytes()
            .to_vec(),
        stderr: Vec::new(),
    };

    write_status(id, status).await;
}

async fn write_status(id: u64, status: response::ProcessStatus) {
    let mut response = Response::default();
    response.id = id;
    response.event = true;
    response.command = Some(response::Command::Status(status));
    write(response).await;
}

async fn write_response(id: u64, command: response::Command) {
    let mut response = Response::default();
    response.id = id;
    response.event = false;
    response.command = Some(command);
    write(response).await;
}

async fn write(res: Response) {
    let mut stdout = FramedWrite::new(tokio::io::stdout(), Codec::<Response>::default());
    stdout.send(res).await.unwrap();
    stdout.flush().await.unwrap();
}

#[actix_rt::main]
async fn main() -> Result<()> {
    let cmdargs = CmdArgs::from_args();
    match &cmdargs.command {
        Commands::Deploy { .. } => deploy(&cmdargs).await?,
        Commands::Start { .. } => start().await?,
    }

    Ok(())
}
