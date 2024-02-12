use crate::startup_config::ProviderConfig;
use structopt::StructOpt;
use ya_utils_path::data_dir::DataDir;

fn check_cert_perm(data_dir: &DataDir) -> anyhow::Result<()> {
    static DEFAULT_CONTENTS: &[u8] = include_bytes!("default-cert-permissions.json");

    let cert_perm_dir = data_dir
        .get_or_create()?
        .join("cert-dir/cert-permissions.json");
    if cert_perm_dir.exists() && cert_perm_dir.is_file() {
        let bytes = std::fs::read(&cert_perm_dir)?;
        if bytes != DEFAULT_CONTENTS {
            println!("Warning: User-modified cert-permissions were detected.");
            println!("         They are now superseded by outbound rules.");
            println!("         Please migrate your permissions to outbound rules manually.");
            println!("         Currently the file contains:");
            println!("{}", std::str::from_utf8(&bytes)?);
        } else {
            std::fs::remove_file(&cert_perm_dir)?;
        }
    }

    Ok(())
}

#[derive(StructOpt, Clone, Debug)]
pub struct PreInstallConfig {}

impl PreInstallConfig {
    pub fn run(&self, config: ProviderConfig) -> anyhow::Result<()> {
        check_cert_perm(&config.data_dir)?;

        Ok(())
    }
}
