use crate::CliArgs;
use std::fmt::Debug;
use std::{convert::TryFrom, fmt, path::PathBuf};
use structopt::*;

pub const DEFAULT_PORT: u16 = 7465;

#[allow(dead_code)]
pub struct CliCtx {
    data_dir: PathBuf,
    addr: (String, u16),
    json_output: bool,
    //    accept_any_prompt: bool,
    //    net: Option<Net>,
    interactive: bool,
    //    sys: SystemRunner,
}

impl TryFrom<&CliArgs> for CliCtx {
    type Error = anyhow::Error;

    fn try_from(args: &CliArgs) -> Result<Self, Self::Error> {
        let data_dir = args.get_data_dir();
        let addr = args.get_address()?;
        let json_output = args.json;
        //        let net = value.net.clone();
        //        let accept_any_prompt = args.accept_any_prompt;
        let interactive = args.interactive;
        //        let sys = actix::System::new("golemcli");

        Ok(CliCtx {
            addr,
            data_dir,
            json_output,
            //            accept_any_prompt,
            //            net,
            interactive,
            //            sys,
        })
    }
}

impl CliCtx {
    pub fn address(&self) -> (&str, u16) {
        (&self.addr.0, self.addr.1)
    }
}

#[derive(StructOpt)]
/// Generates autocomplete script from given shell
pub struct Complete {
    /// Describes which shell to produce a completions file for
    #[structopt(
        parse(try_from_str),
        possible_values = &clap::Shell::variants(),
        case_insensitive = true
    )]
    shell: clap::Shell,
}

impl Debug for Complete {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        writeln!(f, "complete({})", self.shell)
    }
}

impl Complete {
    pub(crate) fn run_command(&self) -> anyhow::Result<()> {
        let binary_name = clap::crate_name!();
        println!(
            "# generating {} completions for {}",
            binary_name, self.shell
        );
        CliArgs::clap().gen_completions_to(binary_name, self.shell, &mut std::io::stdout());

        Ok(())
    }
}
