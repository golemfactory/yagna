use crate::dao::transaction::TransactionDao;
use crate::ethereum::EthereumClient;
use crate::models::{TransactionStatus, TxType};
use crate::utils::u256_from_big_endian_hex;
use crate::{utils, PaymentDriverError, SignTx};

use actix::prelude::*;
use chrono::Utc;
use ethereum_tx_sign::RawTransaction;
use ethereum_types::{Address, H256, U256};
use futures3::channel::oneshot;
use futures3::prelude::*;
use std::collections::{HashMap, HashSet};
use std::mem;
use std::ops::Range;
use std::sync::Arc;
use std::time::{Duration, Instant};
use web3::contract::tokens::Tokenize;
use web3::contract::Contract;
use web3::types::TransactionReceipt;
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
    type Result = Result<Vec<String>, PaymentDriverError>;
}

pub struct Retry {
    pub tx_id: String,
}

impl Message for Retry {
    type Result = Result<bool, PaymentDriverError>;
}

pub struct WaitForTx {
    pub tx_id: String,
}

impl Message for WaitForTx {
    type Result = Result<TransactionReceipt, PaymentDriverError>;
}

pub struct TransactionSender {
    ethereum_client: Arc<EthereumClient>,
    nonces: HashMap<Address, U256>,
    next_reservation_id: u64,
    pending_reservations: Vec<(TxReq, oneshot::Sender<Reservation>)>,
    pending_confirmations: Vec<PendingConfirmation>,
    receipt_queue: HashMap<String, oneshot::Sender<TransactionReceipt>>,
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
            receipt_queue: Default::default(),
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
        self.load_txs(ctx);
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

