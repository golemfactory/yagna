use anyhow::{bail, Context, Result};
use futures::{future::FutureExt, select};
use std::{io, process::Stdio};
use tokio::{io::AsyncBufReadExt, net::TcpStream, process::Command, time};
use url::Url;
use ya_service_api_web::{DEFAULT_YAGNA_API_URL, YAGNA_API_URL_ENV_VAR};

async fn wait_for_socket(addr: std::net::SocketAddr) -> Result<()> {
    let mut timeout_remaining = 3;

    loop {
        match TcpStream::connect(addr).await {
            Ok(_) => {
                log::info!("socket found, addr: {}", addr);
                return Ok(());
            }
            Err(err) => match err.kind() {
                io::ErrorKind::ConnectionRefused => {
                    log::info!("Waiting for socket ...");
                    if timeout_remaining > 0 {
                        time::delay_for(time::Duration::from_secs(1)).await;
                        timeout_remaining -= 1;
                    } else {
                        bail!("Could not connect to the bus socket");
                    }
                }
                _ => break Err(err.into()),
            },
        }
    }
}

async fn reader_to_log<T: tokio::io::AsyncRead + Unpin>(name: String, reader: T) {
    let mut reader = tokio::io::BufReader::new(reader);
    let mut buf = Vec::new();
    loop {
        match reader.read_until(b'\n', &mut buf).await {
            Ok(len) => {
                if len > 0 {
                    log::debug!(
                        "{}: {}",
                        name,
                        String::from_utf8_lossy(&strip_ansi_escapes::strip(&buf).unwrap())
                            .trim_end()
                    );
                    buf.clear();
                } else {
                    break;
                }
            }
            Err(e) => {
                log::error!("{} output error: {}", name, e);
            }
        }
    }
}

fn spawn(name: &str, command: &mut Command) -> Result<tokio::process::Child> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("Failed to spawn {}", name))?;

    // TODO: redirect output to files or something
    tokio::spawn(reader_to_log(
        format!("{} stdout", name),
        child.stdout.take().unwrap(),
    ));
    tokio::spawn(reader_to_log(
        format!("{} stderr", name),
        child.stderr.take().unwrap(),
    ));

    Ok(child)
}

fn handle_ctrl_c(result: io::Result<()>) -> Result</*exit code*/ i32> {
    if result.is_ok() {
        log::info!("Got ctrl+c. Bye!");
    }
    result.context("Couldn't listen to signals")?;
    Ok(0)
}

fn handle_subprocess(
    name: &str,
    result: io::Result<std::process::ExitStatus>,
) -> Result</*exit code*/ i32> {
    match result {
        Ok(exit_status) => {
            bail!("{} exited too early, {}", name, exit_status);
        }
        Err(e) => {
            bail!("Failed to spawn {}: {}", name, e);
        }
    }
}

pub async fn run(accept_terms: bool) -> Result</*exit code*/ i32> {
    let mut command = Command::new("yagna");
    command.arg("service").arg("run");
    if accept_terms {
        command.arg("--accept-terms");
    }
    let service = spawn("yagna service", &mut command)?;

    // otherwise ya-provider will bail out saying it cannot connect to yagna api
    wait_for_socket(
        *Url::parse(
            &std::env::var(YAGNA_API_URL_ENV_VAR)
                .unwrap_or_else(|_| DEFAULT_YAGNA_API_URL.to_string()),
        )
        .context("Failed to parse yagna API URL")?
        .socket_addrs(|| None)
        .context("Failed to resolve yagna API URL")?
        .get(0)
        .unwrap(),
    )
    .await?;

    let provider = spawn("ya-provider", Command::new("ya-provider").arg("run"))?;

    let ctrl_c = tokio::signal::ctrl_c();

    select!(
        result = ctrl_c.fuse() => handle_ctrl_c(result),
        result = service.fuse() => handle_subprocess("yagna service", result),
        result = provider.fuse() => handle_subprocess("ya-provider", result),
    )
}
