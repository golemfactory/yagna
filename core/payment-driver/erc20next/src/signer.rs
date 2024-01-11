use std::sync::{Arc, Mutex};

use actix::{Actor, Addr, Context, Handler, Message, ResponseFuture};
use async_trait::async_trait;
use erc20_payment_lib::{contracts::DUMMY_RPC_PROVIDER, signer::SignerError};
use ethereum_types::{H160, H256};
use futures::FutureExt;
use web3::{
    signing::{Signature, SigningError},
    types::{Address, SignedTransaction, TransactionParameters},
};
use ya_client_model::NodeId;
use ya_payment_driver::bus;

#[derive(Default, Clone)]
struct DummyKeyState {
    message: Vec<u8>,
    signed: Vec<u8>,
}

/// Key for hacky interaction with the web3 API
///
/// We cannot sign the transaction here, as it needs to be done by GSB,
/// which cannot be done in the implementation of [`web3::signing::Key`]
/// either.
///
/// This key is to be used in two steps -- first one invokes `sign_transaction`
/// to capture the payload for signing. Then the payload has to be signed using
/// the identitiy API. Afterwards the signed message can be injected into the state,
/// and `sign_transaction` can be invoked again -- this time returning the pre-computed
/// signature.
///
/// This doesn't really depend on internal details of web3 and thus will work with future
/// versions of web3 as long as you pass in transactions consistently. This means you
/// cannot depend on `sign_transaction` populating the optional fields: `nonce`, `gas_price`
/// and `chain_id`.
#[derive(Clone)]
struct DummyKey {
    pub pub_address: Address,
    pub state: Arc<Mutex<DummyKeyState>>,
}

impl DummyKey {
    fn new(pub_address: Address) -> (DummyKey, Arc<Mutex<DummyKeyState>>) {
        let state = Arc::new(Mutex::new(DummyKeyState::default()));
        let key = DummyKey {
            pub_address,
            state: state.clone(),
        };
        (key, state)
    }
}

impl web3::signing::Key for DummyKey {
    fn sign(&self, _message: &[u8], _chain_id: Option<u64>) -> Result<Signature, SigningError> {
        panic!("DummyKey cannot sign legacy transactions");
    }

    fn sign_message(&self, message: &[u8]) -> Result<Signature, SigningError> {
        let mut state = self.state.lock().unwrap();

        if state.signed.is_empty() {
            state.message = message.to_vec();
            Ok(Signature {
                v: 0,
                r: Default::default(),
                s: Default::default(),
            })
        } else {
            log::debug!(
                "Signed message: ({}) {:?}",
                state.signed.len(),
                &state.signed
            );
            Ok(Signature {
                v: state.signed[0] as u64,
                r: H256::from_slice(&state.signed[1..33]),
                s: H256::from_slice(&state.signed[33..65]),
            })
        }
    }

    fn address(&self) -> Address {
        self.pub_address
    }
}

#[derive(Message)]
#[rtype(result = "Result<NodeId, SignerError>")]
struct MapAddressToNodeId {
    pub_address: H160,
}

#[derive(Message)]
#[rtype(result = "Result<SignedTransaction, SignerError>")]
struct Sign {
    pub_address: H160,
    tp: TransactionParameters,
}

pub struct IdentitySignerActor;

impl IdentitySignerActor {
    async fn map_address_to_node_id(pub_address: H160) -> Result<NodeId, SignerError> {
        let unlocked_identities =
            bus::list_unlocked_identities()
                .await
                .map_err(|e| SignerError {
                    message: e.to_string(),
                })?;

        for node_id in unlocked_identities {
            let addr = bus::get_pubkey(node_id).await.map_err(|e| SignerError {
                message: e.to_string(),
            })?;
            let address = *ethsign::PublicKey::from_slice(&addr)
                .map_err(|_| SignerError {
                    message: "Public address from bus::get_pubkey is invalid".to_string(),
                })?
                .address();

            if address == pub_address.as_bytes() {
                return Ok(node_id);
            }
        }

        Err(SignerError {
            message: format!("No matching unlocked identity for address {pub_address}"),
        })
    }
}

impl Actor for IdentitySignerActor {
    type Context = Context<Self>;
}

impl Handler<MapAddressToNodeId> for IdentitySignerActor {
    type Result = ResponseFuture<Result<NodeId, SignerError>>;

    fn handle(&mut self, msg: MapAddressToNodeId, _ctx: &mut Self::Context) -> Self::Result {
        Self::map_address_to_node_id(msg.pub_address).boxed_local()
    }
}

impl Handler<Sign> for IdentitySignerActor {
    type Result = ResponseFuture<Result<SignedTransaction, SignerError>>;

    fn handle(&mut self, msg: Sign, _ctx: &mut Self::Context) -> Self::Result {
        let (dummy_key, state) = DummyKey::new(msg.pub_address);
        let pub_address = msg.pub_address;
        let tp = msg.tp;

        async move {
            // We don't care about the result. This is only called
            // so that web3 computes the message to sign for us.
            DUMMY_RPC_PROVIDER
                .accounts()
                .sign_transaction(tp.clone(), dummy_key.clone())
                .await
                .ok();

            let message = state.lock().unwrap().message.clone();
            let node_id = Self::map_address_to_node_id(pub_address).await?;
            let signed = bus::sign(node_id, message).await.map_err(|e| SignerError {
                message: e.to_string(),
            })?;

            {
                let mut state = state.lock().unwrap();
                state.signed = signed;
            }

            DUMMY_RPC_PROVIDER
                .accounts()
                .sign_transaction(tp, dummy_key)
                .await
                .map_err(|e| SignerError {
                    message: e.to_string(),
                })
        }
        .boxed_local()
    }
}

pub struct IdentitySigner(Addr<IdentitySignerActor>);

impl IdentitySigner {
    pub fn new(addr: Addr<IdentitySignerActor>) -> Self {
        IdentitySigner(addr)
    }
}

#[async_trait]
impl erc20_payment_lib::signer::Signer for IdentitySigner {
    async fn check_if_sign_possible(&self, pub_address: H160) -> Result<(), SignerError> {
        self.0
            .send(MapAddressToNodeId { pub_address })
            .await
            .map_err(|e| SignerError {
                message: format!("Mailbox error: {e}"),
            })?
            .map(|_| ())
    }

    async fn sign(
        &self,
        pub_address: H160,
        tp: TransactionParameters,
    ) -> Result<SignedTransaction, SignerError> {
        self.0
            .send(Sign { pub_address, tp })
            .await
            .map_err(|e| SignerError {
                message: format!("Mailbox error: {e}"),
            })?
    }
}
