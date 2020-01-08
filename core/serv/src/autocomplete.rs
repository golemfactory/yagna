use std::fmt;
use structopt::{clap, StructOpt};

use crate::{CliArgs, CliCtx};
use ya_service_api::CommandOutput;

#[derive(StructOpt)]
/// Generates autocomplete script from given shell
pub struct CompleteCommand {
    /// Describes which shell to produce a completions file for
    #[structopt(
    parse(try_from_str),
    possible_values = &clap::Shell::variants(),
    case_insensitive = true
    )]
    shell: clap::Shell,
}

impl fmt::Debug for CompleteCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        writeln!(f, "complete({})", self.shell)
    }
}

impl CompleteCommand {
    pub fn run_command(&self, _ctx: &CliCtx) -> anyhow::Result<CommandOutput> {
        let binary_name = clap::crate_name!();
        println!(
            "# generating {} completions for {}",
            binary_name, self.shell
        );
        CliArgs::clap().gen_completions_to(binary_name, self.shell, &mut std::io::stdout());

        Ok(CommandOutput::NoOutput)
    }
}
