use crate::dao::transaction::TransactionDao;
use crate::gnt::ethereum::EthereumClient;
use crate::models::{PaymentEntity, TransactionStatus, TxType, TRANSFER_TX};
use crate::utils::{
    u256_from_big_endian_hex, PAYMENT_STATUS_FAILED, PAYMENT_STATUS_NOT_ENOUGH_FUNDS,
    PAYMENT_STATUS_NOT_ENOUGH_GAS,
};
use crate::{utils, GNTDriverError, GNTDriverResult};

use crate::dao::payment::PaymentDao;
use crate::gnt::config::EnvConfiguration;
use crate::gnt::{common, notify_payment, SignTx};
use crate::networks::Network;
use actix::prelude::*;
use bigdecimal::{BigDecimal, Zero};
use chrono::Utc;
use ethereum_tx_sign::RawTransaction;
use ethereum_types::{Address, H256, U256};
use futures3::channel::oneshot;
use futures3::future::Either;
use futures3::prelude::*;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::mem;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use web3::contract::tokens::Tokenize;
use web3::contract::Contract;
use web3::transports::Http;
use web3::types::TransactionReceipt;
use web3::Transport;
use ya_client_model::NodeId;
use ya_core_model::driver::PaymentConfirmation;
use ya_persistence::executor::DbExecutor;

const NONCE_EXPIRE: Duration = Duration::from_secs(12);
const GNT_TRANSFER_GAS: u32 = 55000;
const TRANSFER_CONTRACT_FUNCTION: &str = "transfer";
const CONFIRMATION_JOB_LAPSE: Duration = Duration::from_secs(10);

struct Accounts {
    accounts: HashMap<String, NodeId>,
}

impl Accounts {
    pub fn add_account(&mut self, account: NodeId) {
        self.accounts.insert(account.to_string(), account);
    }

    pub fn remove_account(&mut self, account: NodeId) {
        self.accounts.remove(&account.to_string());
    }

    pub fn list_accounts(&self) -> Vec<String> {
        self.accounts.keys().cloned().collect()
    }

    pub fn get_node_id(&self, account: &str) -> Option<NodeId> {
        self.accounts.get(account).cloned()
    }
}

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
    type Result = Result<Reservation, GNTDriverError>;
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
    type Result = Result<Vec<String>, GNTDriverError>;
}

pub struct Retry {
    pub tx_id: String,
}

impl Message for Retry {
    type Result = Result<bool, GNTDriverError>;
}

pub struct WaitForTx {
    pub tx_id: String,
}

impl Message for WaitForTx {
    type Result = Result<TransactionReceipt, GNTDriverError>;
}

pub struct TransactionSender {
    active_accounts: Rc<RefCell<Accounts>>,
    network: Network,
    ethereum_client: Arc<EthereumClient>,
    gnt_contract: Arc<Contract<Http>>,
    nonces: HashMap<Address, U256>,
    next_reservation_id: u64,
    pending_reservations: Vec<(TxReq, oneshot::Sender<Reservation>)>,
    pending_confirmations: Vec<PendingConfirmation>,
    receipt_queue: HashMap<String, oneshot::Sender<TransactionReceipt>>,
    reservation: Option<Reservation>,
    db: DbExecutor,
    required_confirmations: u64,
}

impl TransactionSender {
    pub fn new(
        network: Network,
        ethereum_client: Arc<EthereumClient>,
        gnt_contract: Arc<Contract<Http>>,
        db: DbExecutor,
        env: &EnvConfiguration,
    ) -> Addr<Self> {
        let active_accounts = Rc::new(RefCell::new(Accounts {
            accounts: Default::default(),
        }));
        let me = TransactionSender {
            active_accounts,
            network,
            ethereum_client,
            gnt_contract,
            db,
            nonces: Default::default(),
            next_reservation_id: 0,
            pending_reservations: Default::default(),
            pending_confirmations: Default::default(),
            receipt_queue: Default::default(),
            reservation: None,
            required_confirmations: env.required_confirmations,
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
        self.start_payment_job(ctx);
    }
}

impl TransactionSender {
    /// Finds next nonce for uninitialized address.
    ///
    /// TODO: Detect invalid transactions from db.
    fn read_init_nonce(
        &self,
        address: Address,
    ) -> impl Future<Output = Result<U256, GNTDriverError>> + 'static {
        let db = self.db.clone();
        let client = self.ethereum_client.clone();
        let address_str = crate::utils::addr_to_str(&address);
        let network = self.network;

