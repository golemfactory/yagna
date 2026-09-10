use crate::appkey;
use crate::command::{YaCommand, DRIVERS, NETWORK_GROUP_MAP};
use crate::setup::RunConfig;
use crate::utils::payment_account;
use anyhow::{Context, Result};
use futures::channel::{mpsc, oneshot};
use futures::prelude::*;
use futures::StreamExt;
use std::io;
use std::process::ExitStatus;
use structopt::StructOpt;
use tokio::process::Child;
use tokio::time::Duration;

const PROVIDER_SHUTDOWN_GRACE_SECS: i64 = 15;

#[derive(StructOpt, Debug)]
pub struct StopConfig {
    /// Graceful stop: deactivate offers, finish computing currently running
    /// tasks, then stop. No new agreements are accepted in the meantime.
    #[structopt(long)]
    pub graceful: bool,
    /// Stop waiting gracefully after this many seconds and begin shutdown
    #[structopt(long, requires = "graceful")]
    pub timeout: Option<u64>,
}

fn handle_ctrl_c(result: io::Result<()>) -> Result<()> {
    if result.is_ok() {
        log::info!("Got ctrl+c. Bye!");
    }
    result.context("Couldn't listen to signals")?;
    Ok(())
}

struct AbortableChild(Option<oneshot::Sender<oneshot::Sender<io::Result<ExitStatus>>>>);

impl AbortableChild {
    fn new(
        mut child: Child,
        mut kill_cmd: mpsc::Sender<()>,
        name: &'static str,
        send_term: bool,
    ) -> Self {
        let (tx, mut rx) = oneshot::channel::<oneshot::Sender<io::Result<ExitStatus>>>();

        #[allow(unused)]
        async fn wait_and_kill(mut child: Child, send_term: bool) -> io::Result<ExitStatus> {
            #[cfg(target_os = "linux")]
            if send_term {
                use ::nix::sys::signal::*;
                use ::nix::unistd::Pid;

                match child.id() {
                    Some(id) => {
                        let _ret = ::nix::sys::signal::kill(Pid::from_raw(id as i32), SIGTERM);
                    }
                    None => log::error!("missing child process pid"),
                }
            }
            // Yagna service should get ~10 seconds to clean up
            match tokio::time::timeout(Duration::from_secs(15), child.wait()).await {
                Ok(r) => r,
                Err(_) => {
                    child.start_kill()?;
                    child.wait().await
                }
            }
        }

        tokio::task::spawn_local(async move {
            let exited_on_its_own = tokio::select! {
                r = child.wait() => {
                    match &r {
                        Ok(status) if status.success() => {
                            log::info!("child {} exited on its own: {:?}", name, status)
                        }
                        _ => log::error!("child {} exited too early: {:?}", name, r),
                    }
                    if kill_cmd.send(()).await.is_err() {
                        log::warn!("unable to send end-of-process notification");
                    }
                    Some(r)
                },
                r = &mut rx => {
                    match r {
                        Ok(tx) => {
                            let _ = tx.send(wait_and_kill(child, send_term).await);
                        },
                        Err(_) => {
                            let _ = wait_and_kill(child, send_term).await;
                        }
                    }
                    None
                }
            };

            // The child is already gone, but abort() may still be called - a
            // graceful `ya-provider` shutdown looks exactly like this. Answer
            // it with the status we collected instead of dropping `rx` and
            // failing it with "process exited too early", which would abandon
            // the other child.
            if let Some(status) = exited_on_its_own {
                if let Ok(tx) = rx.await {
                    let _ = tx.send(status);
                }
            }
        });

        Self(Some(tx))
    }

    async fn abort(&mut self) -> io::Result<ExitStatus> {
        let (tx, rx) = oneshot::channel();
        let _ = self.0.take().unwrap().send(tx);
        rx.await
            .map_err(|_| io::Error::other("process exited too early"))?
    }
}

pub async fn watch_for_vm() -> anyhow::Result<()> {
    let cmd = YaCommand::new()?;
    let presets = cmd.ya_provider()?.list_presets().await?;
    if !presets.iter().any(|p| p.exeunit_name == "vm") {
        return Ok(());
    }
    let mut status = crate::platform::kvm_status();

    cmd.ya_provider()?
        .set_profile_activity("vm", status.is_valid())
        .await
        .ok();

    loop {
        tokio::time::sleep(Duration::from_secs(60)).await;
        let new_status = crate::platform::kvm_status();
        if new_status.is_valid() != status.is_valid() {
            cmd.ya_provider()?
                .set_profile_activity("vm", new_status.is_valid())
                .await
                .ok();
            log::info!("Changed vm status to {:?}", new_status.is_valid());
        }
        status = new_status
    }
}

