use anyhow::Context;
use bigdecimal::{BigDecimal, Zero};
use std::borrow::BorrowMut;
use std::collections::{btree_map, hash_map};
use std::collections::{BTreeMap, HashMap};
use uuid::Uuid;
use ya_client::model::payment as mpay;
use ya_client_model::payment::DocumentStatus;
use ya_core_model::net::NetApiError::NodeIdParseError;
use ya_core_model::payment::local as pay;
use ya_core_model::payment::public as ppay;
use ya_core_model::NodeId;
use ya_payment::dao::{AgreementDao, InvoiceDao, OrderDao};
use ya_persistence::executor::DbExecutor;
use ya_service_bus::typed as bus;

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

#[actix_rt::main]
async fn main() -> anyhow::Result<()> {
    let owner_id: NodeId = "0x206bfe4f439a83b65a5b9c2c3b1cc6cb49054cc4".parse()?;
    let database_url = format!(
        "file:{}/.local/share/yagna/payment.db",
        std::env::home_dir().unwrap().display()
    );
    let db = DbExecutor::new(database_url)?;
    db.apply_migration(ya_payment::migrations::run_with_output)?;
    let ts = chrono::Utc::now() + chrono::Duration::days(-7);
    let invoices = db
        .as_dao::<InvoiceDao>()
        .get_for_node_id(
            "0x206bfe4F439a83b65A5B9c2C3B1cc6cB49054cc4".parse()?,
            Some(ts.naive_utc()),
            Some(1000),
        )
        .await?;

    let mut payments = HashMap::<(NodeId, NodeId, String), Payment>::new();

    let mut total_amount_i = BigDecimal::default();
    for invoice in invoices
        .into_iter()
        .filter(|i| i.status == DocumentStatus::Accepted)
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

    for ((payer_addr, payee_addr, payment_platform), payment) in payments {
        let payment_id = Uuid::new_v4().to_string();
        let mut order = Order {
            payer_addr,
            payee_addr,
            payment_platform: payment_platform.clone(),
            obligations: Default::default(),
            payments: Default::default(),
            amount: payment.amount.clone(),
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

        total_amount += &payment.amount;
        eprintln!("\n\n{}/{}/{}\n", payment_platform, payer_addr, payee_addr);
        eprintln!("  :: amount {}", payment.amount);
        for (peer, obligations) in payment.delivers {
            let mut payment_template = mpay::Payment {
                payment_id: payment_id.clone(),
                payer_id: payer_addr,
                payee_id: payee_addr,
                payer_addr: payer_addr.to_string(),
                payee_addr: payee_addr.to_string(),
                payment_platform: payment_platform.clone(),
                amount: payment.amount.clone(),
                timestamp: chrono::Utc::now(),
                agreement_payments: vec![],
                activity_payments: vec![],
                details: "".to_string(),
            };
            let _ = match item.items.entry(payee_addr.to_string()) {
                hash_map::Entry::Occupied(mut e) => e
                    .get_mut()
                    .1
                    .insert(peer, serde_json::to_string_pretty(&payment_template)?),
                hash_map::Entry::Vacant(e) => e
                    .insert((payment.amount.clone(), Default::default()))
                    .1
                    .insert(peer, serde_json::to_string_pretty(&payment_template)?),
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
            order.payments.push(payment_template);
        }
        //eprintln!("order={:?}", order);
    }
    for (k, v) in px {
        let id = db
            .as_dao::<OrderDao>()
            .new_batch_order(
                owner_id,
                k.payer_addr.to_string(),
                k.payment_platform,
                v.items,
            )
            .await?;
        eprintln!("order={}", id);
    }

    eprintln!("total={} / {}", total_amount, total_amount_i);
    Ok(())
}
