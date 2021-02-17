/*
    Wallet functions on zksync.
*/

// External crates
use bigdecimal::{BigDecimal, Zero};
use chrono::{Duration, Utc};
use num_bigint::BigUint;
use std::str::FromStr;
use std::time;
use tokio::time::delay_for;
use zksync::operations::SyncTransactionHandle;
use zksync::types::BlockStatus;
use zksync::zksync_types::{tx::TxHash, Address, Nonce, TxFeeTypes};
use zksync::{
    provider::get_rpc_addr,
    provider::{Provider, RpcProvider},
    Network as ZkNetwork, Wallet, WalletCredentials,
};
use zksync_eth_signer::EthereumSigner;

// Workspace uses
use ya_payment_driver::{
    db::models::Network,
    model::{AccountMode, GenericError, PaymentDetails},
};

// Local uses
use crate::{
    config::NetworkConfig,
    zksync::{signer::YagnaEthSigner, utils},
};

const MAX_FAUCET_REQUESTS: u32 = 6;

lazy_static! {
    static ref MIN_BALANCE: BigDecimal = BigDecimal::from(50);
    static ref MAX_WAIT: Duration = Duration::minutes(1);
}

#[derive(Debug, Clone)]
pub struct ZkSyncClient {
    provider: RpcProvider,
    api_url: String, // HACK: REST API is used to get tx data
    network: Network,
    config: NetworkConfig,
}

impl ZkSyncClient {
    pub fn new(network: Network, config: NetworkConfig) -> Self {
        let zk_network = get_zk_network(network);
        let rpc_addr = config
            .rpc_addr()
            .unwrap_or_else(|| get_rpc_addr(zk_network).to_owned());
        let api_url = rpc_addr.replace("/jsrpc", "/api/v0.1"); // HACK: REST API is used to get tx data
        let provider = RpcProvider::from_addr(rpc_addr, zk_network);
        Self {
            provider,
            api_url,
            network,
            config,
        }
    }
}

fn get_zk_network(network: Network) -> ZkNetwork {
    match network {
        Network::Rinkeby => ZkNetwork::Rinkeby,
        Network::Mainnet => ZkNetwork::Mainnet,
    }
}

fn hash_to_hex(hash: TxHash) -> String {
    // TxHash::to_string adds a prefix to the hex value
    hex::encode(hash.as_ref())
}

#[derive(serde::Deserialize)]
struct TxRespObj {
    to: String,
    from: String,
    amount: String,
    created_at: String,
}

impl ZkSyncClient {
    pub async fn account_balance(&self, address: &str) -> Result<BigDecimal, GenericError> {
        let pub_address = Address::from_str(&address[2..]).map_err(GenericError::new)?;
        let acc_info = self
            .provider
            .account_info(pub_address)
            .await
            .map_err(GenericError::new)?;
        let balance_com = acc_info
            .committed
            .balances
            .get(&self.config.token_zksync)
            .map(|x| x.0.clone())
            .unwrap_or(BigUint::zero());
        let balance = utils::big_uint_to_big_dec(balance_com);
        log::debug!(
            "account_balance. address={}, network={}, balance={}",
            address,
            &self.network,
            &balance
        );
        Ok(balance)
    }

    pub async fn init_wallet(&self, address: &str, mode: AccountMode) -> Result<(), GenericError> {
        log::debug!(
            "init_wallet. address={} mode={:?} network={}",
            address,
            mode,
            &self.network
        );

        if mode.contains(AccountMode::SEND) {
            let wallet = self.get_wallet(address).await?;
            self.unlock_wallet(wallet).await?;
        }
        Ok(())
    }

    pub async fn fund(&self, address: &str) -> Result<(), GenericError> {
        if self.network == Network::Mainnet {
            return Err(GenericError::new("Wallet can not be funded on mainnet."));
        }
        self.request_ngnt(address).await?;
        Ok(())
    }

    pub async fn exit(
        &self,
        sender: &str,
        to: Option<&str>,
        amount: Option<BigDecimal>,
    ) -> Result<String, GenericError> {
        let wallet = self.get_wallet(sender).await?;
        let tx_handle = self.withdraw(wallet, amount, to).await?;
        let tx_info = tx_handle
            .wait_for_commit()
            .await
            .map_err(GenericError::new)?;

        match tx_info.success {
            Some(true) => Ok(hash_to_hex(tx_handle.hash())),
            Some(false) => Err(GenericError::new(
                tx_info
                    .fail_reason
                    .unwrap_or("Unknown failure reason".to_string()),
            )),
            None => Err(GenericError::new("Transaction time-outed")),
        }
    }

    pub async fn get_nonce(&self, address: &str) -> u32 {
        let addr = match Address::from_str(&address[2..]) {
            Ok(a) => a,
            Err(e) => {
                log::error!("Unable to parse address, failed to get nonce. {:?}", e);
                return 0;
            }
        };
        let account_info = match self.provider.account_info(addr).await {
            Ok(i) => i,
            Err(e) => {
                log::error!("Unable to get account info, failed to get nonce. {:?}", e);
                return 0;
            }
        };
        *account_info.committed.nonce
    }