        future::try_join(
            async move {
                let db_nonce = db
                    .as_dao::<TransactionDao>()
                    .get_used_nonces(address_str, network)
                    .await?
                    .into_iter()
                    .map(u256_from_big_endian_hex)
                    .max();
                log::trace!("DB nonce: {:?}", db_nonce);
                Ok::<_, GNTDriverError>(db_nonce)
            },
            async move {
                let client_nonce = client
                    .get_next_nonce(address)
                    .map_err(GNTDriverError::from)
                    .await;
                log::trace!("Client nonce: {:?}", client_nonce);
                client_nonce
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
    ) -> impl ActorFuture<Self, Output = Result<U256, GNTDriverError>> + 'static {
        if let Some(n) = self.nonces.get(&address) {
            Either::Left(fut::ok(n.clone()))
        } else {
            Either::Right(self.read_init_nonce(address.clone()).into_actor(self).then(
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
    type Result = ActorResponse<Self, Result<Reservation, GNTDriverError>>;

    fn handle(&mut self, msg: TxReq, _ctx: &mut Self::Context) -> Self::Result {
        let address = msg.address.clone();
        let fut = self.nonce(address).then(move |r, act, _ctx| {
            let next_nonce = match r {
                Ok(v) => v,
                Err(e) => return Either::Right(fut::err(e)),
            };
            if act.reservation.is_some() {
                let (tx, rx) = oneshot::channel();
                act.pending_reservations.push((msg, tx));
                return Either::Left(
                    async move {
                        let result = rx.await.map_err(|_| GNTDriverError::FailedTransaction)?;
                        Ok(result)
                    }
                    .into_actor(act),
                );
            }
            let r = act.new_reservation(msg.address, msg.count, next_nonce);

            Either::Right(fut::ok(r.clone()))
        });
        ActorResponse::r#async(fut)
    }
}

impl Handler<TxSave> for TransactionSender {
    type Result = ActorResponse<Self, Result<Vec<String>, GNTDriverError>>;

    fn handle(&mut self, msg: TxSave, _ctx: &mut Self::Context) -> Self::Result {
        fn transaction_error(
            msg: &str,
        ) -> ActorResponse<TransactionSender, Result<Vec<String>, GNTDriverError>> {
            log::error!("tx-save fail: {}", msg);
            ActorResponse::reply(Err(GNTDriverError::LibraryError(msg.to_owned())))
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
        let chain_id = self.network.chain_id();
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
        let sender = msg.address.clone();
        let encoded_transactions = msg
            .transactions
            .into_iter()
            .map(|(tx, sign)| {
                (
                    crate::utils::prepare_tx_id(&tx, chain_id, sender),
                    crate::eth_utils::encode_signed_tx(&tx, sign, chain_id),
                )
            })
            .collect::<Vec<_>>();

        db_transactions
            .iter()
            .for_each(|tx| log::trace!("Creating db transaction: {:?}", tx));
        let db = self.db.clone();
        let fut = {
            let db = db.clone();
            async move {
                db.as_dao::<TransactionDao>()
                    .insert_transactions(db_transactions.clone())
                    .await?;
                Ok::<_, GNTDriverError>(db_transactions)
            }
        }
        .into_actor(self)
        .then(move |r, act, ctx| {
            let reservation = act.reservation.take().unwrap();
            let _transactions = match r {
                Err(e) => return Either::Right(fut::err(e)),
                Ok(v) => v,
            };
            act.nonces.insert(reservation.address, next_nonce);
            act.wake_pending_reservation(ctx);
            let client = act.ethereum_client.clone();
            let me = ctx.address();
            let required_confirmations = act.required_confirmations;
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
                                confirmations: required_confirmations,
                            });
                        }
                        Err(e) => {
                            log::error!("Error sending transaction: {:?}", e);
                            db.as_dao::<TransactionDao>()
                                .update_tx_status(tx_id, TransactionStatus::Failed.into())
                                .await
                                .unwrap();
                        }
                    }
                }
                Ok(result)
            };

            Either::Left(fut.into_actor(act))
        });

