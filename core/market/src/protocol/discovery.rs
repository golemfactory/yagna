use anyhow::Result;
use bigdecimal::BigDecimal;
use chrono::{DateTime, TimeZone, Utc};
use golem_base_sdk::account::TransactionSigner;
use std::collections::HashSet;
use std::convert::TryFrom;
use std::sync::Arc;
use tokio::time;
use ya_core_model::identity::event::IdentityEvent;
use ya_service_bus::typed as bus;
use ya_service_bus::typed::ServiceBinder;
use ya_service_bus::RpcEndpoint;

use golem_base_sdk::client::GolemBaseClient;
use golem_base_sdk::entity::Create;
use golem_base_sdk::rpc::SearchResult;
use golem_base_sdk::{Address, B256};
use ya_client::model::market::Offer as ClientOffer;
use ya_client::model::NodeId;

use super::callback::HandlerSlot;
use crate::config::DiscoveryConfig;
use crate::db::model::{Offer as ModelOffer, SubscriptionId};
use crate::identity::{IdentityApi, YagnaIdSigner};
use crate::protocol::discovery::error::*;
use crate::protocol::discovery::message::*;

const GOLEM_BASE_CALLER: &str = "GolemBase";
const BUS_ID: &str = "market-discovery";

// TODO: Get this value from node configuration
const BLOCK_TIME_SECONDS: i64 = 2;

pub mod builder;
pub mod error;
pub mod message;

/// Responsible for communication with Golem Base during discovery phase.
#[derive(Clone)]
pub struct Discovery {
    inner: Arc<DiscoveryImpl>,
}

pub(super) struct OfferHandlers {
    filter_out_known_ids: HandlerSlot<OffersBcast>,
    receive_remote_offers: HandlerSlot<OffersRetrieved>,
    offer_unsubscribe_handler: HandlerSlot<UnsubscribedOffersBcast>,
}

pub struct DiscoveryImpl {
    identity: Arc<dyn IdentityApi>,
    golem_base: GolemBaseClient,
    offer_handlers: OfferHandlers,
    config: DiscoveryConfig,
}

impl Discovery {
    /// Broadcasts Offers to Golem Base
    pub async fn bcast_offer(&self, offer: &ModelOffer) -> Result<(), DiscoveryError> {
        let client = &self.inner.golem_base;
        let address = Address::from(&offer.node_id.into_array());

        // Serialize the offer to JSON
        let payload = serde_json::to_vec(&offer.into_client_offer()?).map_err(|e| {
            DiscoveryError::InternalError(format!("Failed to serialize offer: {}", e))
        })?;

        // Calculate TTL in blocks based on expiration time
        let now = Utc::now();
        let expiration = Utc.from_utc_datetime(&offer.expiration_ts);
        let ttl_seconds = (expiration - now).num_seconds();
        let ttl_blocks = (ttl_seconds / BLOCK_TIME_SECONDS) as u64;

        // Create entry with marketplace type and ID annotations
        let entry = Create::new(payload, ttl_blocks)
            .annotate_string("golem_marketplace_type", "Offer")
            .annotate_string("golem_marketplace_id", &offer.id.to_string());

        // Create entry on GolemBase
        let entry_id = client.create_entry(address, entry).await.map_err(|e| {
            DiscoveryError::GolemBaseError(format!("Failed to create offer: {}", e))
        })?;

        log::info!("Created Offer entry in GolemBase with ID: {}", entry_id);

        Ok(())
    }

    /// Queries GolemBase for all offers with marketplace type "Offer"
    pub async fn query_offers(&self) -> Result<Vec<ModelOffer>, DiscoveryError> {
        let client = &self.inner.golem_base;

        // Query for entities with golem_marketplace_type = "Offer"
        let query = r#"golem_marketplace_type = "Offer""#;
        let results = client.query_entities(query).await.map_err(|e| {
            DiscoveryError::GolemBaseError(format!("Failed to query offers: {}", e))
        })?;

        Self::parse_offers(results)
    }

    /// Retrieves Offers from Golem Base
    pub async fn get_remote_offers(
        &self,
        _target_node_id: String,
        offer_ids: Vec<SubscriptionId>,
    ) -> Result<Vec<ModelOffer>, DiscoveryError> {
        let results = self
            .query_subscriptions(&offer_ids)
            .await
            .map_err(|e| DiscoveryError::GolemBaseError(e.to_string()))?;
        Self::parse_offers(results)
    }