    pub async fn make_transfer(
        &self,
        details: &PaymentDetails,
        nonce: u32,
    ) -> Result<String, GenericError> {
        log::debug!("make_transfer. {:?}", details);
        let amount = details.amount.clone();
        let amount = utils::big_dec_to_big_uint(amount)?;
        let amount = utils::pack_up(&amount);

        let sender = details.sender.clone();
        let wallet = self.get_wallet(&sender).await?;

        let balance = wallet
            .get_balance(BlockStatus::Committed, self.config.token_zksync.as_str())
            .await
            .map_err(GenericError::new)?;
        log::debug!("balance before transfer={}", balance);

        let transfer_builder = wallet
            .start_transfer()
            .nonce(Nonce(nonce))
            .str_to(&details.recipient[2..])
            .map_err(GenericError::new)?
            .token(self.config.token_zksync.as_str())
            .map_err(GenericError::new)?
            .amount(amount.clone());
        log::debug!(
            "transfer raw data. nonce={}, to={}, token={}, amount={}",
            nonce,
            &details.recipient,
            self.config.token_zksync,
            amount
        );
        let transfer = transfer_builder.send().await.map_err(GenericError::new)?;

        let tx_hash = hex::encode(transfer.hash());
        log::info!("Created zksync transaction with hash={}", tx_hash);
        Ok(tx_hash)
    }

    pub async fn check_tx(&self, tx_hash: &str) -> Option<Result<(), String>> {
        let tx_hash = format!("sync-tx:{}", tx_hash);
        let tx_hash = TxHash::from_str(&tx_hash).unwrap();
        let tx_info = self.provider.tx_info(tx_hash).await.unwrap();
        log::trace!("tx_info: {:?}", tx_info);
        match tx_info.success {
            None => None,
            Some(true) => Some(Ok(())),
            Some(false) => match tx_info.fail_reason {
                Some(err) => Some(Err(err)),
                None => Some(Err("Unknown failure".to_string())),
            },
        }
    }

    pub async fn verify_tx(&self, tx_hash: &str) -> Result<PaymentDetails, GenericError> {
        let req_url = format!("{}/transactions_all/{}", self.api_url, tx_hash);
        log::debug!("Request URL: {}", &req_url);

        let client = awc::Client::new();
        let response = client
            .get(req_url)
            .send()
            .await
            .map_err(GenericError::new)?
            .body()
            .await
            .map_err(GenericError::new)?;
        let response = String::from_utf8_lossy(response.as_ref());
        log::trace!("Request response: {}", &response);
        let v: TxRespObj = serde_json::from_str(&response).map_err(GenericError::new)?;

        let recipient = v.to;
        let sender = v.from;
        let amount =
            utils::big_uint_to_big_dec(BigUint::from_str(&v.amount).map_err(GenericError::new)?);
        let date_str = format!("{}Z", v.created_at);
        let date = Some(chrono::DateTime::from_str(&date_str).map_err(GenericError::new)?);
        let details = PaymentDetails {
            recipient,
            sender,
            amount,
            date,
        };
        log::debug!("PaymentDetails from server: {:?}", &details);

        Ok(details)
    }

    async fn get_wallet(
        &self,
        address: &str,
    ) -> Result<Wallet<YagnaEthSigner, RpcProvider>, GenericError> {
        log::debug!("get_wallet {:?}", address);
        let addr = Address::from_str(&address[2..]).map_err(GenericError::new)?;
        let signer = YagnaEthSigner::new(addr);
        let credentials =
            WalletCredentials::from_eth_signer(addr, signer, get_zk_network(self.network))
                .await
                .map_err(GenericError::new)?;
        let wallet = Wallet::new(self.provider.clone(), credentials)
            .await
            .map_err(GenericError::new)?;
        Ok(wallet)
    }

    async fn unlock_wallet<S: EthereumSigner + Clone, P: Provider + Clone>(
        &self,
        wallet: Wallet<S, P>,
    ) -> Result<(), GenericError> {
        log::debug!("unlock_wallet");
        if !wallet
            .is_signing_key_set()
            .await
            .map_err(GenericError::new)?
        {
            log::info!("Unlocking wallet... address = {}", wallet.signer.address);

            let unlock = wallet
                .start_change_pubkey()
                .fee_token(self.config.token_zksync.as_str())
                .map_err(|e| GenericError::new(format!("Failed to create change_pubkey request: {}", e)))?
                .send()
                .await
                .map_err(|e| GenericError::new(format!("Failed to send change_pubkey request: '{}'. HINT: Did you run `yagna payment fund` and follow the instructions?", e)))?;
            // DO WE NEED DEBUG? log::debug!("Unlock tx: {:?}", unlock);
            log::info!("Unlock send. tx_hash= {}", unlock.hash().to_string());

            let tx_info = unlock.wait_for_commit().await.map_err(GenericError::new)?;
            log::debug!("tx_info = {:?}", tx_info);
            match tx_info.success {
                Some(true) => log::info!("Wallet successfully unlocked. address = {}", wallet.signer.address),
                Some(false) => return Err(GenericError::new(format!("Failed to unlock wallet. reason={}", tx_info.fail_reason.unwrap_or("Unknown reason".to_string())))),
                None => return Err(GenericError::new(format!("Unknown result from zksync unlock, please check your wallet on zkscan and try again. {:?}", tx_info))),
            }
        }
        Ok(())
    }

