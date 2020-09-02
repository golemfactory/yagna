#![allow(unused_imports)]

use std::error::Error;
use tokio::process::Command;
use tokio_process_ns::*;

#[tokio::main]
#[cfg(target_os = "linux")]
async fn main() -> Result<(), Box<dyn Error>> {
    let status = Command::new("/bin/bash")
        .new_ns(NsOptions::new().procfs().kill_child())
        .status()
        .await?;
    eprintln!("done: {}", status);
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    unimplemented!()
}
