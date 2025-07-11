use crate::config::DiscoveryConfig;
use crate::protocol::discovery::pow::solve_pow;

use anyhow::Result;
use bigdecimal::BigDecimal;
use futures::stream::{self, StreamExt};
use golem_base_sdk::Address;
use golem_base_sdk::{client::GolemBaseClient, Hash};
use serde::{Deserialize, Serialize};
use url::Url;

/// Response from the challenge endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChallengeResponse {
    /// List of challenges to solve
    pub challenge: Vec<[String; 2]>,
    /// Token to use for redeeming the solution
    pub token: String,
    /// Expiration timestamp for the challenge
    pub expires: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FaucetResponse {
    pub tx_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RedeemResponse {
    pub success: bool,
    pub token: String,
    pub expires: i64,
}

pub struct FaucetClient {
    config: DiscoveryConfig,
    client: GolemBaseClient,
}

impl FaucetClient {
    pub fn new(config: DiscoveryConfig, client: GolemBaseClient) -> Self {
        Self { config, client }
    }

    pub async fn fund_local_account(&self, address: Address) -> Result<()> {
        self.client
            .fund(address, BigDecimal::from(10))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fund local wallet: {}", e))
            .map(|_| ())
    }

    pub async fn fund_from_faucet(&self, address: &str) -> Result<()> {
        let faucet_url = self.config.get_faucet_url().join("/api/faucet")?;
        let request = serde_json::json!({
            "address": address
        });

        self.post::<_, ()>(&faucet_url, request).await
    }

    /// Computes solutions for the given challenge response
    pub async fn compute_solutions(
        &self,
        response: ChallengeResponse,
    ) -> Result<Vec<serde_json::Value>> {
        let total = response.challenge.len();
        let log_interval = total / 20;
        let mut solved = 0;

        log::info!(
            "GolemBase fund: Received {total} challenges to solve on {threads} threads",
            threads = self.config.get_pow_threads()
        );

        stream::iter(response.challenge)
            .map(|[hash, target]| {
                tokio::task::spawn_blocking(move || {
                    let solution = solve_pow(&hash, &target);
                    serde_json::json!([hash, target, solution])
                })
            })
            .buffer_unordered(self.config.get_pow_threads())
            .map(|result| {
                solved += 1;
                if solved % log_interval == 0 || solved == total {
                    log::debug!("GolemBase fund: Solved {solved}/{total} challenges");
                }
                result
            })
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| anyhow::anyhow!("Failed to compute PoW solutions: {e}"))
    }

    pub async fn fund_from_faucet_with_pow(&self, address: &str) -> Result<()> {
        log::info!("GolemBase: Funding address {address} from PoW faucet");

        // External PoW service is used to acquire access token to the faucet.
        let pow_url = Url::parse("https://cap.gobas.me/05381a2cef5e/api/")?;

        // First get the challenge from the faucet
        let response: ChallengeResponse = self
            .post::<(), ChallengeResponse>(&pow_url.join("challenge")?, ())
            .await?;

        let token = response.token.clone();
        let solutions = self.compute_solutions(response).await?;

        let redeem_response: RedeemResponse = self
            .post::<_, RedeemResponse>(
                &pow_url.join("redeem")?,
                serde_json::json!({
                    "token": token,
                    "solutions": solutions
                }),
            )
            .await?;

        if !redeem_response.success {
            return Err(anyhow::anyhow!(
                "PoW server claims, that our challenge solutions are invalid (address {address})."
            ));
        }

        // Request funds from faucet using the token
        let faucet_request_url = self.config.get_faucet_url().join("api/faucet")?;
        let request = serde_json::json!({
            "address": address,
            "captchaToken": redeem_response.token
        });

        log::info!("GolemBase fund: Requesting funds from faucet for address {address}");

        let address = address.parse()?;
        let mut last_balance = self.client.get_balance(address).await?;

        let response: FaucetResponse = self
            .post::<_, FaucetResponse>(&faucet_request_url, request)
            .await?;

        log::info!(
            "GolemBase fund: Received tx hash: {}, waiting for it to be mined...",
            response.tx_hash
        );

        // Wait for transaction to be mined.
        // Note: Transaction hash references L2 bridge deposit, that's why we need a new client.
        let client = GolemBaseClient::new(self.config.get_l2_rpc_url().clone())?;
        client
            .wait_for_transaction(response.tx_hash.parse::<Hash>()?)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to wait for transaction: {}", e))?;

        // Transaction was mined on L2, but now we need to wait until funds will be available on L3.
        // There is no simple way to check what L3 transaction corresponds to the L2 one.
        // Instead we will poll balance until it increases and assume that the increase is a result
        // of funding.
        // If it's not than it isn't the problem, because the funds are anyway available.
        loop {
            let current_balance = self.client.get_balance(address).await?;
            match current_balance.cmp(&last_balance) {
                std::cmp::Ordering::Greater => {
                    log::info!(
                        "GolemBase fund: Detected balance increase, funds received (address {address})."
                    );
                    break;
                }
                std::cmp::Ordering::Less => {
                    // Balance decreased - wallet is being used and funds are being spent.
                    last_balance = current_balance.clone();
                }
                std::cmp::Ordering::Equal => {}
            }

            tokio::time::sleep(tokio::time::Duration::from_millis(400)).await;
        }

        log::info!("GolemBase fund: Successfully funded address {address}");
        Ok(())
    }

    /// Generic function to handle request/response flow with PoW challenge
    async fn post<Req, Resp>(&self, url: &Url, request: Req) -> Result<Resp>
    where
        Req: Serialize,
        Resp: for<'de> Deserialize<'de>,
    {
        let response = reqwest::Client::new()
            .post(url.to_string())
            .json(&request)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to make request: {}", e))?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Request failed with status: {}, body: {}",
                response.status(),
                response.text().await?
            ));
        }

        response
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to parse response: {}", e))
    }
}
