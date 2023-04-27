use structopt::StructOpt;

use crate::dir::clean_provider_dir;
use crate::startup_config::ProviderConfig;

#[derive(StructOpt, Clone, Debug)]
#[structopt(rename_all = "kebab-case")]
pub struct CleanConfig {
    /// Age of files which will be removed expressed in the following format:
    /// <number>P, e.g. 30d
    /// where P: s|m|h|d|w|M|y or empty for days
    #[structopt(default_value = "30d")]
    pub age: String,
    /// Perform a dry run
    #[structopt(long)]
    pub dry_run: bool,
}

impl CleanConfig {
    pub fn run(&self, config: ProviderConfig) -> anyhow::Result<()> {
        let data_dir = config.data_dir.get_or_create()?;
        println!("Using data dir: {}", data_dir.display());

        let freed = clean_provider_dir(&data_dir, &self.age, true, self.dry_run)?;
        let human_freed = bytesize::to_string(freed, false);

        if self.dry_run {
            println!("Dry run: {} to be freed", human_freed)
        } else {
            println!("Freed {} of disk space", human_freed)
        }

        Ok(())
    }
}
