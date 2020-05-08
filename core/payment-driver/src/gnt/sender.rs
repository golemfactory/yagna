use crate::dao::payment::PaymentDao;
use crate::dao::transaction::TransactionDao;
use crate::ethereum::EthereumClient;
use crate::models::{TransactionEntity, TransactionStatus, TxType};
use crate::utils::{h256_from_hex, u256_from_big_endian_hex};
use crate::{PaymentDriverError, SignTx};
use actix::fut::Either;
use actix::prelude::*;
use chrono::Utc;
use ethereum_tx_sign::RawTransaction;
use ethereum_types::{Address, H256, U256};
use futures3::channel::oneshot;
use futures3::prelude::*;
use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;
use std::time::{Duration, Instant};
use web3::contract::tokens::Tokenize;
use web3::contract::Contract;
use web3::Transport;
use ya_persistence::executor::DbExecutor;

const NONCE_EXPIRE: Duration = Duration::from_secs(12);

#[derive(Clone)]
pub struct Reservation {
    pub reservation_id: u64,
    pub address: Address,
    pub nonces: Range<U256>,
    expire: Option<Instant>,
}

impl Reservation {
    pub fn is_valid(&self) -> bool {
        self.expire.map(|exp| exp > Instant::now()).unwrap_or(true)
    }

    fn lock(&mut self) {
        self.expire = None;
    }
}

pub struct TxReq {
    pub address: Address,
    pub count: usize,
}

impl Message for TxReq {
    type Result = Result<Reservation, PaymentDriverError>;
}

pub struct TxSave {
    pub reservation_id: u64,
    pub address: Address,
    pub tx_type: TxType,
    pub transactions: Vec<(RawTransaction, Vec<u8>)>,
}

impl TxSave {
    pub fn from_reservation(reservation: Reservation) -> Self {
        let reservation_id = reservation.reservation_id;
        let address = reservation.address;
        let transactions = Default::default();
        let tx_type = TxType::Transfer;
        TxSave {
            reservation_id,
            address,
            transactions,
            tx_type,
        }
    }
}

impl Message for TxSave {
    type Result = Result<Vec<H256>, PaymentDriverError>;
}

pub struct Retry {
    pub tx_id : String
}

impl Message for Retry {
    type Result = Result<bool, PaymentDriverError>;
}

pub struct TransactionSender {
    ethereum_client: Arc<EthereumClient>,
    nonces: HashMap<Address, U256>,
    next_reservation_id: u64,
    pending_reservations: Vec<(TxReq, oneshot::Sender<Reservation>)>,
    pending_confirmations : Vec<PendingConfirmation>,
    reservation: Option<Reservation>,
    db: DbExecutor,
}

impl TransactionSender {
    pub fn new(ethereum_client: Arc<EthereumClient>, db: DbExecutor) -> Addr<Self> {
        let me = TransactionSender {
            ethereum_client,
            db,
            nonces: Default::default(),
            next_reservation_id: 0,
            pending_reservations: Default::default(),
            pending_confirmations: Default::default(),
            reservation: None,
        };

        me.start()
    }
}

impl Actor for TransactionSender {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        self.start_confirmation_job(ctx);
        self.start_block_traces(ctx);
    }
}

impl TransactionSender {
    /// Finds next nonce for uninitialized address.
    ///
    /// TODO: Detect invalid transactions from db.
    fn read_init_nonce(
        &self,
        address: Address,
    ) -> impl Future<Output = Result<U256, PaymentDriverError>> + 'static {
        let db = self.db.clone();
        let client = self.ethereum_client.clone();
        let address_str = crate::utils::addr_to_str(&address);

