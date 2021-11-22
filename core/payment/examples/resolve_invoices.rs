use anyhow::Context;
use bigdecimal::{BigDecimal, ToPrimitive, Zero};
use chrono::{Duration, Utc};
use num_bigint::ToBigInt;
use std::borrow::BorrowMut;
use std::collections::{btree_map, hash_map};
use std::collections::{BTreeMap, HashMap};
use structopt::StructOpt;
use uuid::Uuid;
use ya_client::model::payment as mpay;
use ya_client_model::payment::DocumentStatus;
use ya_core_model::driver::{driver_bus_id, SchedulePayment};
use ya_core_model::net::NetApiError::NodeIdParseError;
use ya_core_model::payment::local as pay;
use ya_core_model::payment::public as ppay;
use ya_core_model::NodeId;
use ya_payment::dao::{AgreementDao, BatchDao, InvoiceDao, OrderDao};
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
        #[structopt(long, short = "n")]
        dry_run: bool,
        #[structopt(long)]
        incremental: bool,
        #[structopt(long, default_value = "0x206bfe4f439a83b65a5b9c2c3b1cc6cb49054cc4")]
        owner: NodeId,
        #[structopt(long, default_value = "erc20-mumbai-tglm")]
        payment_platform: String,
    },
    SendPayments {
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
        let database_url = format!(
            "file:{}/.local/share/yagna/payment.db",
            std::env::home_dir().unwrap().display()
        );
        let db = DbExecutor::new(database_url)?;
        db.apply_migration(ya_payment::migrations::run_with_output)?;
        db
    };

    match args.command {
        Command::Generate {
            dry_run,
            payment_platform,
            owner,
            incremental,
        } => generate(db, owner, payment_platform, dry_run, incremental).await?,
        Command::SendPayments { order_id } => send_payments(db, order_id).await?,
        Command::Run {
            owner,
            payment_platform,
            interval,
        } => {
            if let Some(duration) = interval {
                loop {
                    tokio::time::delay_for(duration.into()).await;
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

async fn list_dn(db: DbExecutor) -> anyhow::Result<()> {
    for (id, amount, prev_id) in db
        .as_dao::<BatchDao>()
        .list_debit_notes(
            "0xf7530cbcd3685a997b1e49974f68dbe94b1116be".parse()?,
            "erc20-mumbai-tglm".to_string(),
            chrono::Utc::now() + chrono::Duration::days(-30),
        )
        .await?
    {
        eprintln!("id={}, amount={}, prev_id={}", id, amount, prev_id);
    }
    Ok(())
}

struct Payment {
    amount: BigDecimal,
    delivers: HashMap<NodeId, Vec<Obligation>>,
}

#[derive(Debug, Clone)]
enum Obligation {
    Invoice {
        id: String,
        amount: BigDecimal,
        agreement_id: String,
    },
}

impl Payment {
    fn new() -> Self {
        Payment {
            amount: BigDecimal::default(),
            delivers: Default::default(),
        }
    }
}

#[derive(Debug)]
struct Order {
    payer_addr: NodeId,
    payee_addr: NodeId,
    payment_platform: String,
    amount: BigDecimal,
    payments: Vec<mpay::Payment>,
    obligations: Vec<Obligation>,
}

async fn generate(
    db: DbExecutor,
    owner_id: NodeId,
    payment_platform: String,
    dry_run: bool,
    incremental: bool,
) -> anyhow::Result<()> {
    let ts = chrono::Utc::now() + chrono::Duration::days(-7);

    let order_id = db
        .as_dao::<BatchDao>()
        .resolve(owner_id, owner_id.to_string(), payment_platform, ts)
        .await?;
    /*
    let invoices = db
        .as_dao::<InvoiceDao>()
        .get_for_node_id(owner_id, Some(ts.naive_utc()), Some(1000))
        .await?;

    let mut payments = HashMap::<(NodeId, NodeId, String), Payment>::new();

    let mut total_amount_i = BigDecimal::default();
    for invoice in invoices
        .into_iter()
        .filter(|i| i.status == DocumentStatus::Accepted)
        .filter(|i| i.payment_platform == payment_platform)
        .filter(|i| i.recipient_id == owner_id)
    {
        let agreement = db
            .as_dao::<AgreementDao>()
            .get(invoice.agreement_id.clone(), invoice.recipient_id)
            .await?
            .context("agreement exists")?;
        let amount_to_pay = agreement.total_amount_due.0 - agreement.total_amount_paid.0;
        if amount_to_pay < BigDecimal::from(0u32) {
            continue;
        }
        //assert_eq!(amount_to_pay, invoice.amount);
        let obligation = Obligation::Invoice {
            id: invoice.invoice_id,
            amount: amount_to_pay.clone(),
            agreement_id: invoice.agreement_id,
        };
        total_amount_i += &amount_to_pay;
        match payments.entry((
            invoice.payer_addr.parse()?,
            invoice.payee_addr.parse()?,
            invoice.payment_platform,
        )) {
            hash_map::Entry::Occupied(mut e) => {
                let mut payment = e.get_mut();
                payment.amount += &amount_to_pay;
                match payment.delivers.entry(invoice.issuer_id) {
                    hash_map::Entry::Occupied(mut e) => {
                        e.get_mut().push(obligation);
                    }
                    hash_map::Entry::Vacant(e) => {
                        e.insert(vec![obligation]);
                    }
                }
            }
            hash_map::Entry::Vacant(e) => {
                let mut payment = Payment::new();
                payment.amount = amount_to_pay.clone();
                let r = payment.delivers.insert(invoice.issuer_id, vec![obligation]);
                assert!(r.is_none());
                e.insert(payment);
            }
        }
    }
    let mut total_amount = BigDecimal::default();

    #[derive(Hash, Eq, PartialEq)]
    struct Key {
        payer_addr: NodeId,
        payment_platform: String,
    }

    struct Item {
        items: HashMap<String, (BigDecimal, HashMap<NodeId, String>)>,
    }

    let mut px: HashMap<Key, Item> = Default::default();
    let ten = BigDecimal::from(10u64);
    let mut presision = BigDecimal::from(1u64);
    for _ in 0..18 {
        presision *= &ten;
    }

    let mut sqls = Vec::new();
    let mut prev_amount_map = HashMap::<(NodeId, String), _>::new();
    let zero = BigDecimal::default();
    for ((payer_addr, payee_addr, payment_platform), payment) in payments {
        let key = (owner_id, payment_platform.clone());
        if !prev_amount_map.contains_key(&key) {
            prev_amount_map.insert(
                key.clone(),
                db.as_dao::<OrderDao>()
                    .get_batch_items(owner_id, payment_platform.clone())
                    .await?,
            );
        }
        let prev_amount = prev_amount_map.get(&key).unwrap();

        let payment_id = Uuid::new_v4().to_string();
        let mut payment_amount = if incremental {
            &payment.amount
                - prev_amount
                    .get(&(payer_addr.to_string(), payee_addr.to_string()))
                    .unwrap_or(&zero)
        } else {
            payment.amount.clone()
        };
        if payment_amount < zero {
            payment_amount = zero.clone();
        }
        let mut order = Order {
            payer_addr,
            payee_addr,
            payment_platform: payment_platform.clone(),
            obligations: Default::default(),
            payments: Default::default(),
            amount: payment_amount.clone(),
        };

        match px.entry(Key {
            payer_addr,
            payment_platform: payment_platform.clone(),
        }) {
            hash_map::Entry::Occupied(mut e) => (),
            hash_map::Entry::Vacant(e) => {
                e.insert(Item {
                    items: Default::default(),
                });
            }
        };

        let item = px
            .get_mut(&Key {
                payer_addr,
                payment_platform: payment_platform.clone(),
            })
            .unwrap();

        total_amount += &payment_amount;

        let sql_line = format!(
            r#"
        INSERT INTO payment (order_id, amount, gas, sender, recipient, payment_due_date, status, tx_id, network)
        VALUES ('{}', '{}', '0000000000000000000000000000000000000000000000000000000000000000', '{}', '{}', '2021-08-27 23:19:32.577052800', 99, null, 1);"#,
            Uuid::new_v4(),
            format!(
                "{:064x}",
                (&payment_amount * &presision).round(0).to_bigint().unwrap()
            ),
            owner_id,
            payee_addr
        );
        if payment_amount > zero {
            sqls.push(sql_line);
        }

        eprintln!("\n\n{}/{}/{}\n", payment_platform, payer_addr, payee_addr);
        eprintln!("  :: amount {} // {}", &payment_amount, &payment.amount);
        for (peer, obligations) in payment.delivers {
            let mut payment_template = mpay::Payment {
                payment_id: payment_id.clone(),
                payer_id: payer_addr,
                payee_id: payee_addr,
                payer_addr: payer_addr.to_string(),
                payee_addr: payee_addr.to_string(),
                payment_platform: payment_platform.clone(),
                amount: payment_amount.clone(),
                timestamp: chrono::Utc::now(),
                agreement_payments: vec![],
                activity_payments: vec![],
                details: "".to_string(),
            };

            eprintln!("  :: peer {} invoices {}", peer, obligations.len());
            for obligation in obligations {
                order.obligations.push(obligation.clone());
                match obligation {
                    Obligation::Invoice {
                        id,
                        amount,
                        agreement_id,
                    } => {
                        payment_template
                            .agreement_payments
                            .push(mpay::AgreementPayment {
                                agreement_id: agreement_id.to_string(),
                                amount: amount.clone(),
                                allocation_id: None,
                            });
                    }
                }
            }
            let _ = match item.items.entry(payee_addr.to_string()) {
                hash_map::Entry::Occupied(mut e) => e
                    .get_mut()
                    .1
                    .insert(peer, serde_json::to_string_pretty(&payment_template)?),
                hash_map::Entry::Vacant(e) => e
                    .insert((payment_amount.clone(), Default::default()))
                    .1
                    .insert(peer, serde_json::to_string_pretty(&payment_template)?),
            };
            order.payments.push(payment_template);
        }
        //eprintln!("order={:?}", order);
    }

    for sql in sqls {
        eprintln!("{}", sql);
    }

    if !dry_run {
        /*for (k, v) in px {
            let id = db
                .as_dao::<BatchDao>()
                .new_batch_order(
                    owner_id,
                    k.payer_addr.to_string(),
                    k.payment_platform,
                    v.items,
                )
                .await?;
            eprintln!("order={}", id);
        }
        todo!()
    }*/*/

    eprintln!("order={:?}", order_id);
    Ok(())
}

async fn send_payments(db: DbExecutor, order_id: String) -> anyhow::Result<()> {
    let (order, items) = db
        .as_dao::<BatchDao>()
        .get_unsent_batch_items(order_id.clone())
        .await?;
    eprintln!("got {} orders", items.len());
    let bus_id = driver_bus_id("erc20");
    for item in items {
        eprintln!("sending: {:?}", &item);
        let payment_order_id = bus::service(&bus_id)
            .call(SchedulePayment::new(
                item.amount.0,
                order.payer_addr.clone(),
                item.payee_addr.clone(),
                order.platform.clone(),
                chrono::Utc::now(),
            ))
            .await??;
        db.as_dao::<BatchDao>()
            .batch_order_item_send(order_id.clone(), item.payee_addr, payment_order_id)
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
        send_payments(db, order_id).await?;
    }
    Ok(())
}
