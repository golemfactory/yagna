use std::path::PathBuf;

#[derive(structopt::StructOpt, Debug)]
pub struct Cli {
    #[structopt(long, short, set = structopt::clap::ArgSettings::Global)]
    agreement: PathBuf,
    #[structopt(long, short, set = structopt::clap::ArgSettings::Global)]
    workdir: PathBuf,
    #[structopt(long, short, set = structopt::clap::ArgSettings::Global)]
    cachedir: PathBuf,
    #[structopt(subcommand)]
    command: Command,
}

#[derive(structopt::StructOpt, Debug)]
pub enum Command {
    ServiceBus { service_id: String },
    FromFile { input: PathBuf },
}
