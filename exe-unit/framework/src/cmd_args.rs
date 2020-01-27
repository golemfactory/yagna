use structopt::StructOpt;

use std::path::PathBuf;


#[derive(StructOpt, Debug)]
pub enum Config {
    /// Run interactively in CLI mode
    CLI,
    /// Execute commands from JSON file
    FromFile { input: PathBuf },
    /// Bind to the Golem Service Bus
    Gsb { service_id: String },
}