        future::try_join(
            async move {
                Ok::<_, PaymentDriverError>(
                    db.as_dao::<TransactionDao>()
                        .get_used_nonces(address_str)
                        .await?
                        .into_iter()
                        .map(u256_from_big_endian_hex)
                        .max(),
                )
            },
            async move {
                client
                    .get_next_nonce(address)
                    .map_err(PaymentDriverError::from)
                    .await
            },
        )
        .and_then(|r| {
            future::ok(match r {
                (None, net_nonce) => net_nonce,
                (Some(v), net_nonce) => {
                    if v >= net_nonce {
                        v + 1
                    } else {
                        net_nonce
                    }
                }
            })
        })
    }

    fn nonce(
        &self,
        address: Address,
    ) -> impl ActorFuture<Actor = Self, Output = Result<U256, PaymentDriverError>> + 'static {
        if let Some(n) = self.nonces.get(&address) {
            fut::Either::Left(fut::ok(n.clone()))
        } else {
            fut::Either::Right(self.read_init_nonce(address.clone()).into_actor(self).then(
                move |r, act, _ctx| match r {
                    Ok(v) => {
                        let output = act.nonces.entry(address).or_insert(v).clone();
                        fut::ok(output)
                    }
                    Err(e) => fut::err(e),
                },
            ))
        }
    }

    fn new_reservation(
        &mut self,
        address: Address,
        count: usize,
        next_nonce: U256,
    ) -> &mut Reservation {
        debug_assert!(self.reservation.is_none());
        debug_assert_eq!(Some(&next_nonce), self.nonces.get(&address));
        let reservation_id = self.next_reservation_id;
        let nonces = Range {
            start: next_nonce,
            end: (next_nonce + count),
        };
        let expire = Some(Instant::now() + NONCE_EXPIRE);
        self.next_reservation_id += 1;
        self.reservation = Some(Reservation {
            reservation_id,
            address,
            nonces,
            expire,
        });
        self.reservation.as_mut().unwrap()
    }

    fn wake_pending_reservation(&mut self, ctx: &mut Context<Self>) {
        assert!(self.reservation.is_none());

        if let Some((tx, mut r)) = self.pending_reservations.pop() {
            let next_nonce = self.nonces.get(&tx.address).unwrap().clone();
            let reservation = self
                .new_reservation(tx.address, tx.count, next_nonce)
                .clone();
            let _ = ctx.spawn(
                async move {
                    if let Err(e) = r.send(reservation) {
                        log::error!("reservation lost");
                        true
                    } else {
                        false
                    }
                }
                .into_actor(self)
                .then(|resend, act, ctx| {
                    if resend {
                        act.wake_pending_reservation(ctx);
                    }
                    fut::ready(())
                }),
            );
        } else {
            log::debug!("no reservations pending");
        }
    }
}

impl Handler<TxReq> for TransactionSender {
    type Result = ActorResponse<Self, Reservation, PaymentDriverError>;

    fn handle(&mut self, msg: TxReq, ctx: &mut Self::Context) -> Self::Result {
        let address = msg.address.clone();
        let fut = self.nonce(address).then(move |r, act, ctx| {
            let next_nonce = match r {
                Ok(v) => v,
                Err(e) => return fut::Either::Right(fut::err(e)),
            };
            if act.reservation.is_some() {
                let (tx, rx) = oneshot::channel();
                act.pending_reservations.push((msg, tx));
                return fut::Either::Left(
                    async move {
                        let result = rx
                            .await
                            .map_err(|_| PaymentDriverError::FailedTransaction)?;
                        Ok(result)
                    }
                    .into_actor(act),
                );
            }
            let r = act.new_reservation(msg.address, msg.count, next_nonce);

            fut::Either::Right(fut::ok(r.clone()))
        });
        ActorResponse::r#async(fut)
    }
}

impl Handler<TxSave> for TransactionSender {
    type Result = ActorResponse<Self, Vec<H256>, PaymentDriverError>;

