use anyhow::{bail, Result};
use fs2::FileExt;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

const LOCK_FILE_EXT: &str = "lock";
const PID_FILE_EXT: &str = "pid";

pub struct ProcLock {
    dir: PathBuf,
    name: String,
    lock: Option<File>,
    lock_path: Option<PathBuf>,
    pid_path: Option<PathBuf>,
}

impl ProcLock {
    pub fn new<P: AsRef<Path>>(name: impl ToString, dir: P) -> Result<Self> {
        let dir = dir.as_ref();
        if !dir.is_dir() {
            bail!("{} is not a directory", dir.display());
        }

        Ok(Self {
            dir: dir.to_path_buf(),
            name: name.to_string(),
            lock: None,
            lock_path: None,
            pid_path: None,
        })
    }

    pub fn contains_locks<P: AsRef<Path>>(dir: P) -> Result<bool> {
        Ok(std::fs::read_dir(dir)?
            .filter_map(|r| r.map(|e| e.path()).ok())
            .filter(|p| !p.is_dir())
            .filter(|p| {
                p.extension()
                    .map(|e| {
                        let e = e.to_string_lossy().to_lowercase();
                        e.as_str() == LOCK_FILE_EXT
                    })
                    .unwrap_or(false)
            })
            .any(|p| match File::open(&p) {
                Ok(f) => f.try_lock_exclusive().is_err(),
                _ => true,
            }))
    }

    pub fn lock(mut self, pid: u32) -> Result<Self> {
        let (lock_file, lock_path) = self.lock_file(&self.name)?;
        if lock_file.try_lock_exclusive().is_err() {
            bail!("{} is already running", self.name);
        }

        let pid_path = self.pid_path(&self.name);
        let mut pid_file = match File::create(&pid_path) {
            Ok(f) => f,
            Err(_) => bail!("unable to create file: {}", pid_path.display()),
        };

        if let Err(e) = pid_file.write_all(pid.to_string().as_bytes()) {
            let _ = lock_file.unlock();
            bail!("unable to write to file {}: {}", pid_path.display(), e);
        }
        if let Err(e) = pid_file.flush() {
            let _ = lock_file.unlock();
            bail!("unable to flush file {}: {}", pid_path.display(), e);
        }

        self.lock.replace(lock_file);
        self.lock_path.replace(lock_path);
        self.pid_path.replace(pid_path);

        Ok(self)
    }

    pub fn read_pid(&self) -> Result<u32> {
        let (lock_file, _) = self.lock_file(&self.name)?;
        if lock_file.try_lock_exclusive().is_ok() {
            let _ = lock_file.unlock();
            bail!("{} is not running", self.name);
        }

        let pid_path = self.pid_path(&self.name);
        match std::fs::read_to_string(&pid_path) {
            Ok(s) => match s.parse() {
                Ok(p) => Ok(p),
                Err(_) => bail!("{} is not running", self.name),
            },
            Err(_) => bail!("{} is not running", self.name),
        }
    }

    fn lock_file(&self, name: impl ToString) -> Result<(File, PathBuf)> {
        let lock_path = self
            .dir
            .join(format!("{}.{}", name.to_string(), LOCK_FILE_EXT));
        let lock_file = if lock_path.is_file() {
            match File::open(&lock_path) {
                Ok(f) => f,
                Err(e) => bail!("cannot open lock file {}: {}", lock_path.display(), e),
            }
        } else {
            match File::create(&lock_path) {
                Ok(f) => f,
                Err(e) => bail!("cannot create lock file {}: {}", lock_path.display(), e),
            }
        };
        Ok((lock_file, lock_path))
    }

    #[inline]
    fn pid_path(&self, name: impl ToString) -> PathBuf {
        self.dir
            .join(format!("{}.{}", name.to_string(), PID_FILE_EXT))
    }
}

impl Drop for ProcLock {
    fn drop(&mut self) {
        let lock = self.lock.take();
        let lock_path = self.lock_path.take();
        let pid_path = self.pid_path.take();

        if let Some(f) = lock {
            if let Err(e) = f.unlock() {
                eprintln!("cannot unlock file: {}", e);
            }
            if let Err(e) = std::fs::remove_file(lock_path.unwrap()) {
                eprintln!("cannot remove lock file: {}", e);
            }
        }

        if let Some(p) = pid_path {
            if let Err(e) = std::fs::remove_file(p) {
                eprintln!("cannot remove pid file: {}", e);
            }
        }
    }
}