        ActorResponse::r#async(fut)
    }
}

impl Handler<Retry> for TransactionSender {
    type Result = ActorResponse<Self, Result<bool, GNTDriverError>>;

    fn handle(&mut self, msg: Retry, _ctx: &mut Self::Context) -> Self::Result {
        let db = self.db.clone();
        let client = self.ethereum_client.clone();
        let chain_id = self.network.chain_id();
        // TODO: catch different states.
        let fut = async move {
            if let Some(tx) = db.as_dao::<TransactionDao>().get(msg.tx_id).await? {
                let tx_id: &str = tx.tx_id.as_ref();
                let raw_tx: RawTransaction = serde_json::from_str(tx.encoded.as_str()).unwrap();
                let signature = hex::decode(&tx.signature).unwrap();
                let signed_tx = crate::eth_utils::encode_signed_tx(&raw_tx, signature, chain_id);
                let hash = client.send_tx(signed_tx).await?;
                log::info!("resend transaction: {} tx={:?}", tx_id, hash);
                Ok(true)
            } else {
                Err(GNTDriverError::UnknownTransaction)
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
        let data = crate::eth_utils::contract_encode(contract, func, params).unwrap();
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
    ) -> impl Future<Output = Result<Vec<String>, GNTDriverError>> + 'a {
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
                let signature = sign_tx(crate::eth_utils::get_tx_hash(&tx, me.chain_id)).await;
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
            let blocks = match client.blocks().await {
                Ok(b) => b,
                Err(e) => {
                    log::warn!("Error getting blocks: {:?}", e);
                    return;
                }
            };
            let result = blocks
                .try_for_each(|b| {
                    log::trace!("new block: {:?}", b);
                    future::ok(())
                })
                .await;
            if let Err(e) = result {
                log::warn!("Error tracing blocks: {:?}", e);
            }
        }
        .into_actor(self);
        let _ = ctx.spawn(fut);
    }

    fn start_payment_job(&mut self, ctx: &mut Context<Self>) {
        let _ = ctx.run_interval(Duration::from_secs(30), |act, ctx| {
            for address in act.active_accounts.borrow().list_accounts() {
                log::trace!("payment job for: {:?}", address);
                match act.active_accounts.borrow().get_node_id(address.as_str()) {
                    None => continue,
                    Some(node_id) => {
                        let account = address.clone();
                        let network = act.network;
                        let client = act.ethereum_client.clone();
                        let gnt_contract = act.gnt_contract.clone();
                        let tx_sender = ctx.address();
                        let db = act.db.clone();
                        let sign_tx = utils::get_sign_tx(node_id);
                        tokio::task::spawn_local(async move {
                            process_payments(
                                account,
                                network,
                                client,
                                gnt_contract,
                                tx_sender,
                                db,
                                &sign_tx,
                            )
                            .await;
                        });
                    }
                }
            }
        });
    }

