use crate::hardware::{Resources, UpdateResources, Profiles};
use crate::startup_config::{UpdateNames, ProviderConfig};
use crate::hardware::ProfileError;
use structopt::StructOpt;

#[derive(StructOpt, Clone, Debug)]
#[structopt(rename_all = "kebab-case")]
pub enum ProfileConfig {
    /// List available profiles
    List,
    /// Show the name of an active profile
    Active,
    /// Create a new profile
    Create {
        name: String,
        #[structopt(flatten)]
        resources: Resources,
    },
    /// Update a profile
    Update {
        #[structopt(flatten)]
        names: UpdateNames,
        #[structopt(flatten)]
        resources: UpdateResources,
    },
    /// Remove an existing profile
    Remove { name: String },
    /// Activate a profile
    Activate { name: String },
}


impl ProfileConfig {

    pub fn run(self, config : ProviderConfig) -> anyhow::Result<()> {
        {
            let path = config.hardware_file.as_path();
            match self {
                ProfileConfig::List => {
                    let profiles = Profiles::load_or_create(&config)?.list();
                    println!("{}", serde_json::to_string_pretty(&profiles)?);
                }
                ProfileConfig::Create { name, resources } => {
                    let mut profiles = Profiles::load_or_create(&config)?;
                    if let Some(_) = profiles.get(&name) {
                        return Err(ProfileError::AlreadyExists(name).into());
                    }
                    profiles.add(name, resources)?;
                    profiles.save(path)?;
                }
                ProfileConfig::Update { names, resources } => {
                    update_profiles(config, names, resources)?;
                }
                ProfileConfig::Remove { name } => {
                    let mut profiles = Profiles::load_or_create(&config)?;
                    profiles.remove(name)?;
                    profiles.save(path)?;
                }
                ProfileConfig::Activate { name } => {
                    let mut profiles = Profiles::load_or_create(&config)?;
                    profiles.set_active(name)?;
                    profiles.save(path)?;
                }
                ProfileConfig::Active => {
                    let profiles = Profiles::load_or_create(&config)?;
                    println!("{}", serde_json::to_string_pretty(profiles.active())?);
                }
            }
            Ok(())
        }
    }
}

fn update_profiles(
    config: ProviderConfig,
    names: UpdateNames,
    new_resources: UpdateResources,
) -> anyhow::Result<()> {
    let mut profiles = Profiles::load_or_create(&config)?;

    fn update_profile(resources: &mut Resources, new_resources: UpdateResources) {
        if let Some(cpu_threads) = new_resources.cpu_threads {
            resources.cpu_threads = cpu_threads;
        }
        if let Some(mem_gib) = new_resources.mem_gib {
            resources.mem_gib = mem_gib;
        }
        if let Some(storage_gib) = new_resources.storage_gib {
            resources.storage_gib = storage_gib;
        }
    }

    if names.all {
        for resources in profiles.list().values_mut() {
            update_profile(resources, new_resources);
        }
    } else {
        for name in names.names {
            match profiles.get_mut(&name) {
                Some(resources) => update_profile(resources, new_resources),
                _ => return Err(ProfileError::Unknown(name).into()),
            }
        }
    }

    profiles.save(config.hardware_file.as_path())?;
    Ok(())
}
