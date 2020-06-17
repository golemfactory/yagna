use crate::startup_config::FileMonitor;
use actix::Arbiter;
use notify::DebouncedEvent;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fmt;
use std::io;
use std::ops::{Add, Not, Sub};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use structopt::StructOpt;
use tokio::sync::broadcast;
use ya_agreement_utils::{CpuInfo, InfNodeInfo};
use ya_utils_path::SwapSave;

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

#[derive(Clone, Debug)]
pub enum Event {
    ConfigurationChanged(Resources),
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Event::ConfigurationChanged(res) => write!(f, "Configuration changed: {:?}", res),
        }
    }
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

impl Resources {
    pub fn try_new(work_dir: &Path) -> Result<Self, Error> {
        Ok(Resources {
            cpu_threads: num_cpus::get() as i32,
            mem_gib: 1000. * sys_info::mem_info()?.total as f64 / (1024. * 1024. * 1024.),
            storage_gib: partition_space(work_dir)? as f64 / (1024. * 1024. * 1024.),
        })
    }

    pub fn try_default(work_dir: &Path) -> Result<Self, Error> {
        let res = Self::try_new(work_dir)?;
        Ok(Resources {
            cpu_threads: 1.max(res.cpu_threads - CPU_THREADS_RESERVED),
            mem_gib: 0.7 * res.mem_gib,
            storage_gib: 0.8 * res.storage_gib,
        })
    }

    pub fn empty() -> Self {
        Resources {
            cpu_threads: 0,
            mem_gib: 0.,
            storage_gib: 0.,
        }
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
    pub fn try_default<P: AsRef<Path>>(work_dir: P) -> Result<Self, Error> {
        let resources = Resources::try_default(work_dir.as_ref())?;
        let active = DEFAULT_PROFILE_NAME.to_string();
        let profiles = vec![(active.clone(), resources)].into_iter().collect();
        Ok(Profiles { active, profiles })
    }

    pub fn load_or_create<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
        let path = path.as_ref();
        match path.exists() {
            true => Self::load(path),
            false => {
                let current_dir = std::env::current_dir()?;
                let profiles = Self::try_default(&current_dir)?;
                profiles.save(path)?;
                Ok(profiles)
            }
        }
    }

    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
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

    #[inline]
    pub fn list(&self) -> Vec<String> {
        self.profiles.keys().cloned().collect()
    }

    #[inline]
    pub fn get(&self, name: impl ToString) -> Option<&Resources> {
        self.profiles.get(&name.to_string())
    }

    #[inline]
    pub fn add(&mut self, name: impl ToString, resources: Resources) -> Result<(), Error> {
        if resources < Resources::empty() {
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
        if let None = self.profiles.remove(&name) {
            return Err(ProfileError::Unknown(name).into());
        }
        Ok(())
    }

    #[inline]
    pub fn get_active(&self) -> &Resources {
        self.profiles.get(&self.active).unwrap()
    }

    pub fn set_active(&mut self, name: impl ToString) -> Result<&Resources, Error> {
        let name = name.to_string();
        if self.profiles.contains_key(&name).not() {
            return Err(ProfileError::Unknown(name).into());
        }
        self.active = name;
        Ok(self.get_active())
    }
}

#[derive(Debug)]
pub struct Manager {
    res_available: Resources,
    res_cap: Arc<Mutex<Resources>>,
    res_remaining: Arc<Mutex<Resources>>,
    res_alloc: HashMap<String, Resources>,
    profiles: Arc<Mutex<Profiles>>,
    monitor: Option<FileMonitor>,
    broadcast: broadcast::Sender<Event>,
}

impl Manager {
    pub fn try_new<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
        let current_dir = std::env::current_dir()?;
        let profiles = Profiles::load_or_create(&path)?;
        let active_profile = profiles.active.clone();

        let (tx, mut rx) = broadcast::channel(16);
        Arbiter::spawn(async move {
            let delay = Duration::from_secs_f32(0.5);
            loop {
                if let Ok(evt) = rx.try_recv() {
                    log::info!("{}", evt);
                } else {
                    tokio::time::delay_for(delay).await;
                }
            }
        });

        let mut manager = Manager {
            res_available: Resources::try_new(&current_dir)?,
            res_cap: Arc::new(Mutex::new(Resources::empty())),
            res_remaining: Arc::new(Mutex::new(Resources::empty())),
            res_alloc: HashMap::new(),
            profiles: Arc::new(Mutex::new(profiles)),
            monitor: None,
            broadcast: tx,
        };
        manager.switch_profile(active_profile)?;
        manager.spawn_monitor(path)?;

        Ok(manager)
    }

    fn spawn_monitor<P: AsRef<Path>>(&mut self, path: P) -> Result<(), Error> {
        if let Some(_) = self.monitor {
            return Ok(());
        }

        let profiles = self.profiles.clone();
        let res_available = self.res_available.clone();
        let res_cap = self.res_cap.clone();
        let res_remaining = self.res_remaining.clone();
        let broadcast = self.broadcast.clone();

        let monitor = FileMonitor::spawn(path, move |e| match e {
            DebouncedEvent::Write(p)
            // e.g. file re-created
            | DebouncedEvent::Create(p)
            // e.g. file permissions fixed
            | DebouncedEvent::Chmod(p)
            | DebouncedEvent::Rename(_, p)=> {
                let mut profs = profiles.lock().unwrap();
                *profs = match Profiles::load(&p) {
                    Ok(p) => p,
                    Err(e) => return log::warn!("Error reading hw profiles from {:?}: {:?}", p, e),
                };

                log::info!("Activating profile '{}'", profs.active);
                let mut res = profs.get_active().clone();
                Self::switch_resources(&mut res, &res_available, &res_cap, &res_remaining, &broadcast);
            }
            _ => (),
        })
        .map_err(Error::from)?;

        self.monitor = Some(monitor);
        Ok(())
    }

