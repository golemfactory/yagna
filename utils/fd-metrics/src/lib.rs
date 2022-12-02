#![allow(clippy::needless_return)]

use std::{collections::HashMap, fmt::Display};

/// Type of a file descriptor corresponding to st_mode in stat
///
/// See https://man7.org/linux/man-pages/man2/lstat.2.html
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum FdType {
    Fifo,
    Chr,
    Dir,
    Blk,
    Reg,
    Lnk,
    Sock,
    Fmt,
    Other,
}

impl Display for FdType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use FdType::*;

        write!(
            f,
            "{}",
            match self {
                Fifo => "fifo",
                Chr => "chr",
                Dir => "dir",
                Blk => "blk",
                Reg => "reg",
                Lnk => "lnk",
                Sock => "sock",
                Fmt => "fmt",
                Other => "other",
            }
        )
    }
}

impl FdType {
    /// List of all enum variants
    pub fn all() -> [Self; 9] {
        use FdType::*;

        // Could use strum proc macro, but compilation times are already terrible enough
        [Fifo, Chr, Dir, Blk, Reg, Lnk, Sock, Fmt, Other]
    }
}

pub fn fd_metrics() -> HashMap<FdType, usize> {
    let mut result = HashMap::new();

    // populate all variants
    for fd_type in FdType::all() {
        result.insert(fd_type, 0);
    }

    let measured_fds = list_fds().into_iter().flat_map(fd_type);
    for fd_type in measured_fds {
        *result.get_mut(&fd_type).unwrap() += 1;
    }

    result
}

/// Maps fd to FdType
///
/// Uses fstat on unix, always returns None on Windows
#[allow(unused_variables)]
fn fd_type(fd: i32) -> Option<FdType> {
    #[cfg(target_family = "unix")]
    {
        use nix::sys::stat::SFlag;

        let stat = nix::sys::stat::fstat(fd).ok()?;

        use FdType::*;
        return Some(if stat.st_mode & SFlag::S_IFIFO.bits() != 0 {
            Fifo
        } else if stat.st_mode & SFlag::S_IFCHR.bits() != 0 {
            Chr
        } else if stat.st_mode & SFlag::S_IFDIR.bits() != 0 {
            Dir
        } else if stat.st_mode & SFlag::S_IFBLK.bits() != 0 {
            Blk
        } else if stat.st_mode & SFlag::S_IFREG.bits() != 0 {
            Reg
        } else if stat.st_mode & SFlag::S_IFLNK.bits() != 0 {
            Lnk
        } else if stat.st_mode & SFlag::S_IFSOCK.bits() != 0 {
            Sock
        } else if stat.st_mode & SFlag::S_IFMT.bits() != 0 {
            Fmt
        } else {
            Other
        });
    }

    #[cfg(target_os = "windows")]
    {
        return None;
    }
}

/// List of all open file descriptors
///
/// Reads from `/proc/self/fd` on linux, empty Vec on other systems.
///
/// This is the function you need to modify to support other *nix systems.
fn list_fds() -> Vec<i32> {
    #[cfg(target_os = "linux")]
    {
        let fd_dir = if let Ok(fd_dir) = std::fs::read_dir("/proc/self/fd/") {
            fd_dir
        } else {
            return Vec::new();
        };

        return fd_dir
            .flat_map(|e| e.ok())
            .map(|e| e.file_name())
            .flat_map(|fname| fname.into_string().ok())
            .flat_map(|fname| fname.parse::<i32>().ok())
            .collect();
    }

    #[cfg(not(all(target_os = "linux")))]
    {
        return Vec::new();
    }
}
