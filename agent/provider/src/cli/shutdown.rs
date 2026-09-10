use structopt::{clap, StructOpt};

use ya_utils_process::lock::ProcLock;

use crate::shutdown::ShutdownStatus;
use crate::startup_config::ProviderConfig;

/// Requests graceful shutdown of a running provider. The provider stops
/// offering on the market, waits for current tasks until their announced
/// deadlines, finalizes the remaining Agreements, and gives issued Invoices
/// a bounded delivery window before exiting.
#[derive(StructOpt, Clone, Debug)]
pub struct ShutdownConfig {}

impl ShutdownConfig {
    pub fn run(&self, config: ProviderConfig) -> anyhow::Result<()> {
        ShutdownStatus {
            graceful_shutdown_requested: true,
        }
        .save(&config.shutdown_file)?;

        let data_dir = config.data_dir.get_or_create()?;
        let running = ProcLock::new(clap::crate_name!(), &data_dir)
            .and_then(|lock| lock.read_pid())
            .is_ok();
        if running {
            println!(
                "Graceful shutdown requested. The provider will stop accepting new agreements \
                 and wait for current tasks until the configured termination deadline."
            );
        } else {
            println!(
                "Warning: no running ya-provider detected. \
                 The request will be reset on the next provider start."
            );
        }
        Ok(())
    }
}