    /// Broadcasts unsubscribes to Golem Base
    pub async fn bcast_unsubscribes(
        &self,
        offer_ids: Vec<SubscriptionId>,
    ) -> Result<(), DiscoveryError> {
        let client = &self.inner.golem_base;
        let address = self
            .get_owner_address()
            .map_err(|e| DiscoveryError::InternalError(e.to_string()))?;

        let entries: Vec<B256> = self
            .query_subscriptions(&offer_ids)
            .await
            .map_err(|e| DiscoveryError::GolemBaseError(e.to_string()))?
            .into_iter()
            .map(|result| result.key.clone())
            .collect();

        if entries.is_empty() {
            log::debug!("No entries found in GolemBase for the given offer IDs");
            return Ok(());
        }

        // Remove the entries
        let num_entries = entries.len();
        client.remove_entries(address, entries).await.map_err(|e| {
            DiscoveryError::GolemBaseError(format!("Failed to remove entries: {}", e))
        })?;

        log::info!("Successfully removed {num_entries} entries from GolemBase");
        Ok(())
    }

    /// Gets the GolemBase account address from config
    fn get_owner_address(&self) -> anyhow::Result<Address> {
        self.inner
            .config
            .account
            .map(|account| Address::from(&account.into_array()))
            .ok_or_else(|| anyhow::anyhow!("No account configured for GolemBase"))
    }

