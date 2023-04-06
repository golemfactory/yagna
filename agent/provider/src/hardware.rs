use std::cmp::Ordering;
use std::collections::HashMap;
#[cfg(windows)]
use std::ffi::OsStr;
use std::io;
use std::ops::{Add, Not, Sub};
use std::path::Path;
#[cfg(windows)]
use std::ptr;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use structopt::{clap, StructOpt};
use tokio::sync::watch;

use ya_agreement_utils::{CpuInfo, InfNodeInfo};
use ya_utils_path::SwapSave;

use crate::events::Event;
use crate::startup_config::{FileMonitor, ProviderConfig};

pub const DEFAULT_PROFILE_NAME: &str = "default";
pub const CPU_THREADS_RESERVED: i32 = 1;
pub static MIN_CAPS: Resources = Resources {
    cpu_threads: 1,
    mem_gib: 0.1,
    storage_gib: 0.1,
};

#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    #[error("unknown name: '{0}'")]
    Unknown(String),
    #[error("profile already exists: '{0}'")]
    AlreadyExists(String),
    #[error("profile is active: '{0}'")]
    Active(String),
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("insufficient hardware resources available")]
    InsufficientResources,
    #[error("resources already allocated for id {0}")]
    AlreadyAllocated(String),
    #[error("resources not allocated for id {0}")]
    NotAllocated(String),
    #[error("profile error: {0}")]
    Profile(#[from] ProfileError),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("file watch error: {0}")]
    FileWatch(#[from] notify::Error),
    #[error("system error: {0}")]
    Sys(#[from] sys_info::Error),
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, StructOpt)]
#[structopt(rename_all = "kebab-case")]
pub struct Resources {
    /// Number of CPU logical cores
    #[structopt(long)]
    pub cpu_threads: i32,
    /// Total amount of RAM
    #[structopt(long)]
    pub mem_gib: f64,
    /// Free partition space
    #[structopt(long)]
    pub storage_gib: f64,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, StructOpt)]
#[structopt(rename_all = "kebab-case")]
#[structopt(group = clap::ArgGroup::with_name("up-res").multiple(true).required(true))]
pub struct UpdateResources {
    /// Number of CPU logical cores
    #[structopt(long, group = "up-res")]
    pub cpu_threads: Option<i32>,
    /// Total amount of RAM
    #[structopt(long, group = "up-res")]
    pub mem_gib: Option<f64>,
    /// Free partition space
    #[structopt(long, group = "up-res")]
    pub storage_gib: Option<f64>,
}

impl Resources {
    pub fn try_with_config<P: AsRef<Path>>(
        path: P,
        config: &ProviderConfig,
    ) -> Result<Self, Error> {
        let max_caps = Self::max_caps(path)?;
        if config.rt_cores.is_some() || config.rt_mem.is_some() || config.rt_storage.is_some() {
            let mut user_caps = max_caps;

            if let Some(cores) = config.rt_cores {
                user_caps.cpu_threads = cores as i32;
            }
            if let Some(mem) = config.rt_mem {
                user_caps.mem_gib = mem;
            }
            if let Some(storage) = config.rt_storage {
                user_caps.storage_gib = storage;
            }

            return Ok(user_caps.cap(&max_caps));
        }
        Ok(max_caps)
    }

    fn new_empty() -> Self {
        Resources {
            cpu_threads: 0,
            mem_gib: 0.,
            storage_gib: 0.,
        }
    }

    fn max_caps<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
        Ok(Resources {
            cpu_threads: num_cpus::get() as i32,
            mem_gib: 1000. * sys_info::mem_info()?.total as f64 / (1024. * 1024. * 1024.),
            storage_gib: partition_space(path)? as f64 / (1024. * 1024. * 1024.),
        })
    }

    fn default_caps<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
        let res = Self::max_caps(path)?;
        Ok(Resources {
            cpu_threads: 1.max(res.cpu_threads - CPU_THREADS_RESERVED),
            mem_gib: 0.7 * res.mem_gib,
            storage_gib: 0.8 * res.storage_gib,
        })
    }

    pub fn depleted(&self) -> bool {
        self.cpu_threads <= 0 || self.mem_gib <= 0. || self.storage_gib <= 0.
    }

    pub fn cap(mut self, res: &Resources) -> Self {
        self.cpu_threads = MIN_CAPS
            .cpu_threads
            .max(self.cpu_threads.min(res.cpu_threads));
        self.mem_gib = MIN_CAPS.mem_gib.max(self.mem_gib.min(res.mem_gib));
        self.storage_gib = MIN_CAPS
            .storage_gib
            .max(self.storage_gib.min(res.storage_gib));
        self
    }
}

