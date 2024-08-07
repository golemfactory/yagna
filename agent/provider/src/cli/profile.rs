use crate::hardware::{ProfileError, UpdateResourcesArgs};
use crate::hardware::{Profiles, Resources, UpdateResources};
use crate::startup_config::{ProviderConfig, UpdateNames};
use anyhow::anyhow;
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
        resources: UpdateResourcesArgs,
    },
    /// Remove an existing profile
    Remove { name: String },
    /// Activate a profile
    Activate { name: String },
}

impl ProfileConfig {
    pub fn run(self, config: ProviderConfig) -> anyhow::Result<()> {
        {
            let path = config.hardware_file.as_path();
            match self {
                ProfileConfig::List => {
                    let profiles = Profiles::load_or_create(&config)?.list();
                    println!("{}", serde_json::to_string_pretty(&profiles)?);
                }
                ProfileConfig::Create { name, resources } => {
                    let mut profiles = Profiles::load_or_create(&config)?;
                    if profiles.get(&name).is_some() {
                        return Err(ProfileError::AlreadyExists(name).into());
                    }
                    profiles.add(name, resources)?;
                    profiles.save(path)?;
                }
                ProfileConfig::Update { names, resources } => {
                    update_profiles(config, names, UpdateResourcesParams::from_args(resources)?)?;
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
pub enum UpdateResourcesParams {
    ResetToDefaults,
    UpdateResources(UpdateResources),
}

impl UpdateResourcesParams {
    fn from_args(update_resources_args: UpdateResourcesArgs) -> anyhow::Result<Self> {
        if !update_resources_args.reset_to_defaults {
            return Ok(Self::UpdateResources(UpdateResources {
                cpu_threads: update_resources_args.cpu_threads,
                mem_gib: update_resources_args.mem_gib,
                storage_gib: update_resources_args.storage_gib,
            }));
        } else {
            if update_resources_args.mem_gib.is_some() {
                return Err(anyhow!("--reset-to-defaults conflicts with --mem-gib."));
            } else if update_resources_args.storage_gib.is_some() {
                return Err(anyhow!("--reset-to-defaults conflicts with --storage-gib."));
            } else if update_resources_args.cpu_threads.is_some() {
                return Err(anyhow!("--reset-to-defaults conflicts with --cpu-threads."));
            }
            Ok(Self::ResetToDefaults)
        }
    }
}

fn update_profiles(
    config: ProviderConfig,
    names: UpdateNames,
    update_params: UpdateResourcesParams,
) -> anyhow::Result<()> {
    let mut profiles = Profiles::load_or_create(&config)?;

    let new_resources = match update_params {
        UpdateResourcesParams::ResetToDefaults => {
            let path = config.hardware_file.as_path();
            // path should exist because we've called Profile::load_or_create
            if !path.exists() {
                return Err(anyhow!("Unexpected condition - hardware file disappeared."));
            }
            let resources = Resources::try_with_config(path, &config)?;
            let default_caps = Resources::default_caps(&path, &config.default_caps)?;
            let caped_resources = resources.cap(&default_caps);
            UpdateResources {
                cpu_threads: Some(caped_resources.cpu_threads),
                mem_gib: Some(caped_resources.mem_gib),
                storage_gib: Some(caped_resources.storage_gib),
            }
        }
        UpdateResourcesParams::UpdateResources(res) => res,
    };

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
        for name in names.name {
            match profiles.get_mut(&name) {
                Some(resources) => update_profile(resources, new_resources),
                _ => return Err(ProfileError::Unknown(name).into()),
            }
        }
    }

    profiles.save(config.hardware_file.as_path())?;
    Ok(())
}
