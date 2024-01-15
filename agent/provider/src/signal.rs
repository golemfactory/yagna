pub(crate) type Signal = &'static str;

use tokio::task::JoinHandle;
use tokio::{
    select,
    sync::{
        oneshot,
        oneshot::{Receiver, Sender},
    },
};

#[cfg(target_family = "unix")]
use tokio::signal::unix;
#[cfg(target_family = "windows")]
use tokio::signal::{windows, windows::CtrlBreak};

pub struct SignalMonitor {
    stop_tx: Sender<Signal>,
    stop_rx: Receiver<Signal>,
}

impl Default for SignalMonitor {
    fn default() -> Self {
        let (stop_tx, stop_rx) = oneshot::channel();
        Self { stop_tx, stop_rx }
    }
}

impl SignalMonitor {
    pub async fn recv(self) -> anyhow::Result<Signal> {
        Self::start(self.stop_tx)?;
        Ok(self.stop_rx.await?)
    }

    #[cfg(target_family = "unix")]
    fn start(stop_tx: Sender<Signal>) -> anyhow::Result<JoinHandle<()>> {
        let mut sigterm = unix::signal(unix::SignalKind::terminate())?;
        let mut sigint = unix::signal(unix::SignalKind::interrupt())?;
        let mut sigquit = unix::signal(unix::SignalKind::quit())?;
        Ok(tokio::spawn(async move {
            select! {
                _ = sigterm.recv() => stop_tx.send("SIGTERM").expect("Failed to handle SIGTERM event"),
                _ = sigint.recv() => stop_tx.send("SIGINT").expect("Failed to handle SIGINT event"),
                _ = sigquit.recv() => stop_tx.send("SIGQUIT").expect("Failed to handle SIGQUIT event"),
            };
        }))
    }

    #[cfg(target_family = "windows")]
    fn start(stop_tx: Sender<Signal>) -> anyhow::Result<JoinHandle<()>> {
        let mut ctrl_c = windows::ctrl_c()?;
        let mut ctrl_close = windows::ctrl_close()?;
        let mut ctrl_logoff = windows::ctrl_logoff()?;
        let mut ctrl_shutdown = windows::ctrl_shutdown()?;
        let ctrl_break = windows::ctrl_break()?;
        Ok(tokio::spawn(async move {
            select! {
                _ = ctrl_c.recv() => stop_tx.send("CTRL-C").expect("Failed to handle CTRL-C event"),
                _ = ctrl_close.recv() => stop_tx.send("CTRL-CLOSE").expect("Failed to handle CTRL-CLOSE event"),
                _ = ctrl_logoff.recv() => stop_tx.send("CTRL-LOGOFF").expect("Failed to handle CTRL-LOGOFF event"),
                _ = ctrl_shutdown.recv() => stop_tx.send("CTRL-SHUTDOWN").expect("Failed to handle SHUTDOWN event"),
                _ = ignore_ctrl_break(ctrl_break) => {},
            };
        }))
    }
}

#[cfg(target_family = "windows")]
async fn ignore_ctrl_break(mut ctrl_break: CtrlBreak) {
    loop {
        ctrl_break.recv().await;
        log::trace!("Received CTRL-BREAK. Ignoring it.");
    }
}
