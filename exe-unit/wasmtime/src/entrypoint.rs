use anyhow::{Result, Error};
use log::info;
use std::env;
use std::fs;
use std::path::{PathBuf, Path, Component};
use structopt::StructOpt;

use crate::manifest::{MountPoint, load_manifest};
use crate::wasmtime_unit::Wasmtime;


#[derive(StructOpt)]
pub enum Commands {
    Deploy {
        args: Vec<String>,
    },
    Run {
        args: Vec<String>,
    }
}


#[derive(StructOpt)]
pub struct CmdArgs {
    #[structopt(short = "w", long = "workdir")]
    workdir: PathBuf,
    #[structopt(short = "c", long = "cachedir")]
    cachedir: PathBuf,
    #[structopt(subcommand)]
    command: Commands,
}

pub struct DirectoryMount {
    pub host: PathBuf,
    pub guest: PathBuf,
}


pub struct ExeUnitMain;

impl ExeUnitMain {

    pub fn entrypoint(cmdline: CmdArgs) -> Result<()> {
        match cmdline.command {
            Commands::Run{args} => ExeUnitMain::run(&cmdline.workdir, &cmdline.cachedir, args),
            Commands::Deploy{args} => ExeUnitMain::deploy(&cmdline.workdir, &cmdline.cachedir, args),
        }
    }

    fn deploy(workdir: &Path, cachedir: &Path, args: Vec<String>) -> Result<()> {
        if args.len() != 1 {
            return Err(Error::msg(format!("Deploy: invalid number of args {}.", args.len())));
        }

        let image_url = args[0].clone();
        info!("Deploying image: {}", image_url);

        let image_path = download_image(&image_url, cachedir)?;
        let manifest = load_manifest(&image_path)?;
        let mounts = directories_mounts(workdir, &manifest.mount_points)?;

        create_mount_points(&mounts)?;

        let mut wasmtime = Wasmtime::new(mounts);
        Ok(wasmtime.deploy(&image_path)?)
    }

    fn run(workdir: &Path, cachedir: &Path, args: Vec<String>) -> Result<()> {
        info!("Called run command");
        Ok(())
    }
}

fn download_image(url: &str, cachedir: &Path) -> Result<PathBuf> {
    unimplemented!()
}

fn create_mount_points(mounts: &Vec<DirectoryMount>) -> Result<()> {
    for mount in mounts.iter() {
        fs::create_dir_all(&mount.host)?
    }
    Ok(())
}

fn directories_mounts(workdir: &Path, mount_points: &Vec<MountPoint>) -> Result<Vec<DirectoryMount>> {
    mount_points.iter()
        .map(|mount_point|{
            let mount = mount_point.path();
            let host_path = workdir.join(mount);

            validate_path(mount)?;
            Ok(DirectoryMount{host: host_path, guest: PathBuf::from(mount)})
        })
        .collect()
}

fn validate_path(path: &str) -> Result<()> {
    // Protect ExeUnit from directory traversal attack.
    // Wasm can access only paths inside working directory.
    let mut path = PathBuf::from(path);
    for component in path.components() {
        match component {
            Component::RootDir | Component::Prefix{..} => {
                return Err(Error::msg(format!("Expected relative path instead of [{}].", path.display())));
            },
            Component::ParentDir{..} => {
                return Err(Error::msg(format!("Path [{}] contains illegal '..' component.", path.display())))
            },
            Component::CurDir => {
                return Err(Error::msg(format!("Path [{}] contains illegal '.' component.", path.display())))
            },
            _ => ()
        }
    }
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_validation() {
        assert_eq!(validate_path("/path/path").is_err(), true);
        assert_eq!(validate_path("path/path/path").is_err(), false);
        assert_eq!(validate_path("path/../path").is_err(), true);
        assert_eq!(validate_path("./path/../path").is_err(), true);
        assert_eq!(validate_path("./path/path").is_err(), true);
    }
}
