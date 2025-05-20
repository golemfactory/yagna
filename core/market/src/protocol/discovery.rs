use anyhow::Result;
use bigdecimal::BigDecimal;
use chrono::{DateTime, TimeZone, Utc};
use futures::StreamExt;
use offer::GolemBaseOffer;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use ya_client::model::NodeId;
use ya_core_model::identity::event::IdentityEvent;
use ya_core_model::identity::Error;
use ya_core_model::market::local;
use ya_core_model::market::{
    FundGolemBase, FundGolemBaseResponse, GetGolemBaseBalance, GetGolemBaseBalanceResponse,
    RpcMessageError,
};
use ya_service_bus::typed as bus;
use ya_service_bus::RpcEndpoint;

use golem_base_sdk::client::GolemBaseClient;
use golem_base_sdk::entity::Create;
use golem_base_sdk::events::Event;
use golem_base_sdk::rpc::SearchResult;
use golem_base_sdk::signers::TransactionSigner;
use golem_base_sdk::{Address, Hash};

use super::callback::HandlerSlot;
use crate::config::DiscoveryConfig;
use crate::db::model::{Offer as ModelOffer, SubscriptionId};
use crate::identity::{IdentityApi, IdentityError, YagnaIdSigner};
use crate::protocol::discovery::error::*;
use crate::protocol::discovery::message::*;

const GOLEM_BASE_CALLER: &str = "GolemBase";

// TODO: Get this value from node configuration
const BLOCK_TIME_SECONDS: i64 = 2;

pub mod builder;
pub mod error;
pub mod message;
pub mod offer;
/// Responsible for communication with Golem Base during discovery phase.
#[derive(Clone)]
pub struct Discovery {
    inner: Arc<DiscoveryImpl>,
}

pub(super) struct OfferHandlers {
    filter_out_known_ids: HandlerSlot<OffersBcast>,
    receive_remote_offers: HandlerSlot<OffersRetrieved>,
    #[allow(dead_code)]
    offer_unsubscribe_handler: HandlerSlot<UnsubscribedOffersBcast>,
}

pub struct DiscoveryImpl {
    identity: Arc<dyn IdentityApi>,
    golem_base: GolemBaseClient,
    offer_handlers: OfferHandlers,
    #[allow(dead_code)]
    config: DiscoveryConfig,
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

        log::info!(
            "Serialized offer payload: {}",
            String::from_utf8_lossy(&payload)
        );

        // Calculate TTL in blocks based on expiration time
        let now = Utc::now();
        let expiration = Utc.from_utc_datetime(&offer.expiration.naive_utc());
        let ttl_seconds = (expiration - now).num_seconds();
        let ttl_blocks = (ttl_seconds / BLOCK_TIME_SECONDS) as u64;

        // Create entry with marketplace type and ID annotations
        let entry =
            Create::new(payload, ttl_blocks).annotate_string("golem_marketplace_type", "Offer");
        // .annotate_string("golem_marketplace_id", offer.id.to_string());

        // Create entry on GolemBase
        let entry_id = client.create_entry(address, entry).await.map_err(|e| {
            DiscoveryError::GolemBaseError(format!("Failed to create offer: {}", e))
        })?;

        log::info!("Created Offer entry in GolemBase with ID: {}", entry_id);

        Ok(offer.into_model_offer(entry_id).map_err(|e| {
            DiscoveryError::GolemBaseError(format!("Failed to convert offer to ModelOffer: {}", e))
        })?)
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
        let offer: GolemBaseOffer = serde_json::from_str(&string_utf)
            .map_err(|e| anyhow::anyhow!("Failed to deserialize Offer json: {}", e))?;
        offer.into_model_offer(key)
    }

    /// List all accounts and initialize YagnaIdSigners on GolemBase, so they can be used for
    /// signing storage transactions.
    async fn initialize_account(&self) -> Result<()> {
        let node_ids = self.inner.identity.list_active_ids().await?;
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
            .events_client()
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

    /// Handles incoming Golem Base events
    async fn handle_golem_base_event(&self, event: Event) -> anyhow::Result<()> {
        let client = self.inner.golem_base.clone();

        match event {
            Event::EntityCreated { entity_id, .. } => {
                log::trace!("Entity created in Golem Base: {}", entity_id);

                let offer = client.cat(entity_id).await?;
                let offer = Self::parse_offer(entity_id, &offer)?;

                self.register_incoming_offers(vec![offer]).await?;
            }
            Event::EntityRemoved { entity_id, .. } => {
                log::trace!("Entity removed from Golem Base: {}", entity_id);

                let id = client.get_entity_metadata(entity_id).await?;
                let id = id
                    .string_annotations
                    .iter()
                    .find(|a| a.key == "golem_marketplace_id")
                    .ok_or_else(|| anyhow::anyhow!("No golem_marketplace_id found in metadata"))?;
                let id = id.value.parse::<SubscriptionId>()?;

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
    pub async fn bind_gsb(
        &self,
        _public_prefix: &str,
        local_prefix: &str,
    ) -> Result<(), DiscoveryInitError> {
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

        self.bind_identity_handlers(local_prefix).await?;
        self.bind_fund_handler(local_prefix).await?;
        Ok(())
    }

    async fn subscribe_to_events(&self, endpoint: &str) -> Result<(), DiscoveryInitError> {
        log::debug!("Subscribing to identity events on endpoint: {}", endpoint);
        bus::service(ya_core_model::identity::BUS_ID)
            .send(ya_core_model::identity::Subscribe {
                endpoint: endpoint.to_string(),
            })
            .await
            .map(|_| ())
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
            }
            IdentityEvent::AccountUnlocked { identity } => {
                log::debug!("Account unlocked - registering new signer for {identity}");
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

        Ok(())
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

    async fn fund(&self, msg: FundGolemBase) -> Result<FundGolemBaseResponse, RpcMessageError> {
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
        client
            .fund(address, BigDecimal::from(10))
            .await
            .map_err(|e| RpcMessageError::Market(format!("Failed to fund wallet: {}", e)))?;

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