    pub fn switch_profile(&mut self, name: impl ToString) -> Result<(), Error> {
        let name = name.to_string();
        let mut res = {
            let mut profiles = self.profiles.lock().unwrap();
            profiles.set_active(&name)?.clone()
        };

        log::info!("Activating profile '{}'", name);
        Self::switch_resources(
            &mut res,
            &self.res_available,
            &self.res_cap,
            &self.res_remaining,
            &self.broadcast,
        );
        Ok(())
    }

    fn switch_resources(
        res: &mut Resources,
        res_available: &Resources,
        res_cap: &Arc<Mutex<Resources>>,
        res_remaining: &Arc<Mutex<Resources>>,
        broadcast: &broadcast::Sender<Event>,
    ) {
        let active = res.cap(&res_available);
        let mut cap = res_cap.lock().unwrap();
        let mut remaining = res_remaining.lock().unwrap();
        let delta = *cap - active;
        *cap = active;
        *remaining = *remaining - delta;

        if let Err(e) = broadcast.send(Event::ConfigurationChanged(cap.clone())) {
            log::error!("Error broadcasting configuration change: {:?}", e);
        }
    }
}

impl Manager {
    #[inline]
    pub fn event_receiver(&self) -> broadcast::Receiver<Event> {
        self.broadcast.subscribe()
    }

    #[inline]
    pub fn remaining(&self) -> Resources {
        self.res_remaining.lock().unwrap().clone()
    }

    pub fn allocate(&mut self, id: impl ToString, res: Resources) -> Result<(), Error> {
        let id = id.to_string();
        if self.res_alloc.contains_key(&id) {
            return Err(Error::AlreadyAllocated(id));
        }

        let mut pool = self.res_remaining.lock().unwrap();
        if *pool < res {
            return Err(Error::InsufficientResources);
        }
        *pool = *pool - res;
        self.res_alloc.insert(id, res);
        Ok(())
    }

    pub fn release(&mut self, id: impl ToString) -> Result<(), Error> {
        let id = id.to_string();
        match self.res_alloc.remove(&id) {
            Some(res) => {
                let mut pool = self.res_remaining.lock().unwrap();
                *pool = *pool + res;
            }
            _ => return Err(Error::NotAllocated(id)),
        }
        Ok(())
    }
}

/// Return free space on a partition with a given path
fn partition_space(path: &Path) -> Result<u64, Error> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use winapi::um::fileapi::GetDiskFreeSpaceExW;
        use winapi::um::winnt::PULARGE_INTEGER;

        let wide: Vec<u16> = path.as_os_str().encode_wide().collect();
        let mut free_bytes_available = 0u64;
        let mut total_number_of_bytes = 0u64;
        let mut total_number_of_free_bytes = 0u64;

        if unsafe {
            GetDiskFreeSpaceExW(
                wide.as_ptr(),
                &mut free_bytes_available as *mut u64 as PULARGE_INTEGER,
                &mut total_number_of_bytes as *mut u64 as PULARGE_INTEGER,
                &mut total_number_of_free_bytes as *mut u64 as PULARGE_INTEGER,
            )
        } == 0
        {
            log::error!("Unable to read free partition space for path '{:?}'", path);
        };

        Ok(free_bytes_available)
    }
    #[cfg(not(windows))]
    {
        use nix::sys::statvfs::statvfs;
        let stat =
            statvfs(path.as_os_str()).map_err(|e| sys_info::Error::General(e.to_string()))?;
        Ok(stat.blocks_available() as u64 * stat.fragment_size())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profiles() -> Arc<Mutex<Profiles>> {
        let active = DEFAULT_PROFILE_NAME.to_string();
        let resources = Resources {
            cpu_threads: 4,
            mem_gib: 8.,
            storage_gib: 100.,
        };
        let profiles = vec![(active.clone(), resources)].into_iter().collect();

        Arc::new(Mutex::new(Profiles { active, profiles }))
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
        let mut man = Manager {
            res_available: res.clone(),
            res_cap: Arc::new(Mutex::new(res.clone())),
            res_remaining: Arc::new(Mutex::new(res.clone())),
            res_alloc: HashMap::new(),
            profiles: profiles(),
            monitor: None,
            broadcast: broadcast::channel(1).0,
        };
        let alloc = Resources {
            cpu_threads: 1,
            mem_gib: 1.51,
            storage_gib: 12.37,
        };

        man.allocate("1", alloc.clone()).unwrap();
        man.allocate("2", alloc.clone()).unwrap();
        man.allocate("3", alloc.clone()).unwrap();
        man.release("1").unwrap();
        man.release("2").unwrap();
        man.release("3").unwrap();

        let remaining = man.res_remaining.lock().unwrap();
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
        let mut man = Manager {
            res_available: res.clone(),
            res_cap: Arc::new(Mutex::new(res.clone())),
            res_remaining: Arc::new(Mutex::new(res.clone())),
            res_alloc: HashMap::new(),
            profiles: profiles(),
            monitor: None,
            broadcast: broadcast::channel(1).0,
        };
        let alloc = Resources {
            cpu_threads: 1,
            mem_gib: 1.51,
            storage_gib: 12.37,
        };

        man.allocate("1", alloc.clone()).unwrap();
        assert!(man.allocate("1", alloc).is_err());
        assert!(man.release("2").is_err());
        assert!(man
            .allocate(
                "3",
                Resources {
                    cpu_threads: 1000,
                    mem_gib: 10000.,
                    storage_gib: 10000.,
                }
            )
            .is_err());
    }
}
