use structopt::StructOpt;
use crate::startup_config::ProviderConfig;

#[derive(StructOpt, Clone, Debug)]
#[structopt(rename_all = "kebab-case")]
pub enum ExeUnitsConfig {
    List,
    // TODO: Install command - could download ExeUnit and add to descriptor file.
    // TODO: Update command - could update ExeUnit.
}

impl ExeUnitsConfig {
    pub fn run(self, config: ProviderConfig) -> anyhow::Result<()> {
        match self {
            ExeUnitsConfig::List => list(config)
        }
    }
}

fn list(config: ProviderConfig) -> anyhow::Result<()> {
    let registry = config.registry()?;
    if let Err(errors) = registry.validate() {
        log::error!("Encountered errors while checking ExeUnits:\n{}", errors);
    }

    if config.json {
        println!("{}", serde_json::to_string_pretty(&registry.list())?);
    } else {
        println!("Available ExeUnits:");
        for exeunit in registry.list() {
            println!("\n{}", exeunit);
        }
    }
    Ok(())
}
