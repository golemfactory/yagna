use anyhow::{Result, Error};
use log::info;
use std::path::{PathBuf, Path};
use structopt::StructOpt;



#[derive(StructOpt)]
pub enum Commands {
    Deploy {
        args: Vec<String>,
    },
    Run {
        args: Vec<String>,
    }
}


#[derive(StructOpt)]
pub struct CmdArgs {
    #[structopt(short = "w", long = "workdir")]
    workdir: PathBuf,
    #[structopt(short = "c", long = "cachedir")]
    cachedir: PathBuf,
    #[structopt(subcommand)]
    command: Commands,
}


pub struct ExeUnitMain;

impl ExeUnitMain {

    pub fn entrypoint(cmdline: CmdArgs) -> Result<()> {
        match cmdline.command {
            Commands::Run{args} => ExeUnitMain::run(&cmdline.workdir, &cmdline.cachedir, args),
            Commands::Deploy{args} => ExeUnitMain::deploy(&cmdline.workdir, &cmdline.cachedir, args),
        }
    }

    fn run(workdir: &Path, cachedir: &Path, args: Vec<String>) -> Result<()> {
        info!("Called run command");
        Ok(())
    }

    fn deploy(workdir: &Path, cachedir: &Path, args: Vec<String>) -> Result<()> {
        info!("Called deploy command");
        Ok(())
    }
}