    /// Queries GolemBase for entries matching the given subscription IDs
    async fn query_subscriptions(
        &self,
        offer_ids: &[SubscriptionId],
    ) -> anyhow::Result<Vec<SearchResult>> {
        if offer_ids.is_empty() {
            return Ok(Vec::new());
        }

        // Build query with OR conditions for all offer IDs
        let id_conditions: Vec<String> = offer_ids
            .iter()
            .map(|id| format!(r#"golem_marketplace_id = "{}""#, id))
            .collect();
        let query = format!(
            r#"golem_marketplace_type = "Offer" && ({})"#,
            id_conditions.join(" || ")
        );

        // Query for the entries
        self.inner
            .golem_base
            .query_entities(&query)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to query entries: {}", e))
    }

    /// Converts search results to ModelOffer objects
    fn parse_offers(results: Vec<SearchResult>) -> Result<Vec<ModelOffer>, DiscoveryError> {
        let mut offers = Vec::new();
        for result in results {
            match Self::parse_offer(result) {
                Ok(offer) => offers.push(offer),
                Err(e) => log::trace!("Failed to parse offer: {}", e),
            }
        }
        Ok(offers)
    }

    /// Parses a single SearchResult into a ModelOffer
    fn parse_offer(result: SearchResult) -> anyhow::Result<ModelOffer> {
        let value = result.value_as_string()?;
        let offer: ClientOffer = serde_json::from_str(&value)
            .map_err(|e| anyhow::anyhow!("Failed to deserialize Offer json: {}", e))?;

        ModelOffer::try_from(offer)
    }

    async fn initialize_account(&self) -> Result<()> {
        let client = self.inner.golem_base.clone();

        // Get accounts from GolemBase
        client
            .account_sync()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to sync accounts with GolemBase: {e}"))?;

        // List all NodeIds and initialize YagnaIdSigners
        self.register_yagna_signers().await?;

        // Check if we have a wallet address in config
        let account = match self.inner.config.account {
            Some(wallet) => {
                // Convert NodeId to Address
                let address = Address::from(&wallet.into_array());

                // Try to load the account
                client
                    .account_load(address, &self.inner.config.password)
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to load account {}: {e}", wallet))?;
                address
            }
            None => {
                // Generate new account
                let new_account = client
                    .account_generate("")
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to generate new account: {e}"))?;
                log::info!("Generated new account: {}", new_account);
                new_account
            }
        };

        // Check balance and fund if needed
        let balance = client
            .get_balance(account)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get balance for account {}: {e}", account))?;

        if balance < BigDecimal::from(10) {
            log::info!("Account {account} has insufficient balance ({balance} ETH), funding...");
            client
                .fund(account, BigDecimal::from(10))
                .await
                .map_err(|e| anyhow::anyhow!("Failed to fund account {}: {e}", account))?;
        }

        Ok(())
    }

    /// Function doesn't bind any GSB handlers.
    /// It's only used to sync with GolemBase node and initialize Discovery struct state.
    pub async fn bind_gsb(
        &self,
        _public_prefix: &str,
        local_prefix: &str,
    ) -> Result<(), DiscoveryInitError> {
        let client = self.inner.golem_base.clone();

        // Sync with GolemBase node
        client.sync_node().await.map_err(|e| {
            DiscoveryInitError::GolemBaseInitFailed(format!(
                "Failed to sync with GolemBase node: {e}"
            ))
        })?;

        self.initialize_account()
            .await
            .map_err(|e| DiscoveryInitError::GolemBaseInitFailed(e.to_string()))?;

        self.bind_offers_listener().await?;
        self.bind_identity_handlers(local_prefix).await?;
        Ok(())
    }

    async fn subscribe_to_events(&self, endpoint: &str) -> Result<(), DiscoveryInitError> {
        log::debug!("Subscribing to identity events on endpoint: {}", endpoint);
        Ok(bus::service(ya_core_model::identity::BUS_ID)
            .send(ya_core_model::identity::Subscribe {
                endpoint: endpoint.to_string(),
            })
            .await
            .map(|_| ())
            .map_err(|e| {
                DiscoveryInitError::BindingGsbFailed(endpoint.to_string(), e.to_string())
            })?)
    }

    /// Registers a single YagnaIdSigner with GolemBase
    async fn register_signer(&self, node_id: NodeId) -> anyhow::Result<()> {
        let signer = YagnaIdSigner::new(self.inner.identity.clone(), node_id);
        let address = signer.address();

        self.inner.golem_base.account_register(signer).await?;

        let balance = self.inner.golem_base.get_balance(address).await?;
        log::info!("GolemBase client registered account {address} with balance: {balance}");
        Ok(())
    }

    /// Lists all NodeIds from IdentityApi and initializes YagnaIdSigners for all of them, storing them in DiscoveryImpl.
    pub async fn register_yagna_signers(&self) -> anyhow::Result<()> {
        let node_ids = self.inner.identity.list_active_ids().await?;

        for node_id in node_ids {
            if let Err(e) = self.register_signer(node_id).await {
                log::error!("Failed to register signer for {}: {}", node_id, e);
            }
        }
        Ok(())
    }

    async fn bind_identity_handlers(&self, local_prefix: &str) -> Result<(), DiscoveryInitError> {
        let discovery = self.clone();
        let endpoint = format!("{}/{BUS_ID}/handlers", local_prefix);

        // Subscribe to identity events, which will be received on the endpoint.
        self.subscribe_to_events(&endpoint).await?;

        // Bind the handlers for received events.
        ServiceBinder::new(&endpoint, &(), discovery.clone()).bind_with_processor(
            move |_, myself, _caller: String, event: IdentityEvent| {
                let myself = myself;
                async move {
                    match event {
                        IdentityEvent::AccountLocked { identity } => {
                            log::debug!(
                                "Account locked for {identity} - no new offers will be published"
                            );
                        }
                        IdentityEvent::AccountUnlocked { identity } => {
                            log::debug!("Account unlocked - registering new signer for {identity}");
                            if let Err(e) = myself.register_signer(identity).await {
                                log::error!("Failed to register new signer for {identity}: {e}");
                            }
                        }
                    }
                    Ok(())
                }
            },
        );

        Ok(())
    }

    pub async fn bind_offers_listener(&self) -> Result<(), DiscoveryInitError> {
        let discovery = self.clone();
        // TODO: Add separate config value for offers query interval instead of reusing broadcast interval
        let interval = discovery.inner.config.mean_cyclic_bcast_interval;

        tokio::spawn(async move {
            let mut interval = time::interval(interval);
            loop {
                interval.tick().await;

                if let Err(e) = async {
                    // Query all offers from GolemBase
                    let offers = discovery.query_offers().await?;

                    // Filter out known offers
                    let ids = offers.iter().map(|offer| offer.id.clone()).collect();
                    let unknown_offers = discovery
                        .inner
                        .offer_handlers
                        .filter_out_known_ids
                        .call(
                            GOLEM_BASE_CALLER.to_string(),
                            OffersBcast { offer_ids: ids },
                        )
                        .await
                        .unwrap_or_default();

                    // Add unknown Offers to local storage
                    if !unknown_offers.is_empty() {
                        let unknown_offer_ids: HashSet<_> = unknown_offers.iter().collect();
                        let filtered_offers = offers
                            .into_iter()
                            .filter(|offer| unknown_offer_ids.contains(&offer.id))
                            .collect();

                        discovery
                            .inner
                            .offer_handlers
                            .receive_remote_offers
                            .call(
                                GOLEM_BASE_CALLER.to_string(),
                                OffersRetrieved {
                                    offers: filtered_offers,
                                },
                            )
                            .await
                            .ok();
                    }
                    Ok::<(), DiscoveryError>(())
                }
                .await
                {
                    log::error!("Error in offers listener: {}", e);
                }
            }
        });

        Ok(())
    }

    pub(crate) async fn get_last_bcast_ts(&self) -> DateTime<Utc> {
        Utc::now()
    }
}