    pub async fn withdraw<S: EthereumSigner + Clone, P: Provider + Clone>(
        &self,
        wallet: Wallet<S, P>,
        amount: Option<BigDecimal>,
        recipient: Option<&str>,
    ) -> Result<SyncTransactionHandle<P>, GenericError> {
        let balance = wallet
            .get_balance(BlockStatus::Committed, self.config.token_zksync.as_str())
            .await
            .map_err(GenericError::new)?;
        info!(
            "Wallet funded with {} {} available for withdrawal",
            utils::big_uint_to_big_dec(balance.clone()),
            self.config.token
        );

        info!("Obtaining withdrawal fee");
        let address = wallet.address();
        let withdraw_fee = wallet
            .provider
            .get_tx_fee(
                TxFeeTypes::Withdraw,
                address,
                self.config.token_zksync.as_str(),
            )
            .await
            .map_err(GenericError::new)?
            .total_fee;
        info!(
            "Withdrawal transaction fee {:.5}",
            utils::big_uint_to_big_dec(withdraw_fee.clone())
        );

        let amount = match amount {
            Some(amount) => utils::big_dec_to_big_uint(amount)?,
            None => balance.clone(),
        };
        let withdraw_amount = std::cmp::min(balance - withdraw_fee, amount);
        info!(
            "Withdrawal of {:.5} {} started",
            utils::big_uint_to_big_dec(withdraw_amount.clone()),
            self.config.token
        );

        let recipient_address = match recipient {
            Some(addr) => Address::from_str(&addr[2..]).map_err(GenericError::new)?,
            None => address,
        };

        let withdraw_builder = wallet
            .start_withdraw()
            .token(self.config.token_zksync.as_str())
            .map_err(GenericError::new)?
            .amount(withdraw_amount.clone())
            .to(recipient_address);
        log::debug!(
            "Withdrawal raw data. token={}, amount={}, to={}",
            self.config.token,
            withdraw_amount,
            recipient_address
        );
        let withdraw_handle = withdraw_builder.send().await.map_err(GenericError::new)?;

        Ok(withdraw_handle)
    }

    pub async fn request_ngnt(&self, address: &str) -> Result<(), GenericError> {
        let balance = self.account_balance(address).await?;
        if balance >= *MIN_BALANCE {
            return Ok(());
        }

        log::info!(
            "Requesting tGLM from zkSync faucet... address = {}",
            address
        );

        for i in 0..MAX_FAUCET_REQUESTS {
            match self.faucet_donate(address).await {
                Ok(()) => break,
                Err(e) => {
                    // Do not warn nor sleep at the last try.
                    if i >= MAX_FAUCET_REQUESTS - 1 {
                        log::error!(
                            "Failed to request tGLM from Faucet, tried {} times.: {:?}",
                            MAX_FAUCET_REQUESTS,
                            e
                        );
                        return Err(e);
                    } else {
                        log::warn!(
                            "Retrying ({}/{}) to request tGLM from Faucet after failure: {:?}",
                            i + 1,
                            MAX_FAUCET_REQUESTS,
                            e
                        );
                        delay_for(time::Duration::from_secs(10)).await;
                    }
                }
            }
        }
        self.wait_for_glm(address).await?;
        Ok(())
    }

    async fn wait_for_glm(&self, address: &str) -> Result<(), GenericError> {
        log::info!("Waiting for tGLM from faucet...");
        let wait_until = Utc::now() + *MAX_WAIT;
        while Utc::now() < wait_until {
            if self.account_balance(address).await? >= *MIN_BALANCE {
                log::info!("Received tGLM from faucet.");
                return Ok(());
            }
            delay_for(time::Duration::from_secs(3)).await;
        }
        let msg = "Waiting for tGLM timed out.";
        log::error!("{}", msg);
        Err(GenericError::new(msg))
    }

    async fn faucet_donate(&self, address: &str) -> Result<(), GenericError> {
        let faucet_url = match self.config.resolve_faucet_url().await {
            Some(url) => url,
            None => return Err(GenericError::new("Faucet unavailable")),
        };
        // TODO: Reduce timeout to 20-30 seconds when transfer is used.
        let client = awc::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .finish();
        debug!("Faucet url: {}/{}", faucet_url, address);
        let response = client
            .get(format!("{}/{}", faucet_url, address))
            .send()
            .await
            .map_err(GenericError::new)?
            .body()
            .await
            .map_err(GenericError::new)?;
        let response = String::from_utf8_lossy(response.as_ref());
        log::debug!("Funds requested. Response = {}", response);
        // TODO: Verify tx hash
        Ok(())
    }
}
