use std::path::PathBuf;

#[derive(structopt::StructOpt, Debug)]
pub struct Cli {
    #[structopt(long, short, set = structopt::clap::ArgSettings::Global)]
    agreement: Option<PathBuf>,
    #[structopt(long, short, set = structopt::clap::ArgSettings::Global)]
    work_dir: PathBuf,
    #[structopt(long, short, set = structopt::clap::ArgSettings::Global)]
    cache_dir: PathBuf,
    #[structopt(subcommand)]
    command: Command,
}

#[derive(structopt::StructOpt, Debug)]
pub enum Command {
    ServiceBus { service_id: String },
    FromFile { input: PathBuf },
}