    fn start_confirmation_job(&mut self, ctx: &mut Context<Self>) {
        let _ = ctx.run_interval(CONFIRMATION_JOB_LAPSE, |act, ctx| {
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
                        if tx_block_number <= block_number {
                            // When any transaction is first broadcast to the blockchain it starts with zero confirmations.
                            // This number then increases as the information is added to the first block.
                            let confirmations = block_number - tx_block_number + 1;
                            log::info!(
                                "tx_hash={:?}, confirmations={}",
                                pending_confirmation.tx_hash,
                                confirmations
                            );
                            if confirmations >= pending_confirmation.confirmations.into() {
                                resolved.insert(pending_confirmation.tx_hash);
                            }
                        }
                    }
                }
                Ok::<_, GNTDriverError>(resolved)
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

            let _ = notify_tx_confirmed(db, pending_confirmation.tx_id.clone())
                .await
                .map_err(|e| log::error!("Error while notifying about tx: {:?}", e));

            Some((pending_confirmation.tx_id, confirmation))
        }
        .into_actor(self)
        .then(|r, act, _ctx| {
            if let Some((tx_id, confirmation)) = r {
                log::info!("tx_hash={}, processed", &confirmation.transaction_hash);
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
    type Result = ActorResponse<Self, Result<TransactionReceipt, GNTDriverError>>;

    fn handle(&mut self, msg: WaitForTx, _ctx: &mut Self::Context) -> Self::Result {
        if self
            .pending_confirmations
            .iter()
            .any(|p| p.tx_id == msg.tx_id)
        {
            let (tx, rx) = oneshot::channel();
            self.receipt_queue.insert(msg.tx_id, tx);
            let fut = async move { Ok(rx.await.map_err(GNTDriverError::library_err_msg)?) };
            return ActorResponse::r#async(fut.into_actor(self));
        }
        // TODO: recover from db
        ActorResponse::reply(Err(GNTDriverError::UnknownTransaction))
    }
}

// -- Processing tx from db
impl TransactionSender {
    fn load_txs(&self, ctx: &mut Context<Self>) {
        let db = self.db.clone();
        let me = ctx.address();
        let required_confirmations = self.required_confirmations;
        let network = self.network;
        let job = async move {
            let txs = db
                .as_dao::<TransactionDao>()
                .get_unconfirmed_txs(network)
                .await
                .unwrap();
            for tx in txs {
                let tx_id = tx.tx_id;
                let tx_hash = utils::h256_from_hex(tx.tx_hash.clone().unwrap());
                me.send(PendingConfirmation {
                    tx_id,
                    tx_hash,
                    confirmations: required_confirmations,
                })
                .await
                .unwrap();
            }
        }
        .into_actor(self);
        ctx.spawn(job);
    }
}

pub struct AccountLocked {
    pub identity: NodeId,
}

impl Message for AccountLocked {
    type Result = Result<(), GNTDriverError>;
}

impl Handler<AccountLocked> for TransactionSender {
    type Result = ActorResponse<Self, Result<(), GNTDriverError>>;

    fn handle(&mut self, msg: AccountLocked, _ctx: &mut Self::Context) -> Self::Result {
        self.active_accounts
            .borrow_mut()
            .remove_account(msg.identity);
        log::info!("Account: {:?} is locked", msg.identity.to_string());
        ActorResponse::reply(Ok(()))
    }
}

pub struct AccountUnlocked {
    pub identity: NodeId,
}

impl Message for AccountUnlocked {
    type Result = Result<(), GNTDriverError>;
}

impl Handler<AccountUnlocked> for TransactionSender {
    type Result = ActorResponse<Self, Result<(), GNTDriverError>>;

    fn handle(&mut self, msg: AccountUnlocked, _ctx: &mut Self::Context) -> Self::Result {
        self.active_accounts.borrow_mut().add_account(msg.identity);
        log::info!("Account: {:?} is unlocked", msg.identity.to_string());
        ActorResponse::reply(Ok(()))
    }
}

async fn process_payments(
    account: String,
    network: Network,
    client: Arc<EthereumClient>,
    gnt_contract: Arc<Contract<Http>>,
    tx_sender: Addr<TransactionSender>,
    db: DbExecutor,
    sign_tx: SignTx<'_>,
) {
    match db
        .as_dao::<PaymentDao>()
        .get_pending_payments(account.clone(), network)
        .await
    {
        Err(e) => log::error!(
            "Failed to fetch pending payments for {:?} : {:?}",
            account,
            e
        ),
        Ok(payments) => {
            if !payments.is_empty() {
                log::info!("Processing {} Payments", payments.len());
                log::debug!("Payments details: {:?}", payments);
            }
            for payment in payments {
                let _ = process_payment(
                    payment.clone(),
                    client.clone(),
                    gnt_contract.clone(),
                    tx_sender.clone(),
                    db.clone(),
                    sign_tx,
                )
                .await
                .map_err(|e| {
                    log::error!("Failed to process payment: {:?}, error: {:?}", payment, e)
                });
            }
        }
    };
}

async fn process_payment(
    payment: PaymentEntity,
    client: Arc<EthereumClient>,
    gnt_contract: Arc<Contract<Http>>,
    tx_sender: Addr<TransactionSender>,
    db: DbExecutor,
    sign_tx: SignTx<'_>,
) -> GNTDriverResult<()> {
    log::info!("Processing payment: {:?}", payment);
    let gas_price = client.get_gas_price().await?;
    let chain_id = payment.network.chain_id();
    match transfer_gnt(
        gnt_contract,
        tx_sender,
        utils::u256_from_big_endian_hex(payment.amount),
        utils::str_to_addr(&payment.sender)?,
        utils::str_to_addr(&payment.recipient)?,
        sign_tx,
        gas_price,
        chain_id,
    )
    .await
    {
        Ok(tx_id) => {
            db.as_dao::<PaymentDao>()
                .update_tx_id(payment.order_id, tx_id)
                .await?;
        }
        Err(e) => {
            db.as_dao::<PaymentDao>()
                .update_status(
                    payment.order_id,
                    match e {
                        GNTDriverError::InsufficientFunds => PAYMENT_STATUS_NOT_ENOUGH_FUNDS,
                        GNTDriverError::InsufficientGas => PAYMENT_STATUS_NOT_ENOUGH_GAS,
                        _ => PAYMENT_STATUS_FAILED,
                    },
                )
                .await?;
            log::error!("GLM transfer failed: {}", e);
            return Err(e);
        }
    }
    Ok(())
}

async fn transfer_gnt(
    gnt_contract: Arc<Contract<Http>>,
    tx_sender: Addr<TransactionSender>,
    gnt_amount: U256,
    address: Address,
    recipient: Address,
    sign_tx: SignTx<'_>,
    gas_price: U256,
    chain_id: u64,
) -> GNTDriverResult<String> {
    let gnt_balance =
        utils::big_dec_to_u256(common::get_gnt_balance(&gnt_contract, address).await?)?;

    if gnt_amount > gnt_balance {
        return Err(GNTDriverError::InsufficientFunds);
    }

    let mut batch = Builder::new(address, gas_price, chain_id);
    batch.push(
        &gnt_contract,
        TRANSFER_CONTRACT_FUNCTION,
        (recipient, gnt_amount),
        GNT_TRANSFER_GAS.into(),
    );
    let r = batch.send_to(tx_sender, sign_tx).await?;
    match r.into_iter().next() {
        Some(tx) => Ok(tx),
        None => Err(GNTDriverError::LibraryError(
            "GLM transfer failed".to_string(),
        )),
    }
}

async fn notify_tx_confirmed(db: DbExecutor, tx_id: String) -> GNTDriverResult<()> {
    let tx = match db.as_dao::<TransactionDao>().get(tx_id.clone()).await? {
        Some(tx) => {
            if tx.tx_type != TRANSFER_TX {
                return Ok(());
            }
            tx
        }
        None => {
            return Err(GNTDriverError::LibraryError(format!(
                "Unknown transaction: {:?}",
                tx_id
            )));
        }
    };

    let payments = db
        .as_dao::<PaymentDao>()
        .get_by_tx_id(tx_id.clone())
        .await?;
    assert_ne!(payments.len(), 0);
    let platform = payments[0].network.default_platform();

    let mut amount = BigDecimal::zero();
    for payment in payments.iter() {
        amount += utils::u256_to_big_dec(utils::u256_from_big_endian_hex(payment.amount.clone()))?;
    }

    let sender: String = payments[0].sender.clone();
    let recipient: String = payments[0].recipient.clone();

    let order_ids: Vec<String> = payments
        .into_iter()
        .map(|payment| payment.order_id)
        .collect();

    let confirmation = match tx.tx_hash {
        Some(tx_hash) => PaymentConfirmation {
            confirmation: hex::decode(tx_hash)?,
        },
        None => {
            return Err(GNTDriverError::LibraryError(format!(
                "Invalid tx state, tx_id: {:?}",
                tx_id
            )));
        }
    };

    notify_payment(amount, sender, recipient, platform, order_ids, confirmation).await
}
