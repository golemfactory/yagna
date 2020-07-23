use anyhow::{anyhow, Result};
use futures::{future::FutureExt, pin_mut, select};
use std::collections::HashMap;
use std::path::PathBuf;
use structopt::StructOpt;

use ya_agreement_utils::{constraints, ConstraintKey, Constraints};
use ya_requestor_sdk::{commands, CommandList, Image::GVMKit, Package, Requestor};

#[derive(StructOpt)]
struct Args {
    #[structopt(flatten)]
    package: Location,
    input: PathBuf,
}

#[derive(Debug, Clone, StructOpt)]
enum Location {
    Local { path: PathBuf },
    Url { url: String, digest: String },
}

impl From<Location> for Package {
    fn from(args: Location) -> Self {
        match args {
            Location::Local { path } => Package::Archive(path),
            Location::Url { digest, url } => Package::Url { digest, url },
        }
    }
}

#[actix_rt::main]
async fn main() -> Result<()> {
    let _ = dotenv::dotenv()?;
    env_logger::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let args = Args::from_args();
    let package = args.package.clone().into();

    let requestor = Requestor::new("My Requestor", GVMKit, package)
        .with_max_budget_gnt(5)
        .with_constraints(constraints![
            "golem.inf.mem.gib" > 0.5,
            "golem.inf.storage.gib" > 1.0
        ])
        .with_tasks(vec!["1"].into_iter().map(move |i| {
            commands! {
                upload(args.input.clone(), "/workdir/scene.blend");
                run("blender", "-b","/workdir/scene.blend", "-o", "/workdir/out##.png", "-E", "CYCLES", "-s", i, "-e", i, "-a", "-F", "PNG");
                run("mv", "/workdir/out*.png", "/workdir/output.png");
                download("/workdir/output.png", format!("output-{}.png", i))
            }
        }))
        .on_completed(|outputs: HashMap<String, String>| {
            for (prov_id, output) in outputs {
                println!("{} => {}", prov_id, output);
            }
        })
        .run()
        .fuse();
    let ctrl_c = actix_rt::signal::ctrl_c().fuse();

    pin_mut!(requestor, ctrl_c);

    select! {
        comp_res = requestor => comp_res,
        _ = ctrl_c => Err(anyhow!("interrupted: ctrl-c detected!")),
    }
}