    fn handle(&mut self, msg: TxSave, ctx: &mut Self::Context) -> Self::Result {
        fn transaction_error(
            msg: &str,
        ) -> ActorResponse<TransactionSender, Vec<H256>, PaymentDriverError> {
            log::error!("tx-save fail: {}", msg);
            ActorResponse::reply(Err(PaymentDriverError::LibraryError(msg.to_owned())))
        }

        log::trace!(
            "checking reservation validity (r={}, a={})",
            msg.reservation_id,
            msg.address
        );
        let next_nonce = if let Some(r) = self.reservation.as_ref() {
            if !r.is_valid() {
                self.reservation = None;
                return transaction_error("reservation expired");
            }
            if r.reservation_id != msg.reservation_id {
                return transaction_error("invalid reservation id");
            }

            if r.address != msg.address {
                return transaction_error("invalid reservation address");
            }
            r.nonces.end
        } else {
            return transaction_error("reservation missing");
        };
        let chain_id = self.ethereum_client.chain_id();
        let now = Utc::now();
        let tx_type = msg.tx_type;
        let db_transactions: Vec<_> = msg
            .transactions
            .iter()
            .map(|(raw_tx, signature)| {
                crate::utils::raw_tx_to_entity(
                    raw_tx,
                    msg.address,
                    chain_id,
                    now,
                    signature,
                    tx_type,
                )
            })
            .collect();
        let encoded_transactions = msg
            .transactions
            .into_iter()
            .map(|(tx, sign)| {
                (
                    hex::encode(tx.hash(chain_id)),
                    tx.encode_signed_tx(sign, chain_id),
                )
            })
            .collect::<Vec<_>>();

        let db = self.db.clone();
        let fut = {
            let db = db.clone();
            async move {
                db.as_dao::<TransactionDao>()
                    .insert_transactions(db_transactions.clone())
                    .await?;
                Ok::<_, PaymentDriverError>(db_transactions)
            }
        }
        .into_actor(self)
        .then(move |r, act, ctx| {
            let reservation = act.reservation.take().unwrap();
            let transactions = match r {
                Err(e) => return fut::Either::Right(fut::err(e)),
                Ok(v) => v,
            };
            act.nonces.insert(reservation.address, next_nonce);
            act.wake_pending_reservation(ctx);
            let client = act.ethereum_client.clone();
            let fut = async move {
                let mut result = Vec::new();
                for (tx_id, tx_data) in encoded_transactions {
                    match client.send_tx(tx_data).await {
                        Ok(tx_hash) => {
                            // TODO: remove unwrap
                            db.as_dao::<TransactionDao>()
                                .update_tx_sent(tx_id, hex::encode(&tx_hash))
                                .await
                                .unwrap();
                            result.push(tx_hash);
                        }
                        Err(e) => {
                            db.as_dao::<TransactionDao>()
                                .update_tx_status(tx_id, TransactionStatus::Failed.into())
                                .await
                                .unwrap();
                        }
                    }
                }
                Ok(result)
            };

            fut::Either::Left(fut.into_actor(act))
        });

        ActorResponse::r#async(fut)
    }
}

impl Handler<Retry> for TransactionSender {
    type Result = ActorResponse<Self, bool, PaymentDriverError>;

    fn handle(&mut self, msg: Retry, ctx: &mut Self::Context) -> Self::Result {
        let db = self.db.clone();
        let client = self.ethereum_client.clone();
        let chain_id = client.chain_id();
        // TODO: catch diffrent states.
        let fut = async move {
            if let Some(tx) = db.as_dao::<TransactionDao>().get(msg.tx_id).await? {
                let tx_id : &str = tx.tx_id.as_ref();
                let raw_tx: RawTransaction = serde_json::from_str(tx.encoded.as_str()).unwrap();
                let signature = hex::decode(&tx.signature).unwrap();
                let signed_tx = raw_tx.encode_signed_tx(signature, chain_id);
                let hash = client.send_tx(signed_tx).await?;
                log::info!("resend transaciotn: {} tx={:?}", tx_id, hash);
                Ok(true)
            }
            else {
                Err(PaymentDriverError::UnknownTransaction)
            }
        }.into_actor(self);
        ActorResponse::r#async(fut)
    }
}

pub struct Builder {
    address: Address,
    gas_price: U256,
    chain_id: u64,
    tx: Vec<RawTransaction>,
}

impl Builder {
    pub fn new(address: Address, gas_price: U256, chain_id: u64) -> Self {
        let tx = Default::default();
        Builder {
            chain_id,
            address,
            gas_price,
            tx,
        }
    }

