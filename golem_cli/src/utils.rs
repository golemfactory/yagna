use anyhow::{bail, Context, Result};
use tokio::{net::TcpStream, process::Command};
use url::Url;

use ya_core_model::NodeId;

use crate::command::YaCommand;

pub async fn get_command_raw_output(program: &str, args: &[&str]) -> Result<Vec<u8>> {
    let mut command = Command::new(program);
    command.args(args);
    log::debug!("executing {:?} {:?}", program, args);
    let command_output = command
        .output()
        .await
        .with_context(|| format!("Failed to spawn {:?} {:?}", program, args))?;
    if !command_output.status.success() {
        log::debug!("subcommand failed");
        bail!("subcommand failed: {:?}", command);
    }
    log::debug!(
        "subcommand output: {:?}",
        String::from_utf8_lossy(&command_output.stdout)
    );
    Ok(command_output.stdout)
}

pub async fn get_command_output(program: &str, args: &[&str]) -> Result<String> {
    let output = get_command_raw_output(program, args).await?;
    Ok(String::from_utf8(output)?)
}

pub async fn get_command_json_output(program: &str, args: &[&str]) -> Result<serde_json::Value> {
    let output = get_command_raw_output(program, args).await?;
    Ok(serde_json::from_slice(&output)?)
}

pub fn move_string_out_of_json(value: serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(s) => Some(s),
        _ => None,
    }
}

#[cfg(not(unix))]
async fn wait_for_socket(addr: std::net::SocketAddr) -> Result<()> {
    use std::io;
    use tokio::time;

    let mut timeout_remaining = 10;

    loop {
        match TcpStream::connect(addr).await {
            Ok(_) => {
                log::debug!("socket found, addr: {}", addr);
                return Ok(());
            }
            Err(err) => match err.kind() {
                io::ErrorKind::ConnectionRefused => {
                    log::debug!("Waiting for socket ...");
                    if timeout_remaining > 0 {
                        time::delay_for(time::Duration::from_secs(1)).await;
                        timeout_remaining -= 1;
                    } else {
                        bail!("Could not connect to the yagna socket");
                    }
                }
                _ => break Err(err.into()),
            },
        }
    }
}

fn yagna_addr() -> Result<std::net::SocketAddr> {
    Ok(Url::parse(
        &std::env::var("YAGNA_API_URL").unwrap_or_else(|_| "http://127.0.0.1:7465".to_string()),
    )
    .context("Failed to parse yagna API URL")?
    .socket_addrs(|| None)
    .context("Failed to resolve yagna API URL")?
    .drain(..)
    .next()
    .unwrap())
}

#[cfg(not(unix))]
pub async fn wait_for_yagna() -> Result<()> {
    wait_for_socket(yagna_addr()?).await
}

pub async fn is_yagna_running() -> Result<bool> {
    Ok(TcpStream::connect(yagna_addr()?).await.is_ok())
}

pub async fn payment_account(cmd: &YaCommand, address: &Option<NodeId>) -> Result<String> {
    Ok(match address {
        Some(address) => address.to_string(),
        _ => cmd.yagna()?.default_id().await?.node_id.to_string(),
    })
}