impl PartialEq for Resources {
    fn eq(&self, other: &Self) -> bool {
        self.cpu_threads == other.cpu_threads
            && self.mem_gib == other.mem_gib
            && self.storage_gib == other.storage_gib
    }
}

impl PartialOrd for Resources {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        if self == other {
            Some(Ordering::Equal)
        } else if self.cpu_threads >= other.cpu_threads
            && self.mem_gib >= other.mem_gib
            && self.storage_gib >= other.storage_gib
        {
            Some(Ordering::Greater)
        } else {
            Some(Ordering::Less)
        }
    }
}

impl Eq for Resources {}

impl Add for Resources {
    type Output = Resources;

    fn add(self, rhs: Self) -> Self::Output {
        Resources {
            cpu_threads: self.cpu_threads + rhs.cpu_threads,
            mem_gib: self.mem_gib + rhs.mem_gib,
            storage_gib: self.storage_gib + rhs.storage_gib,
        }
    }
}

impl Sub for Resources {
    type Output = Resources;

    fn sub(self, rhs: Self) -> Self::Output {
        Resources {
            cpu_threads: self.cpu_threads - rhs.cpu_threads,
            mem_gib: self.mem_gib - rhs.mem_gib,
            storage_gib: self.storage_gib - rhs.storage_gib,
        }
    }
}

impl From<Resources> for InfNodeInfo {
    fn from(res: Resources) -> Self {
        let cpu_info = CpuInfo {
            architecture: std::env::consts::ARCH.to_string(),
            cores: num_cpus::get_physical() as u32,
            threads: res.cpu_threads as u32,
        };

        InfNodeInfo::default()
            .with_mem(res.mem_gib)
            .with_storage(res.storage_gib)
            .with_cpu(cpu_info)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Profiles {
    active: String,
    profiles: HashMap<String, Resources>,
}

impl Profiles {
    pub fn load_or_create(config: &ProviderConfig) -> Result<Self, Error> {
        let path = config.hardware_file.as_path();
        match path.exists() {
            true => Self::load(path),
            false => {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::File::create(path)?;

                let mut profiles = Self::try_with_config(path, config)?;
                let default_caps = Resources::default_caps(path)?;
                for profile in profiles.profiles.values_mut() {
                    *profile = profile.cap(&default_caps);
                }
                profiles.save(path)?;
                Ok(profiles)
            }
        }
    }

    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
        log::debug!("Loading profile from: {}", path.as_ref().display());
        let contents = std::fs::read_to_string(&path)?;
        let new: Profiles = serde_json::from_str(contents.as_str())?;
        if new.profiles.contains_key(&new.active).not() {
            return Err(ProfileError::Unknown(new.active).into());
        }

        Ok(serde_json::from_str(contents.as_str())?)
    }

    #[inline]
    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<(), Error> {
        Ok(path.swap_save(serde_json::to_string_pretty(self)?)?)
    }

    fn try_with_config<P: AsRef<Path>>(path: P, config: &ProviderConfig) -> Result<Self, Error> {
        let resources = Resources::try_with_config(path.as_ref(), config)?;
        let active = DEFAULT_PROFILE_NAME.to_string();
        let profiles = vec![(active.clone(), resources)].into_iter().collect();
        Ok(Profiles { active, profiles })
    }
}

impl Profiles {
    #[inline]
    pub fn list(&self) -> HashMap<String, Resources> {
        self.profiles.clone()
    }

    #[inline]
    pub fn get(&self, name: impl ToString) -> Option<&Resources> {
        self.profiles.get(&name.to_string())
    }

    #[inline]
    pub fn get_mut(&mut self, name: impl ToString) -> Option<&mut Resources> {
        self.profiles.get_mut(&name.to_string())
    }

    #[inline]
    pub fn add(&mut self, name: impl ToString, resources: Resources) -> Result<(), Error> {
        if resources < Resources::new_empty() {
            return Err(Error::InsufficientResources);
        }
        self.profiles.insert(name.to_string(), resources);
        Ok(())
    }

    #[inline]
    pub fn remove(&mut self, name: impl ToString) -> Result<(), Error> {
        let name = name.to_string();
        if name == self.active {
            return Err(ProfileError::Active(name).into());
        }
        if self.profiles.remove(&name).is_none() {
            return Err(ProfileError::Unknown(name).into());
        }
        Ok(())
    }

    #[inline]
    pub fn active(&self) -> &String {
        &self.active
    }

