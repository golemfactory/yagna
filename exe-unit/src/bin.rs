use structopt::StructOpt;

use ya_exe_unit::logger::*;
use ya_exe_unit::{run, Cli};

#[cfg(feature = "packet-trace-enable")]
fn init_packet_trace() -> anyhow::Result<()> {
    use ya_packet_trace::{set_write_target, WriteTarget};

    let write = std::fs::File::create("./exe-unit.trace")?;
    set_write_target(WriteTarget::Write(Box::new(write)));

    Ok(())
}

#[actix_rt::main]
async fn main() {
    let panic_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |e| {
        log::error!("ExeUnit Supervisor panic: {e}");
        panic_hook(e)
    }));

    if let Err(error) = start_file_logger() {
        start_logger().expect("Failed to start logging");
        log::warn!("Using fallback logging due to an error: {:?}", error);
    };

    dotenv::dotenv().ok();
    #[cfg(feature = "packet-trace-enable")]
    init_packet_trace()?;

    let cli: Cli = Cli::from_args();

    std::process::exit(match run(cli).await {
        Ok(_) => 0,
        Err(error) => {
            log::error!("{}", error);
            1
        }
    })
}
