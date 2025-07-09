use anyhow::Result;
use chrono::{DateTime, TimeZone, Utc};
use futures::StreamExt;
use offer::GolemBaseOffer;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use ya_service_bus::timeout::IntoTimeoutFuture;

use ya_client::model::NodeId;
use ya_core_model::bus::GsbBindPoints;
use ya_core_model::identity::event::IdentityEvent;
use ya_core_model::identity::Error;
use ya_core_model::market::{local, GetGolemBaseOffer, GetGolemBaseOfferResponse};
use ya_core_model::market::{
    FundGolemBase, FundGolemBaseResponse, GetGolemBaseBalance, GetGolemBaseBalanceResponse,
    RpcMessageError,
};
use ya_service_bus::typed as bus;

use golem_base_sdk::client::GolemBaseClient;
use golem_base_sdk::entity::Create;
use golem_base_sdk::events::Event;
use golem_base_sdk::rpc::{EntityMetaData, SearchResult};
use golem_base_sdk::signers::TransactionSigner;
use golem_base_sdk::{Address, Hash};

use super::callback::HandlerSlot;
use crate::config::DiscoveryConfig;
use crate::db::model::{Offer as ModelOffer, SubscriptionId};
use crate::identity::{IdentityApi, IdentityError, YagnaIdSigner};
use crate::protocol::discovery::error::*;
use crate::protocol::discovery::faucet::FaucetClient;
use crate::protocol::discovery::message::*;

const GOLEM_BASE_CALLER: &str = "GolemBase";

// TODO: Get this value from node configuration
const BLOCK_TIME_SECONDS: i64 = 2;

pub mod builder;
pub mod error;
pub mod faucet;
pub mod message;
pub mod offer;
pub mod pow;

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
    identities: Mutex<HashSet<NodeId>>,
}

impl Discovery {
    /// Broadcasts Offers to Golem Base
    pub async fn bcast_offer(&self, offer: GolemBaseOffer) -> Result<ModelOffer, DiscoveryError> {
        // Validate account to return more menaingfull error messages than create_entry would.
        self.validate_account(offer.provider_id).await?;

        let client = &self.inner.golem_base;
        let address = Address::from(&offer.provider_id.into_array());

        // Serialize the offer to JSON
        let payload = serde_json::to_vec(&offer).map_err(|e| {
            DiscoveryError::InternalError(format!("Failed to serialize offer: {}", e))
        })?;

        // Calculate TTL in blocks based on expiration time
        let now = Utc::now();
        let expiration = Utc.from_utc_datetime(&offer.expiration.naive_utc());
        let ttl_seconds = (expiration - now).num_seconds();
        let ttl_blocks = (ttl_seconds / BLOCK_TIME_SECONDS) as u64;

        // Create entry with marketplace type and ID annotations
        let entry =
            Create::new(payload, ttl_blocks).annotate_string("golem_marketplace_type", "Offer");

        // Create entry on GolemBase
        let timeout = self.inner.config.offer_publish_timeout;
        let entry_id = client
            .create_entry(address, entry)
            .timeout(Some(timeout))
            .await
            .map_err(|_| {
                DiscoveryError::GolemBaseError(format!(
                    "Timeout ({}) creating offer.",
                    humantime::Duration::from(timeout)
                ))
            })?
            .map_err(|e| {
                DiscoveryError::GolemBaseError(format!("Failed to create offer: {}", e))
            })?;

        log::info!("Created Offer entry in GolemBase with ID: {}", entry_id);

        let model_offer = offer.into_model_offer(entry_id).map_err(|e| {
            DiscoveryError::GolemBaseError(format!("Failed to convert offer to ModelOffer: {}", e))
        })?;

        Ok(model_offer)
    }