pub async fn run(config: RunConfig) -> Result</*exit code*/ i32> {
    crate::setup::setup(&config, false).await?;

    let cmd = YaCommand::new()?;

    let service = cmd.yagna()?.service_run(&config).await?;
    let app_key = appkey::get_app_key().await?;

    let provider_config = cmd.ya_provider()?.get_config().await?;
    let address =
        payment_account(&cmd, &config.account.account.or(provider_config.account)).await?;
    for nn in NETWORK_GROUP_MAP[&config.account.network].iter() {
        for driver in DRIVERS.iter() {
            if driver.platform(nn).is_err() {
                continue;
            }

            if let Err(e) = cmd.yagna()?.payment_init(&address, nn, driver).await {
                log::debug!("Failed to initialize {} driver. Error: {e}", driver.name);
            }
        }
    }

    let provider = cmd.ya_provider()?.spawn(&app_key, &config).await?;
    let ctrl_c = tokio::signal::ctrl_c();

    log::info!("Golem provider is running");

    let (event_tx, mut event_rx) = mpsc::channel(1);
    let mut service = AbortableChild::new(service, event_tx.clone(), "yagna", true);
    let mut provider = AbortableChild::new(provider, event_tx, "provider", false);

    futures::pin_mut!(ctrl_c);
    //futures::pin_mut!(event_rx);
    tokio::task::spawn_local(async move {
        if let Err(e) = watch_for_vm().await {
            log::error!("vm checker failed: {:?}", e)
        }
    });

    if let future::Either::Left((r, _)) =
        future::select(ctrl_c, StreamExt::next(&mut event_rx)).await
    {
        let _ignore = handle_ctrl_c(r);
    }

    // The provider may have exited on its own (graceful shutdown). Stop yagna
    // in either case, or it stays behind as an orphan.
    let provider_failed = match provider.abort().await {
        Err(e) => {
            log::warn!("provider exited with: {:?}", e);
            true
        }
        Ok(status) if !status.success() => {
            log::warn!("provider exited with: {:?}", status);
            true
        }
        Ok(_) => false,
    };
    if let Err(e) = service.abort().await {
        log::warn!("service exited with: {:?}", e);
        return Ok(12);
    }
    if provider_failed {
        return Ok(11);
    }
    Ok(0)
}

pub async fn stop(config: StopConfig) -> Result<i32> {
    use ya_utils_path::data_dir::DataDir;
    use ya_utils_process::lock::ProcLock;

    let provider_dir = DataDir::new("ya-provider")
        .get_or_create()
        .expect("unable to get ya-provider data dir");
    let provider_pid = ProcLock::new("ya-provider", &provider_dir)?
        .read_pid()
        .context("ya-provider is not running")?;

    match config.graceful {
        true => graceful_stop_provider(provider_pid, config.timeout)
            .await
            .context("failed to gracefully stop provider")?,
        false => kill_pid(provider_pid, PROVIDER_SHUTDOWN_GRACE_SECS)
            .await
            .context("failed to stop provider")?,
    }

    Ok(0)
}

async fn graceful_stop_provider(pid: u32, timeout: Option<u64>) -> Result<()> {
    use std::time::Instant;

    let cmd = YaCommand::new()?;
    cmd.ya_provider()?.request_shutdown().await?;
    println!(
        "Graceful shutdown requested. Waiting for running tasks to finish within the configured \
         termination grace period..."
    );
    println!("Agreements still active at the announced deadline will be terminated.");

    let started = Instant::now();
    let mut next_progress = Duration::from_secs(10);

    while process_alive(pid) {
        if let Some(secs) = timeout {
            if started.elapsed() >= Duration::from_secs(secs) {
                println!(
                    "Timeout of {secs}s reached, stopping provider with a \
                     {PROVIDER_SHUTDOWN_GRACE_SECS}s cleanup window."
                );
                kill_pid(pid, PROVIDER_SHUTDOWN_GRACE_SECS)
                    .await
                    .context("failed to force-stop provider")?;
                return Ok(());
            }
        }
        if started.elapsed() >= next_progress {
            print_drain_progress(started.elapsed()).await;
            next_progress += Duration::from_secs(10);
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    println!("Provider stopped.");
    Ok(())
}

async fn print_drain_progress(elapsed: Duration) {
    // Best effort only - yagna is still running during the drain, but a
    // failure to get the count must never fail the stop.
    let in_progress = async {
        anyhow::Ok(
            YaCommand::new()?
                .yagna()?
                .activity_status()
                .await?
                .in_progress(),
        )
    }
    .await;

    match in_progress {
        Ok(count) => println!(
            "{} activity(ies) still in progress ({}s elapsed)",
            count,
            elapsed.as_secs()
        ),
        Err(_) => println!(
            "Waiting for tasks to finish ({}s elapsed)",
            elapsed.as_secs()
        ),
    }
}

/// Checks whether a process we don't own is still running.
///
/// A waitpid-based check can't be used here: it reports processes that aren't
/// our children as dead, which is every process `golemsp stop` deals with.
#[cfg(target_family = "unix")]
fn process_alive(pid: u32) -> bool {
    nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid as i32), None).is_ok()
}

