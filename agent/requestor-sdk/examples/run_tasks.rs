use std::path::PathBuf;
use std::time::Duration;
use structopt::StructOpt;
use ya_agreement_utils::{constraints, ConstraintKey, Constraints};
use ya_requestor_sdk::{commands, CommandList, Image, Package::Archive, Requestor};

#[derive(StructOpt)]
struct Args {
    #[structopt(default_value = "rust-wasi-tutorial.zip")]
    package: PathBuf,
    #[structopt(long, default_value = "testnet")]
    subnet: String,
    #[structopt(long)]
    sgx: bool,
}

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::from_args();
    match args.sgx {
        true => Requestor::new(
            "My Requestor",
            Image::Sgx((0, 1, 0).into()),
            Archive(args.package),
        )
        .tee(),
        false => Requestor::new(
            "My Requestor",
            Image::WebAssembly((0, 1, 0).into()),
            Archive(args.package),
        ),
    }
    .with_subnet(args.subnet)
    .with_max_budget_ngnt(10)
    .with_timeout(Duration::from_secs(12 * 60))
    .with_constraints(constraints![
        "golem.inf.mem.gib" > 0.4,
        "golem.inf.storage.gib" > 0.1
    ])
    .with_tasks(vec!["1", "2"].into_iter().map(|i| {
        commands! {
            upload(format!("input-{}.txt", i), "/input/input.txt");
            run("main", "/input/input.txt", "/output/output.txt");
            download("/output/output.txt", format!("output-{}.txt", i))
        }
    }))
    .on_completed(|activity_id, output| {
        println!("{} => {:?}", activity_id, output);
    })
    .run()
    .await
}