    /// Checks if an offer belongs to us based on metadata and entity_id
    fn is_own_offer(&self, metadata: &EntityMetaData) -> bool {
        let identities = self.inner.identities.lock().unwrap();
        let owner_bytes = NodeId::from(metadata.owner.as_slice());
        identities.contains(&owner_bytes)
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

    /// Broadcasts unsubscribe to Golem Base
    pub async fn bcast_unsubscribe(&self, offer_id: SubscriptionId) -> Result<(), DiscoveryError> {
        let client = &self.inner.golem_base;

        // Get metadata to find owner
        let key = Hash::from(offer_id.to_bytes());
        let metadata = client.get_entity_metadata(key).await.map_err(|e| {
            DiscoveryError::GolemBaseError(format!(
                "Failed to get entry metadata for offer {offer_id}: {e}"
            ))
        })?;

        // Remove the entry
        client
            .remove_entries(metadata.owner, vec![key])
            .await
            .map_err(|e| {
                DiscoveryError::GolemBaseError(format!(
                    "Failed to remove entry for owner {}: {e}",
                    metadata.owner
                ))
            })?;

        log::info!(
            "Successfully removed entry from GolemBase for offer {}",
            offer_id
        );
        Ok(())
    }

    /// Queries GolemBase for entries matching the given subscription IDs
    async fn query_subscriptions(
        &self,
        offer_ids: &[SubscriptionId],
    ) -> anyhow::Result<Vec<SearchResult>> {
        if offer_ids.is_empty() {
            return Ok(Vec::new());
        }

        let offer_ids: Vec<Hash> = offer_ids
            .iter()
            .map(|id| Hash::from(id.to_bytes()))
            .collect();

        let mut results = Vec::new();
        for key in offer_ids {
            if let Ok(content) = self.inner.golem_base.get_storage_value(key).await {
                results.push(SearchResult {
                    key,
                    value: content,
                });
            }
        }

        Ok(results)
    }

    /// Converts search results to ModelOffer objects
    fn parse_offers(results: Vec<SearchResult>) -> Result<Vec<ModelOffer>, DiscoveryError> {
        let mut offers = Vec::new();
        for result in results {
            match Self::offer_from_search(result) {
                Ok(offer) => offers.push(offer),
                Err(e) => log::trace!("Failed to parse offer: {}", e),
            }
        }
        Ok(offers)
    }

    /// Parses a single SearchResult into a ModelOffer
    fn offer_from_search(result: SearchResult) -> anyhow::Result<ModelOffer> {
        let value = result.value_as_string()?;
        Self::parse_offer(result.key, &value)
    }

    fn parse_offer(key: Hash, string_utf: &str) -> anyhow::Result<ModelOffer> {
        let offer: GolemBaseOffer = serde_json::from_str(string_utf)
            .map_err(|e| anyhow::anyhow!("Failed to deserialize Offer json: {}", e))?;
        offer.into_model_offer(key)
    }

    /// List all accounts and initialize YagnaIdSigners on GolemBase, so they can be used for
    /// signing storage transactions.
    async fn initialize_account(&self) -> Result<()> {
        let node_ids = self.inner.identity.list_active_ids().await?;
        {
            let mut identities = self.inner.identities.lock().unwrap();
            identities.clear();
            identities.extend(node_ids.iter().cloned());
        }

        for node_id in node_ids {
            if let Err(e) = self.register_signer(node_id).await {
                log::error!("Failed to register signer for {}: {}", node_id, e);
            }
        }
        Ok(())
    }

    async fn offers_events_loop(&self, starting_block: u64) -> anyhow::Result<()> {
        let events = self
            .inner
            .golem_base
            .events_client_with_url(self.inner.config.get_ws_url().clone())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get events client: {}", e))?;

        let mut event_stream = events
            .events_stream_from_block(starting_block)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get events stream: {}", e))?;

        while let Some(event) = event_stream.next().await {
            match event {
                Ok(event) => {
                    // Handle the event based on its type
                    if let Err(e) = self.handle_golem_base_event(event).await {
                        log::error!("Error handling Golem Base event: {}", e);
                    }
                }
                Err(e) => {
                    log::error!("Error receiving Golem Base event: {}", e);
                    // Try to reconnect after a delay, to protect against errors spam
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    continue;
                }
            }
        }
        Ok(())
    }

    /// Spawns a task that listens for WebSocket events from Golem Base
    pub async fn bind_offers_listener(&self) -> Result<(), DiscoveryInitError> {
        let discovery = self.clone();
        let client = self.inner.golem_base.clone();

        // Remove all existing offers from previous runs. Offers are volatile, so it doesn't make
        // any sense to keep them after restart and they polute GolemBase. Offers should expire
        // after some period of time, so this step is not essential, but in case we restart after crash
        // the old Offers would remain.
        log::debug!("Removing all existing offers from previous runs..");
        self.remove_all_node_offers().await;

        // Get current block number to start listening from
        let current_block = client.get_current_block_number().await.map_err(|e| {
            DiscoveryInitError::GolemBaseInitFailed(format!("Failed to get current block: {}", e))
        })?;

        // First, load all existing offers to setup initial state.
        // Later we will listen only for state changes.
        let offers = self.query_offers().await.map_err(|e| {
            DiscoveryInitError::GolemBaseInitFailed(format!("Failed to query offers: {}", e))
        })?;
        self.register_incoming_offers(offers).await.map_err(|e| {
            DiscoveryInitError::GolemBaseInitFailed(format!("Failed to register offers: {}", e))
        })?;

        tokio::spawn(async move {
            discovery
                .offers_events_loop(current_block)
                .await
                .inspect_err(|e| log::error!("Error in GolemBase events listener: {}", e))
                .ok();
        });

        Ok(())
    }

    /// Removes all offers published by any of the node's identities
    async fn remove_all_node_offers(&self) {
        // Get all identities, excluding locked and removed ones. We won't be able to sign
        // removal transaction. @Note We could use default identity as a signer, but it would
        // work temporary until proper permission management on GolemBase is implemented.
        let accounts = match self.inner.identity.list_active_ids().await {
            Ok(accounts) => accounts,
            Err(e) => {
                log::warn!("Removing outdated Offers: failed to list identities: `{e}`. Offers will remain.");
                return;
            }
        };

        for account in accounts {
            if let Err(e) = self.remove_identity_offers(account).await {
                log::warn!("Failed to remove Offers for identity {account}: {e}");
            }
        }
    }

    /// Removes all offers published by a specific identity
    async fn remove_identity_offers(&self, node_id: NodeId) -> anyhow::Result<()> {
        let address = Address::from(&node_id.into_array());

        // Get all entries owned by this address
        let results = self.inner.golem_base.get_entities_of_owner(address).await?;

        // Filter only offer entries
        let mut offer_entries = Vec::new();
        for result in results {
            let metadata = self.inner.golem_base.get_entity_metadata(result).await?;

            // It's important. If we would run on GolemBase chain that is not dedicated for marketplace
            // only, we would remove entries published by other applications.
            if Self::is_golem_offer(&metadata) {
                offer_entries.push(result);
            }
        }

        if !offer_entries.is_empty() {
            let count = offer_entries.len();
            self.inner
                .golem_base
                .remove_entries(address, offer_entries)
                .await?;
            log::info!("Removed {} offers for identity {}", count, node_id);
        }

        Ok(())
    }

    /// Validates if an entity is a Golem offer by checking its marketplace type annotation
    fn is_golem_offer(metadata: &EntityMetaData) -> bool {
        metadata.string_annotations.iter().any(|annotation| {
            annotation.key == "golem_marketplace_type" && annotation.value == "Offer"
        })
    }

    /// Handles incoming Golem Base events
    async fn handle_golem_base_event(&self, event: Event) -> anyhow::Result<()> {
        let client = self.inner.golem_base.clone();

        match event {
            Event::EntityCreated { entity_id, .. } => {
                let metadata = client.get_entity_metadata(entity_id).await?;
                if !Self::is_golem_offer(&metadata) || self.is_own_offer(&metadata) {
                    return Ok(());
                }

                log::trace!("Entity created in Golem Base: {}", entity_id);

                let offer = client.cat(entity_id).await?;
                let offer = Self::parse_offer(entity_id, &offer)?;

                self.register_incoming_offers(vec![offer]).await?;
            }
            Event::EntityRemoved { entity_id, .. } => {
                log::trace!("Entity removed from Golem Base: {}", entity_id);

                let id = SubscriptionId::from_bytes(entity_id.0);
                self.inner
                    .offer_handlers
                    .offer_unsubscribe_handler
                    .call(
                        GOLEM_BASE_CALLER.to_string(),
                        UnsubscribedOffersBcast {
                            offer_ids: vec![id],
                        },
                    )
                    .await
                    .unwrap_or_default();
            }
            // Ignore EntityUpdated events, because market doesn't allow for updating entities.
            _ => {}
        }
        Ok(())
    }

    /// Function doesn't bind any GSB handlers.
    /// It's only used to sync with GolemBase node and initialize Discovery struct state.
    pub async fn bind_gsb(&self, gsb: GsbBindPoints) -> Result<(), DiscoveryInitError> {
        log::info!("Golem Base Configuration:");
        log::info!("  Network: {:?}", self.inner.config.get_network_type());
        log::info!("  RPC URL: {}", self.inner.config.get_rpc_url());
        log::info!("  WebSocket URL: {}", self.inner.config.get_ws_url());
        log::info!("  Faucet URL: {}", self.inner.config.get_faucet_url());
        log::info!("  L2 RPC URL: {}", self.inner.config.get_l2_rpc_url());
        log::info!(
            "  Fund Preallocated: {}",
            self.inner.config.fund_preallocated()
        );

        let client = self.inner.golem_base.clone();

        // Sync with GolemBase node
        client
            .sync_node(Duration::from_secs(10))
            .await
            .map_err(|e| {
                DiscoveryInitError::GolemBaseInitFailed(format!(
                    "Failed to sync with GolemBase node: {e}"
                ))
            })?;

        self.initialize_account()
            .await
            .map_err(|e| DiscoveryInitError::GolemBaseInitFailed(e.to_string()))?;

        // Start Golem Base listener that loads offers and listens for updates
        self.bind_offers_listener().await?;

        self.bind_identity_handlers(gsb.local_addr()).await?;
        self.bind_fund_handler(gsb.local_addr()).await?;
        Ok(())
    }

    async fn subscribe_to_events(&self, endpoint: &str) -> Result<(), DiscoveryInitError> {
        log::debug!("Subscribing to identity events on endpoint: {}", endpoint);
        self.inner
            .identity
            .subscribe_to_events(endpoint)
            .await
            .map_err(|e| DiscoveryInitError::BindingGsbFailed(endpoint.to_string(), e.to_string()))
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

    async fn handle_identity_event(&self, event: IdentityEvent) -> Result<(), Error> {
        match event {
            IdentityEvent::AccountLocked { identity } => {
                log::debug!("Account locked for {identity} - no new offers will be published");
                self.inner.identities.lock().unwrap().remove(&identity);
            }
            IdentityEvent::AccountUnlocked { identity } => {
                log::debug!("Account unlocked - registering new signer for {identity}");
                self.inner.identities.lock().unwrap().insert(identity);
                if let Err(e) = self.register_signer(identity).await {
                    log::error!("Failed to register new signer for {identity}: {e}");
                    return Err(Error::InternalErr(format!(
                        "Failed to register signer: {e}"
                    )));
                }
            }
        }
        Ok(())
    }

    async fn bind_identity_handlers(&self, local_prefix: &str) -> Result<(), DiscoveryInitError> {
        let discovery = self.clone();
        let endpoint = local::build_discovery_endpoint(local_prefix);

        // Subscribe to identity events, which will be received on the endpoint.
        self.subscribe_to_events(&endpoint).await?;

        // Bind the handlers for received events.
        bus::bind(&endpoint, move |event: IdentityEvent| {
            let myself = discovery.clone();
            async move { myself.handle_identity_event(event).await }
        });

        Ok(())
    }

    async fn bind_fund_handler(&self, local_prefix: &str) -> Result<(), DiscoveryInitError> {
        let discovery = self.clone();
        let endpoint = local::build_discovery_endpoint(local_prefix);

        bus::bind(&endpoint, move |msg: FundGolemBase| {
            let myself = discovery.clone();
            async move { myself.fund(msg).await }
        });

        // Bind balance check handler
        let discovery = self.clone();
        bus::bind(&endpoint, move |msg: GetGolemBaseBalance| {
            let myself = discovery.clone();
            async move { myself.get_balance(msg).await }
        });

        // Bind get offer handler
        let discovery = self.clone();
        bus::bind(&endpoint, move |msg: GetGolemBaseOffer| {
            let myself = discovery.clone();
            async move {
                myself
                    .get_offer(msg)
                    .await
                    .map_err(|e| RpcMessageError::Market(e.to_string()))
            }
        });

        Ok(())
    }

    async fn get_offer(&self, msg: GetGolemBaseOffer) -> anyhow::Result<GetGolemBaseOfferResponse> {
        let offer_id = msg
            .offer_id
            .parse::<Hash>()
            .map_err(|e| anyhow::anyhow!("Invalid offer ID format: {}", e))?;

        let client = self.inner.golem_base.clone();
        let block_number = client
            .get_current_block_number()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get current block: {}", e))?;

        let metadata = client
            .get_entity_metadata(offer_id)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get entity metadata: {}", e))?;
        let metadata = serde_json::to_value(&metadata)
            .map_err(|e| anyhow::anyhow!("Failed to serialize metadata: {}", e))?;

        let content = client.cat(offer_id).await?;
        let offer = Self::parse_offer(offer_id, &content)?.into_client_offer()?;

        Ok(GetGolemBaseOfferResponse {
            offer,
            current_block: block_number,
            metadata,
        })
    }

    pub(crate) async fn get_last_bcast_ts(&self) -> DateTime<Utc> {
        Utc::now()
    }

    /// Validates if the account can be used for storing offers
    async fn validate_account(&self, node_id: NodeId) -> Result<(), DiscoveryError> {
        let accounts = self.inner.identity.list().await?;

        let account = accounts
            .iter()
            .find(|acc| acc.node_id == node_id)
            .ok_or_else(|| {
                IdentityError::SigningError(format!("Account {node_id} not found in identities"))
            })?;

        if account.is_locked {
            return Err(IdentityError::SigningError(format!("Account {node_id} is locked")).into());
        }

        if account.deleted {
            return Err(
                IdentityError::SigningError(format!("Account {node_id} is deleted")).into(),
            );
        }

        Ok(())
    }

    pub async fn fund(&self, msg: FundGolemBase) -> Result<FundGolemBaseResponse, RpcMessageError> {
        let wallet = match msg.wallet {
            Some(wallet) => wallet,
            None => self.inner.identity.default_identity().await.map_err(|e| {
                RpcMessageError::Market(format!("Failed to get default identity: {e}"))
            })?,
        };

        self.validate_account(wallet)
            .await
            .map_err(|e| RpcMessageError::Market(e.to_string()))?;

        let client = self.inner.golem_base.clone();
        let address = Address::from(&wallet.into_array());

        let faucet_client = FaucetClient::new(self.inner.config.clone(), client.clone());

        if self.inner.config.fund_preallocated() {
            faucet_client
                .fund_local_account(address)
                .await
                .map_err(|e| RpcMessageError::Market(e.to_string()))?;
        } else {
            faucet_client
                .fund_from_faucet_with_pow(&address.to_string())
                .await
                .map_err(|e| RpcMessageError::Market(e.to_string()))?;
        }

        // Get balance after funding
        let balance = client
            .get_balance(address)
            .await
            .map_err(|e| RpcMessageError::Market(format!("Failed to get balance: {}", e)))?;

        log::info!("GolemBase balance for wallet {}: {}", wallet, balance);
        Ok(FundGolemBaseResponse { wallet, balance })
    }

    async fn get_balance(
        &self,
        msg: GetGolemBaseBalance,
    ) -> Result<GetGolemBaseBalanceResponse, RpcMessageError> {
        let wallet = match msg.wallet {
            Some(wallet) => wallet,
            None => self.inner.identity.default_identity().await.map_err(|e| {
                RpcMessageError::Market(format!("Failed to get default identity: {e}"))
            })?,
        };

        let client = self.inner.golem_base.clone();
        let address = Address::from(&wallet.into_array());

        let balance = client
            .get_balance(address)
            .await
            .map_err(|e| RpcMessageError::Market(format!("Failed to get balance: {}", e)))?;

        Ok(GetGolemBaseBalanceResponse {
            wallet,
            balance,
            token: "tETH".to_string(),
        })
    }

    /// Registers incoming offers by filtering out known ones and adding new ones to local storage
    async fn register_incoming_offers(
        &self,
        offers: Vec<ModelOffer>,
    ) -> Result<(), DiscoveryError> {
        // Filter out known offers
        let ids = offers.iter().map(|offer| offer.id.clone()).collect();
        let unknown_offers = self
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

            self.inner
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
        Ok(())
    }
}
