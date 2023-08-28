use async_trait::async_trait;
use erc20_payment_lib::signer::SignerError;
use ethereum_types::H160;
use web3::types::{SignedTransaction, TransactionParameters};
use ya_payment_driver::bus;

pub struct IdentitySigner;

impl IdentitySigner {
    pub fn new() -> Self {
        IdentitySigner
    }
}

#[async_trait]
impl erc20_payment_lib::signer::Signer for IdentitySigner {
    async fn check_if_sign_possible(&self, pub_address: H160) -> Result<(), SignerError> {
        let pool = tokio_util::task::LocalPoolHandle::new(1);

        pool.spawn_pinned(move || async move {
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
                if addr == pub_address.as_bytes() {
                    return Ok(());
                }
            }

            Err(SignerError {
                message: format!("No matching unlocked identity for address {pub_address}"),
            })
        })
        .await
        .map_err(|e| SignerError {
            message: e.to_string(),
        })?
    }

    async fn sign(
        &self,
        pub_address: H160,
        tp: TransactionParameters,
    ) -> Result<SignedTransaction, SignerError> {
        let pool = tokio_util::task::LocalPoolHandle::new(1);

        pool.spawn_pinned(move || async move { todo!() })
            .await
            .map_err(|e| SignerError {
                message: e.to_string(),
            })?
    }
}
