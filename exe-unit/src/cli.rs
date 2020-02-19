use std::path::PathBuf;

#[derive(structopt::StructOpt, Debug)]
pub struct Cli {
    #[structopt(long, short)]
    pub agreement: PathBuf,
    #[structopt(long, short)]
    pub work_dir: PathBuf,
    #[structopt(long, short)]
    pub cache_dir: PathBuf,
    #[structopt(subcommand)]
    pub command: Command,
}

#[derive(structopt::StructOpt, Debug)]
pub enum Command {
    ServiceBus {
        service_id: String,
        report_url: String,
    },
    FromFile {
        input: PathBuf,
    },
}