    pub fn set_active(&mut self, name: impl ToString) -> Result<(), Error> {
        let name = name.to_string();
        if self.profiles.contains_key(&name).not() {
            return Err(ProfileError::Unknown(name).into());
        }
        self.active = name;
        Ok(())
    }
}

#[derive(Debug)]
pub struct Manager {
    state: Arc<Mutex<ManagerState>>,
    monitor: Option<FileMonitor>,
    sender: Option<watch::Sender<Event>>,
    receiver: watch::Receiver<Event>,
}

#[derive(Debug)]
struct ManagerState {
    profiles: Profiles,
    res_available: Resources,
    res_cap: Resources,
    res_remaining: Resources,
    res_alloc: HashMap<String, Resources>,
}

impl ManagerState {
    #[inline]
    fn update(&mut self, profiles: Profiles) -> Result<bool, Error> {
        self.profiles = profiles;
        self.change_profile(self.profiles.active.clone())
    }

    fn change_profile(&mut self, name: impl ToString) -> Result<bool, Error> {
        let name = name.to_string();
        log::info!("Activating hardware profile '{}'", name);
        self.profiles.set_active(&name)?;
        let res = self
            .profiles
            .get(&self.profiles.active)
            .cloned()
            .ok_or(ProfileError::Unknown(name))?
            .cap(&self.res_available);

        if res == self.res_cap {
            Ok(false)
        } else {
            let delta = self.res_cap - res;
            self.res_cap = res;
            self.res_remaining = self.res_remaining - delta;
            log::info!("Hardware resources cap: {:?}", self.res_cap);
            log::info!("Hardware resources remaining: {:?}", self.res_remaining);
            Ok(true)
        }
    }
}

impl Manager {
    pub fn try_new(conf: &ProviderConfig) -> Result<Self, Error> {
        let profiles = Profiles::load_or_create(conf)?;

        let mut state = ManagerState {
            profiles,
            res_available: Resources::try_with_config(conf.hardware_file.as_path(), conf)?,
            res_cap: Resources::new_empty(),
            res_remaining: Resources::new_empty(),
            res_alloc: HashMap::new(),
        };
        state.change_profile(state.profiles.active.clone())?;

        let (tx, rx) = watch::channel(Event::Initialized);
        Ok(Manager {
            state: Arc::new(Mutex::new(state)),
            monitor: None,
            sender: Some(tx),
            receiver: rx,
        })
    }

    pub fn spawn_monitor<P: AsRef<Path>>(&mut self, path: P) -> Result<(), Error> {
        let tx = self.sender.take().unwrap();
        let state = self.state.clone();
        let handler = move |p| match Profiles::load(&p) {
            Ok(profiles) => {
                let result = { state.lock().unwrap().update(profiles) };
                match result {
                    Ok(val) => match val {
                        true => tx.send(Event::HardwareChanged).unwrap_or_default(),
                        false => log::info!("Hardware configuration unchanged"),
                    },
                    Err(err) => log::warn!("Error updating hardware configuration: {:?}", err),
                }
            }
            Err(e) => log::warn!("Error reading hardware profiles from {:?}: {:?}", p, e),
        };

        let monitor = FileMonitor::spawn(path, FileMonitor::on_modified(handler))?;
        self.monitor = Some(monitor);
        Ok(())
    }

    #[inline]
    pub fn event_receiver(&self) -> watch::Receiver<Event> {
        self.receiver.clone()
    }
}

impl Manager {
    #[inline]
    pub fn capped(&self) -> Resources {
        let state = self.state.lock().unwrap();
        state.res_cap
    }

    #[allow(dead_code)]
    pub fn allocate(&mut self, id: String, res: Resources) -> Result<(), Error> {
        let mut state = self.state.lock().unwrap();
        if state.res_alloc.contains_key(&id) {
            return Err(Error::AlreadyAllocated(id));
        }
        if state.res_remaining < res {
            return Err(Error::InsufficientResources);
        }
        state.res_remaining = state.res_remaining - res;
        state.res_alloc.insert(id, res);
        Ok(())
    }

    #[allow(dead_code)]
    pub fn release(&mut self, id: String) -> Result<(), Error> {
        let mut state = self.state.lock().unwrap();
        match state.res_alloc.remove(&id) {
            Some(res) => state.res_remaining = state.res_remaining + res,
            _ => return Err(Error::NotAllocated(id)),
        }
        Ok(())
    }
}

#[cfg(windows)]
fn to_wstring(value: impl AsRef<OsStr>) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;

