use super::NsOptions;
use libc::{prctl, PR_SET_PDEATHSIG};
use nix::sched::{unshare, CloneFlags};
use nix::sys::signal::Signal::{SIGKILL, SIGTERM};
use nix::sys::wait::waitpid;
use nix::unistd::{fork, ForkResult, Gid, Uid};
use std::{fs, io};

fn nix_to_io(e: nix::Error) -> io::Error {
    Into::<io::Error>::into(e)
}

pub fn pre_exec(options: NsOptions) -> io::Result<()> {
    let uid = Uid::current();
    let gid = Gid::current();
    let mut flags = CloneFlags::CLONE_NEWUSER | CloneFlags::CLONE_NEWPID;
    if options.procfs {
        flags |= CloneFlags::CLONE_NEWNS;
    }
    unshare(flags).map_err(nix_to_io)?;
    fs::write("/proc/self/setgroups", "deny")?;
    fs::write("/proc/self/uid_map", format!("{} {} 1", uid, uid))?;
    fs::write("/proc/self/gid_map", format!("{} {} 1", gid, gid))?;
    if options.fork {
        match unsafe { fork().map_err(nix_to_io)? } {
            ForkResult::Parent { child, .. } => {
                unsafe {
                    prctl(PR_SET_PDEATHSIG, SIGTERM);
                }
                let _ = waitpid(child, None).map_err(nix_to_io)?;
                std::process::exit(0);
            }
            _ => {
                unsafe {
                    prctl(PR_SET_PDEATHSIG, SIGKILL);
                }
                if options.procfs {
                    nix::mount::mount::<str, _, str, str>(
                        None,
                        "/proc",
                        None,
                        nix::mount::MsFlags::MS_PRIVATE | nix::mount::MsFlags::MS_REC,
                        None,
                    )
                    .unwrap();
                    nix::mount::mount::<str, _, str, str>(
                        Some("proc"),
                        "/proc",
                        Some("proc"),
                        nix::mount::MsFlags::MS_NOSUID
                            | nix::mount::MsFlags::MS_NOEXEC
                            | nix::mount::MsFlags::MS_NODEV,
                        None,
                    )
                    .unwrap();
                }
            }
        }
    }
    Ok(())
}