/// Terminates the process and waits for it to actually be gone: SIGTERM, then
/// SIGKILL if it is still there after `grace_secs`.
#[cfg(target_family = "unix")]
async fn kill_pid(pid: u32, grace_secs: i64) -> Result<()> {
    use nix::sys::signal::*;
    use nix::sys::wait::*;
    use nix::unistd::Pid;
    use std::time::Instant;

    // Reaps the process if it happens to be our child; harmless otherwise.
    fn reap(pid: Pid) {
        let _ = waitpid(pid, Some(WaitPidFlag::WNOHANG));
    }

    async fn wait_until_gone(pid: Pid, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while process_alive(pid.as_raw() as u32) {
            if Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
            reap(pid);
        }
        true
    }

    let pid = Pid::from_raw(pid as i32);

    match kill(pid, Signal::SIGTERM) {
        Ok(()) => (),
        Err(nix::errno::Errno::ESRCH) => return Ok(()),
        Err(error) => return Err(error.into()),
    }
    log::debug!("Sent SIGTERM to {:?}", pid);

    if wait_until_gone(pid, Duration::from_secs(grace_secs.max(0) as u64)).await {
        return Ok(());
    }

    log::debug!("Sending SIGKILL to {:?}", pid);
    match kill(pid, Signal::SIGKILL) {
        Ok(()) => (),
        Err(nix::errno::Errno::ESRCH) => return Ok(()),
        Err(error) => return Err(error.into()),
    }

    if !wait_until_gone(pid, Duration::from_secs(5)).await {
        anyhow::bail!("process {} is still running after SIGKILL", pid);
    }
    Ok(())
}

/// Checks whether a process we don't own is still running.
///
/// A process that exited with code 259 (`STILL_ACTIVE`) is indistinguishable
/// from a running one, which is a documented Windows quirk we can live with:
/// neither yagna nor ya-provider uses that exit code.
#[cfg(target_family = "windows")]
fn process_alive(pid: u32) -> bool {
    use winapi::shared::minwindef::{DWORD, FALSE};
    use winapi::um::handleapi::CloseHandle;
    use winapi::um::minwinbase::STILL_ACTIVE;
    use winapi::um::processthreadsapi::{GetExitCodeProcess, OpenProcess};
    use winapi::um::winnt::PROCESS_QUERY_LIMITED_INFORMATION;

    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid);
        if handle.is_null() {
            return false;
        }

        let mut exit_code: DWORD = 0;
        let queried = GetExitCodeProcess(handle, &mut exit_code) != 0;
        CloseHandle(handle);

        queried && exit_code == STILL_ACTIVE
    }
}

/// Windows has no SIGTERM that could be delivered to a process running in
/// another console, so this is an immediate, hard termination - the equivalent
/// of the SIGKILL the unix version escalates to. Use `--graceful` to let the
/// provider finish its tasks first.
#[cfg(target_family = "windows")]
async fn kill_pid(pid: u32, timeout: i64) -> Result<()> {
    use anyhow::bail;
    use std::time::Instant;
    use winapi::shared::minwindef::FALSE;
    use winapi::um::errhandlingapi::GetLastError;
    use winapi::um::handleapi::CloseHandle;
    use winapi::um::processthreadsapi::{OpenProcess, TerminateProcess};
    use winapi::um::winnt::PROCESS_TERMINATE;

    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, FALSE, pid);
        if handle.is_null() {
            // Read the error before process_alive overwrites it with its own calls.
            let error_code = GetLastError();
            // Nothing to kill if the process is already gone.
            if !process_alive(pid) {
                return Ok(());
            }
            bail!("unable to open process {}: error code {}", pid, error_code);
        }

        let terminated = TerminateProcess(handle, 1) != 0;
        let error_code = GetLastError();
        CloseHandle(handle);

        if !terminated && process_alive(pid) {
            bail!(
                "unable to terminate process {}: error code {}",
                pid,
                error_code
            );
        }
    }
    log::debug!("Terminated process {}", pid);

    // TerminateProcess is asynchronous - wait for the process to actually go away.
    let delay = Duration::from_millis(100);
    let deadline = Instant::now() + Duration::from_secs(timeout.max(0) as u64);
    while process_alive(pid) {
        if Instant::now() >= deadline {
            bail!("process {} is still running after termination", pid);
        }
        tokio::time::sleep(delay).await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_config_parses_graceful_with_timeout() {
        let config = StopConfig::from_iter(["stop", "--graceful", "--timeout", "60"]);
        assert!(config.graceful);
        assert_eq!(config.timeout, Some(60));

        let config = StopConfig::from_iter(["stop"]);
        assert!(!config.graceful);
        assert_eq!(config.timeout, None);
    }

    #[test]
    fn stop_config_rejects_removed_provider_only() {
        assert!(StopConfig::from_iter_safe(["stop", "--provider-only"]).is_err());
    }

    #[test]
    fn stop_config_timeout_requires_graceful() {
        assert!(StopConfig::from_iter_safe(["stop", "--timeout", "60"]).is_err());
    }

    #[cfg(target_family = "unix")]
    #[tokio::test]
    async fn stopping_an_already_gone_process_succeeds() {
        kill_pid(i32::MAX as u32, 0).await.unwrap();
    }
}