    value
        .as_ref()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// Return free space on a partition with a given path
fn partition_space<P: AsRef<Path>>(path: P) -> Result<u64, Error> {
    let path = path.as_ref();
    #[cfg(windows)]
    {
        use winapi::um::errhandlingapi::GetLastError;
        use winapi::um::fileapi::GetDiskFreeSpaceExW;
        use winapi::um::winnt::PULARGE_INTEGER;

        let path = if path.is_file() {
            path.parent().unwrap()
        } else {
            path
        };

        let wide: Vec<u16> = to_wstring(path);
        let mut free_bytes_available = 0u64;

        if unsafe {
            GetDiskFreeSpaceExW(
                wide.as_ptr(),
                &mut free_bytes_available as *mut u64 as PULARGE_INTEGER,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        } == 0
        {
            let err = unsafe { GetLastError() };
            log::error!("Unable to read free partition space for path '{:?}", path);
            return Err(Error::Io(std::io::Error::from_raw_os_error(err as i32)));
        };

        Ok(free_bytes_available)
    }
    #[cfg(not(windows))]
    {
        use nix::sys::statvfs::statvfs;
        let stat =
            statvfs(path.as_os_str()).map_err(|e| sys_info::Error::General(e.to_string()))?;
        #[allow(clippy::unnecessary_cast)]
        Ok(stat.blocks_available() as u64 * stat.fragment_size())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profiles() -> Profiles {
        let active = DEFAULT_PROFILE_NAME.to_string();
        let resources = Resources {
            cpu_threads: 4,
            mem_gib: 8.,
            storage_gib: 100.,
        };
        let profiles = vec![(active.clone(), resources)].into_iter().collect();
        Profiles { active, profiles }
    }

    #[test]
    fn limit_by_caps() {
        let res = Resources {
            cpu_threads: 16,
            mem_gib: 24.,
            storage_gib: 200.,
        }
        .cap(&Resources {
            cpu_threads: 4,
            mem_gib: 8.,
            storage_gib: 100.,
        });

        assert_eq!(res.cpu_threads, 4);
        assert_eq!(res.mem_gib, 8.);
        assert_eq!(res.storage_gib, 100.);
    }

    #[test]
    fn limit_by_hardware() {
        let res = Resources {
            cpu_threads: 2,
            mem_gib: 2.,
            storage_gib: 20.,
        }
        .cap(&Resources {
            cpu_threads: 4,
            mem_gib: 8.,
            storage_gib: 100.,
        });

        assert_eq!(res.cpu_threads, 2);
        assert_eq!(res.mem_gib, 2.);
        assert_eq!(res.storage_gib, 20.);
    }

    #[test]
    fn limit_min() {
        let res = Resources {
            cpu_threads: 2,
            mem_gib: 2.,
            storage_gib: 20.,
        }
        .cap(&Resources {
            cpu_threads: 0,
            mem_gib: 0.,
            storage_gib: 0.,
        });

        assert_eq!(res.cpu_threads, 1);
        assert_eq!(res.mem_gib, 0.1);
        assert_eq!(res.storage_gib, 0.1);
    }

    #[test]
    fn allocation() {
        let res = Resources {
            cpu_threads: 8,
            mem_gib: 24.,
            storage_gib: 200.,
        };
        let state = ManagerState {
            res_available: res,
            res_cap: res,
            res_remaining: res,
            res_alloc: HashMap::new(),
            profiles: profiles(),
        };
        let (tx, rx) = watch::channel(Event::Initialized);
        let mut man = Manager {
            state: Arc::new(Mutex::new(state)),
            monitor: None,
            sender: Some(tx),
            receiver: rx,
        };
        let alloc = Resources {
            cpu_threads: 1,
            mem_gib: 1.51,
            storage_gib: 12.37,
        };

        man.allocate("1".into(), alloc).unwrap();
        man.allocate("2".into(), alloc).unwrap();
        man.allocate("3".into(), alloc).unwrap();
        man.release("1".into()).unwrap();
        man.release("2".into()).unwrap();
        man.release("3".into()).unwrap();

        let remaining = man.state.lock().unwrap().res_remaining;
        assert_eq!(remaining.cpu_threads, res.cpu_threads);
        assert_eq!(remaining.mem_gib, res.mem_gib);
        assert_eq!(remaining.storage_gib, res.storage_gib);
    }

    #[test]
    fn allocation_err() {
        let res = Resources {
            cpu_threads: 8,
            mem_gib: 24.,
            storage_gib: 200.,
        };
        let state = ManagerState {
            res_available: res,
            res_cap: res,
            res_remaining: res,
            res_alloc: HashMap::new(),
            profiles: profiles(),
        };
        let (tx, rx) = watch::channel(Event::Initialized);
        let mut man = Manager {
            state: Arc::new(Mutex::new(state)),
            monitor: None,
            sender: Some(tx),
            receiver: rx,
        };
        let alloc = Resources {
            cpu_threads: 1,
            mem_gib: 1.51,
            storage_gib: 12.37,
        };

        man.allocate("1".into(), alloc).unwrap();
        assert!(man.allocate("1".into(), alloc).is_err());
        assert!(man.release("2".into()).is_err());
        assert!(man
            .allocate(
                "3".into(),
                Resources {
                    cpu_threads: 1000,
                    mem_gib: 10000.,
                    storage_gib: 10000.,
                }
            )
            .is_err());
    }
}
