use structopt::StructOpt;
use ya_core_model::ethaddr::NodeId;
use ya_core_model::{appkey, identity};
use ya_service_bus::typed as bus;
use ya_service_bus::RpcEndpoint;

// Tool for generating JWT tokens signed with identity key.
#[derive(StructOpt)]
enum Args {
    List,
    Gen { from_key: String },
}

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::from_args();
    match args {
        Args::Gen { from_key } => {
            let key = bus::service(appkey::BUS_ID)
                .send(appkey::Get::with_key(from_key))
                .await?;
        }
        Args::List => todo!(),
    }
    eprintln!("ok");
    Ok(())
}
