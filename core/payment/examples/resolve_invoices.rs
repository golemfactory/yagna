use structopt::StructOpt;
use ya_core_model::driver::{driver_bus_id, ScheduleDriverPayment};
use ya_core_model::NodeId;
use ya_payment::dao::BatchDao;
use ya_persistence::executor::DbExecutor;
use ya_service_bus::typed as bus;

#[derive(StructOpt)]
struct Args {
    #[structopt(subcommand)]
    command: Command,
}

#[derive(StructOpt)]
enum Command {
    Generate {
        #[structopt(long, default_value = "0x206bfe4f439a83b65a5b9c2c3b1cc6cb49054cc4")]
        owner: NodeId,
        #[structopt(long, default_value = "erc20-mumbai-tglm")]
        payment_platform: String,
    },
    SendPayments {
        #[structopt(long, default_value = "0x206bfe4f439a83b65a5b9c2c3b1cc6cb49054cc4")]
        owner: NodeId,
        #[structopt(long)]
        order_id: String,
    },
    Run {
        #[structopt(long)]
        owner: NodeId,
        #[structopt(long)]
        payment_platform: String,
        #[structopt(long)]
        interval: Option<humantime::Duration>,
    },
}

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    std::env::set_var("RUST_LOG", "debug");
    env_logger::init();

    let args = Args::from_args_safe()?;

    log::info!("test1");

    let db = {
        let database_url = "yagnadir/payment.db";
        let db = DbExecutor::new(database_url)?;
        db.apply_migration(ya_payment::migrations::run_with_output)?;
        db
    };

    match args.command {
        Command::Generate {
            payment_platform,
            owner,
        } => generate(db, owner, payment_platform).await?,
        Command::SendPayments { order_id, owner } => send_payments(db, owner, order_id).await?,
        Command::Run {
            owner,
            payment_platform,
            interval,
        } => {
            if let Some(duration) = interval {
                loop {
                    tokio::time::sleep(duration.into()).await;
                    log::info!("sending payments for {} {}", owner, payment_platform);
                    if let Err(e) = run(db.clone(), owner, payment_platform.clone()).await {
                        log::error!("failed to process order: {:?}", e);
                    }
                }
            } else {
                run(db, owner, payment_platform).await?;
            }
        }
    }
    Ok(())
}

async fn generate(
    db: DbExecutor,
    owner_id: NodeId,
    payment_platform: String,
) -> anyhow::Result<()> {
    let ts = chrono::Utc::now() + chrono::Duration::days(-7);

    let order_id = db
        .as_dao::<BatchDao>()
        .resolve(owner_id, owner_id.to_string(), payment_platform.clone(), ts)
        .await?;

    eprintln!("order={:?}", order_id);
    Ok(())
}

async fn send_payments(db: DbExecutor, owner: NodeId, order_id: String) -> anyhow::Result<()> {
    let (order, items) = db
        .as_dao::<BatchDao>()
        .get_unsent_batch_items(order_id.clone())
        .await?;
    eprintln!("got {} orders", items.len());
    let bus_id = driver_bus_id("erc20");
    for item in items {
        eprintln!("sending: {:?}", &item);
        let payment_order_id = bus::service(&bus_id)
            .call(ScheduleDriverPayment::new(
                item.amount.0,
                order.payer_addr.clone(),
                item.payee_addr.clone(),
                order.platform.clone(),
                None,
                chrono::Utc::now(),
            ))
            .await??;
        db.as_dao::<BatchDao>()
            .batch_order_item_send(order_id.clone(), owner, item.payee_addr, payment_order_id)
            .await?;
    }
    Ok(())
}

async fn run(db: DbExecutor, owner_id: NodeId, payment_platform: String) -> anyhow::Result<()> {
    let ts = chrono::Utc::now() + chrono::Duration::days(-15);

    if let Some(order_id) = db
        .as_dao::<BatchDao>()
        .resolve(owner_id, owner_id.to_string(), payment_platform, ts)
        .await?
    {
        log::info!("resolved order: {}", order_id);
        //nothing
        //send_payments(db, order_id).await?;
    }
    Ok(())
}