    pub fn push<T: Transport, P: Tokenize>(
        &mut self,
        contract: &Contract<T>,
        func: &str,
        params: P,
        gas: U256,
    ) -> &mut Self {
        let data = contract.encode(func, params).unwrap();
        let gas_price = self.gas_price;
        let nonce = Default::default();
        let tx = RawTransaction {
            nonce,
            to: Some(contract.address()),
            value: U256::from(0),
            gas_price,
            gas,
            data,
        };
        self.tx.push(tx);
        self
    }

    pub fn send_to<'a>(
        self,
        sender: Addr<TransactionSender>,
        sign_tx: SignTx<'a>,
    ) -> impl Future<Output = Result<Vec<H256>, PaymentDriverError>> + 'a {
        let mut me = self;
        async move {
            let r = sender
                .send(TxReq {
                    address: me.address,
                    count: me.tx.len(),
                })
                .await??;
            let mut nx = r.nonces.clone();
            let mut tx_save = TxSave::from_reservation(r);
            for mut tx in me.tx {
                tx.nonce = nx.start;
                nx.start += 1.into();
                let tx_hash = tx.hash(me.chain_id);
                let signature = sign_tx(tx_hash.clone()).await;
                tx_save.transactions.push((tx, signature))
            }
            assert_eq!(nx.start, nx.end);

            let tx = sender.send(tx_save).await??;
            Ok(tx)
        }
    }
}

/*
async fn send_created_txs(db: DbExecutor, tx_sender: TxSender) -> PaymentDriverResult<()> {
    let tx_sender = tx_sender;
    let txs = get_created_txs(&db).await?;
    log::info!("Trying to send {:?} created transactions...", txs.len());
    for tx in txs.iter() {
        log::debug!("Trying to send: {:?}", tx);
        let raw_tx: RawTransaction = serde_json::from_str(tx.encoded.as_str()).unwrap();
        let signature = hex::decode(&tx.signature).unwrap();
        let _ = send_created_tx(tx_sender.clone(), raw_tx, signature)
            .await
            .expect("Failed to send tx...");
    }
    Ok(())
}

async fn send_created_tx(
    tx_sender: TxSender,
    raw_tx: RawTransaction,
    signature: Vec<u8>,
) -> PaymentDriverResult<()> {
    let mut tx_sender = tx_sender;
    tokio::spawn(async move {
        let (resp_tx, resp_rx) = oneshot::channel();
        tx_sender
            .send(((raw_tx, signature), resp_tx))
            .await
            .ok()
            .unwrap();
        let tx_hash = resp_rx.await.unwrap().unwrap();
        log::info!("Sent tx: {:?}", tx_hash);
    });
    Ok(())
}

 */

// Confirmation logic
struct PendingConfirmation {
    tx_id : String,
    tx_hash : H256,
    confirmations : u64
}

impl TransactionSender {

    fn start_block_traces(&mut self, ctx : &mut Context<Self>) {
        let client = self.ethereum_client.clone();
        let fut = async move {
            let blocks = client.blocks().await.unwrap();
            blocks.try_for_each(|b| {
                log::info!("new block: {:?}", b);
                future::ok(())
            }).await.unwrap();

        }.into_actor(self);
        let _ = ctx.spawn(fut);
    }

    fn start_confirmation_job(&mut self, ctx : &mut Context<Self>) {
        let _ = ctx.run_interval(Duration::from_secs(30), |act, ctx| {
            if act.pending_confirmations.is_empty() {
                return;
            }
            let client = act.ethereum_client.clone();
            let tx_ids : Vec<_> = act.pending_confirmations.iter().map(|p| p.tx_hash).collect();
            let job = async move {
                let block_number = client.block_number().await?;
                for tx_id in tx_ids {
                    if let Some(t) = client.tx_block_number(tx_id).await? {
                        let confirmations = block_number - t;
                        log::info!("tx_id={}, confirmations={}", tx_id, confirmations)
                    }
                }
                Ok::<_, PaymentDriverError>(())
            }.into_actor(act).then(|r, act, _ctx| fut::ready(()));
            let _job_id = ctx.spawn(job);
        });
    }

}