        if let Some((tx, r)) = self.pending_reservations.pop() {
            let next_nonce = self.nonces.get(&tx.address).unwrap().clone();
            let reservation = self
                .new_reservation(tx.address, tx.count, next_nonce)
                .clone();
            let _ = ctx.spawn(
                async move {
                    if let Err(_e) = r.send(reservation) {
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

    fn handle(&mut self, msg: TxReq, _ctx: &mut Self::Context) -> Self::Result {
        let address = msg.address.clone();
        let fut = self.nonce(address).then(move |r, act, _ctx| {
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
    type Result = ActorResponse<Self, Vec<String>, PaymentDriverError>;

    fn handle(&mut self, msg: TxSave, _ctx: &mut Self::Context) -> Self::Result {
        fn transaction_error(
            msg: &str,
        ) -> ActorResponse<TransactionSender, Vec<String>, PaymentDriverError> {
            log::error!("tx-save fail: {}", msg);
            ActorResponse::reply(Err(PaymentDriverError::LibraryError(msg.to_owned())))
        }

        log::trace!(
            "checking reservation validity (r={}, a={})",
            msg.reservation_id,
            msg.address
        );
        let next_nonce = if let Some(r) = self.reservation.as_mut() {
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
            r.lock();
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
            let _transactions = match r {
                Err(e) => return fut::Either::Right(fut::err(e)),
                Ok(v) => v,
            };
            act.nonces.insert(reservation.address, next_nonce);
            act.wake_pending_reservation(ctx);
            let client = act.ethereum_client.clone();
            let me = ctx.address();
            let fut = async move {
                let mut result = Vec::new();
                for (tx_id, tx_data) in encoded_transactions {
                    match client.send_tx(tx_data).await {
                        Ok(tx_hash) => {
                            // TODO: remove unwrap
                            db.as_dao::<TransactionDao>()
                                .update_tx_sent(tx_id.clone(), hex::encode(&tx_hash))
                                .await
                                .unwrap();
                            result.push(tx_id.clone());
                            me.do_send(PendingConfirmation {
                                tx_id,
                                tx_hash,
                                confirmations: 5,
                            });
                        }
                        Err(_e) => {
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

    fn handle(&mut self, msg: Retry, _ctx: &mut Self::Context) -> Self::Result {
        let db = self.db.clone();
        let client = self.ethereum_client.clone();
        let chain_id = client.chain_id();
        // TODO: catch diffrent states.
        let fut = async move {
            if let Some(tx) = db.as_dao::<TransactionDao>().get(msg.tx_id).await? {
                let tx_id: &str = tx.tx_id.as_ref();
                let raw_tx: RawTransaction = serde_json::from_str(tx.encoded.as_str()).unwrap();
                let signature = hex::decode(&tx.signature).unwrap();
                let signed_tx = raw_tx.encode_signed_tx(signature, chain_id);
                let hash = client.send_tx(signed_tx).await?;
                log::info!("resend transaciotn: {} tx={:?}", tx_id, hash);
                Ok(true)
            } else {
                Err(PaymentDriverError::UnknownTransaction)
            }
        }
        .into_actor(self);
        ActorResponse::r#async(fut)
    }
}

pub struct Builder {
    address: Address,
    gas_price: U256,
    chain_id: u64,
    tx_type: TxType,
    tx: Vec<RawTransaction>,
}

impl Builder {
    pub fn new(address: Address, gas_price: U256, chain_id: u64) -> Self {
        let tx = Default::default();
        let tx_type = TxType::Transfer;
        Builder {
            chain_id,
            address,
            gas_price,
            tx,
            tx_type,
        }
    }

    pub fn with_tx_type(mut self, tx_type: TxType) -> Self {
        self.tx_type = tx_type;
        self
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
    ) -> impl Future<Output = Result<Vec<String>, PaymentDriverError>> + 'a {
        let me = self;
        async move {
            let r = sender
                .send(TxReq {
                    address: me.address,
                    count: me.tx.len(),
                })
                .await??;
            let mut nx = r.nonces.clone();
            let mut tx_save = TxSave::from_reservation(r);
            tx_save.tx_type = me.tx_type;
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

// Confirmation logic
#[derive(Clone)]
struct PendingConfirmation {
    tx_id: String,
    tx_hash: H256,
    confirmations: u64,
}

impl Message for PendingConfirmation {
    type Result = ();
}

impl Handler<PendingConfirmation> for TransactionSender {
    type Result = ();

    fn handle(&mut self, msg: PendingConfirmation, _ctx: &mut Context<Self>) -> Self::Result {
        self.pending_confirmations.push(msg);
    }
}

impl TransactionSender {
    fn start_block_traces(&mut self, ctx: &mut Context<Self>) {
        let client = self.ethereum_client.clone();
        let fut = async move {
            let blocks = client.blocks().await.unwrap();
            blocks
                .try_for_each(|b| {
                    log::info!("new block: {:?}", b);
                    future::ok(())
                })
                .await
                .unwrap();
        }
        .into_actor(self);
        let _ = ctx.spawn(fut);
    }

    fn start_confirmation_job(&mut self, ctx: &mut Context<Self>) {
        let _ = ctx.run_interval(Duration::from_secs(30), |act, ctx| {
            if act.pending_confirmations.is_empty() {
                return;
            }
            let client = act.ethereum_client.clone();
            let confirmations_to_check: Vec<_> = act.pending_confirmations.clone();
            let job = async move {
                let block_number = client.block_number().await?;
                let mut resolved: HashSet<H256> = Default::default();
                for pending_confirmation in confirmations_to_check {
                    if let Some(tx_block_number) =
                        client.tx_block_number(pending_confirmation.tx_hash).await?
                    {
                        if tx_block_number < block_number {
                            let confirmations = block_number - tx_block_number + 1;
                            log::info!(
                                "tx_id={:?}, confirmations={}",
                                pending_confirmation.tx_id,
                                confirmations
                            );
                            if confirmations >= pending_confirmation.confirmations.into() {
                                resolved.insert(pending_confirmation.tx_hash);
                            }
                        }
                    }
                }
                Ok::<_, PaymentDriverError>(resolved)
            }
            .into_actor(act)
            .then(move |r, act, ctx| {
                let resolved = match r {
                    Err(e) => {
                        log::error!("failed to check confirmations: {}", e);
                        return fut::ready(());
                    }
                    Ok(v) => v,
                };
                let pending_confirmations =
                    mem::replace(&mut act.pending_confirmations, Vec::new());
                for pending_confirmation in pending_confirmations {
                    if resolved.contains(&pending_confirmation.tx_hash) {
                        act.tx_commit(pending_confirmation, ctx);
                    } else {
                        act.pending_confirmations.push(pending_confirmation);
                    }
                }
                fut::ready(())
            });
            let _job_id = ctx.spawn(job);
        });
    }

    fn tx_commit(&mut self, pending_confirmation: PendingConfirmation, ctx: &mut Context<Self>) {
        let client = self.ethereum_client.clone();
        let db = self.db.clone();
        let job = async move {
            let confirmation = match client
                .get_transaction_receipt(pending_confirmation.tx_hash)
                .await
            {
                Ok(Some(v)) => v,
                Ok(None) => {
                    log::error!("tx_save fail, missing receipt");
                    db.as_dao::<TransactionDao>()
                        .update_tx_status(
                            pending_confirmation.tx_id,
                            TransactionStatus::Failed.into(),
                        )
                        .await
                        .unwrap();
                    return None;
                }
                Err(e) => {
                    log::error!("tx_save fail, fail to get receipt: {}", e);
                    // Transaction state should left as `send`.
                    return None;
                }
            };
            db.as_dao::<TransactionDao>()
                .update_tx_status(
                    pending_confirmation.tx_id.clone(),
                    TransactionStatus::Confirmed.into(),
                )
                .await
                .unwrap();
            Some((pending_confirmation.tx_id, confirmation))
        }
        .into_actor(self)
        .then(|r, act, _ctx| {
            if let Some((tx_id, confirmation)) = r {
                log::info!("tx_id={}, processed", &tx_id);
                if let Some(sender) = act.receipt_queue.remove(&tx_id) {
                    if let Err(_e) = sender.send(confirmation) {
                        log::warn!("send tx_id={}, receipt failed", tx_id);
                    }
                }
            }
            fut::ready(())
        });

        let _job_id = ctx.spawn(job);
    }
}

impl Handler<WaitForTx> for TransactionSender {
    type Result = ActorResponse<Self, TransactionReceipt, PaymentDriverError>;

    fn handle(&mut self, msg: WaitForTx, _ctx: &mut Self::Context) -> Self::Result {
        if self
            .pending_confirmations
            .iter()
            .any(|p| p.tx_id == msg.tx_id)
        {
            let (tx, rx) = oneshot::channel();
            self.receipt_queue.insert(msg.tx_id, tx);
            let fut = async move { Ok(rx.await.map_err(PaymentDriverError::library_err_msg)?) };
            return ActorResponse::r#async(fut.into_actor(self));
        }
        // TODO: recover from db
        ActorResponse::reply(Err(PaymentDriverError::UnknownTransaction))
    }
}

// -- Processing tx from db
impl TransactionSender {
    fn load_txs(&self, ctx: &mut Context<Self>) {
        let db = self.db.clone();
        let me = ctx.address();
        let job = async move {
            let txs = db
                .as_dao::<TransactionDao>()
                .get_unconfirmed_txs()
                .await
                .unwrap();
            for tx in txs {
                let tx_id = tx.tx_id;
                let tx_hash = utils::h256_from_hex(tx.tx_hash.clone().unwrap());
                me.send(PendingConfirmation {
                    tx_id,
                    tx_hash,
                    confirmations: 5,
                })
                .await
                .unwrap();
            }
        }
        .into_actor(self);
        ctx.spawn(job);
    }
}
