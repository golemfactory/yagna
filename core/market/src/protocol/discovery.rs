use anyhow::Result;
use chrono::{DateTime, Utc};
use serde_json::json;
use std::collections::HashSet;
use std::env;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::task::JoinHandle;

use ya_client::model::NodeId;
use ya_core_model::bus::GsbBindPoints;

use super::callback::HandlerSlot;
use crate::config::DiscoveryConfig;
use crate::db::model::Offer as ModelOffer;
use crate::identity::IdentityApi;
use crate::protocol::discovery::error::*;
use crate::protocol::discovery::message::*;

const ARKIV_CALLER: &str = "Arkiv";

pub mod builder;
pub mod error;
pub mod faucet;
pub mod message;
pub mod offer;
pub mod pow;

/// Responsible for communication with Arkiv during discovery phase.
#[derive(Clone)]
pub struct Discovery {
    inner: Arc<DiscoveryImpl>,
}

pub(super) struct OfferHandlers {
    filter_out_known_ids: HandlerSlot<OffersBcast>,
    receive_remote_offers: HandlerSlot<OffersRetrieved>,
    _offer_unsubscribe_handler: HandlerSlot<UnsubscribedOffersBcast>,
}

pub struct DiscoveryImpl {
    identity: Arc<dyn IdentityApi>,
    //arkiv: ArkivClient,
    offer_handlers: OfferHandlers,
    config: DiscoveryConfig,
    _identities: Mutex<HashSet<NodeId>>,
    websocket_task: Mutex<Option<JoinHandle<()>>>,
}

impl Discovery {
    async fn offers_events_loop(&self, _starting_block: u64) -> anyhow::Result<()> {
        let default_identity = self
            .inner
            .identity
            .default_identity()
            .await
            .expect("Failed to get default identity");

        loop {
            // Wait for either Ctrl+C or timeout to continue loop
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    log::info!("Received Ctrl+C, shutting down offers events loop");
                    break;
                }
                _ = tokio::time::sleep(Duration::from_secs(10)) => {}
            }

            let matcher = env::var("YAGNA_MARKET_MATCHER_URL");
            let take_at_once = env::var("YAGNA_MARKET_MATCHER_TAKE_AT_ONCE")
                .unwrap_or_else(|_e| "20".to_string())
                .parse::<u32>()
                .unwrap_or_else(|_e| {
                    log::error!(
                        "Invalid YAGNA_MARKET_MATCHER_TAKE_AT_ONCE value, defaulting to 20"
                    );
                    20
                });
            if let Ok(matcher) = matcher {
                let client = reqwest::Client::new();

                let body = json! {{
                    "demandId": default_identity.to_string(),
                    "takeAtOnce": take_at_once
                }}
                .to_string();
                let res = client
                    .post(&(matcher + "/requestor/demand/take-from-queue"))
                    .header("Content-Type", "application/json")
                    .body(body)
                    .send()
                    .await;
                match res {
                    Ok(response) => {
                        if response.status().is_success() {
                            let data = response.text().await.unwrap_or_default();

                            let offers = serde_json::from_str::<Vec<ModelOffer>>(&data);
                            match offers {
                                Ok(offers) => {
                                    log::info!(
                                        "Registering incoming offers from providers: {:?}",
                                        offers.iter().map(|f| f.node_id)
                                    );
                                    self.register_incoming_offers(offers).await?;
                                }
                                Err(e) => {
                                    log::error!(
                                        "Failed to parse offer from matcher response: {}",
                                        e
                                    );
                                }
                            }
                        } else if response.status() == reqwest::StatusCode::NOT_FOUND {
                            let response_text = response.text().await.unwrap_or_default();
                            if response_text.is_empty() {
                                log::error!(
                                    "Failed to take offer due to other error (no body): {}",
                                    default_identity
                                );
                            } else {
                                log::info!(
                                    "No offers available in matcher queue for demand {}",
                                    response_text
                                );
                            }
                        } else {
                            log::error!(
                                "Failed to take offer, other status: {}",
                                response.status()
                            );
                        }
                    }
                    Err(e) => {
                        log::error!("Failed to take offer due to other error: {}", e);
                    }
                }
            } else {
                log::warn!("YAGNA_MARKET_MATCHER_URL not set, skipping demand notification");
            }
        }
        Ok(())
    }

    /// Spawns a task that listens for WebSocket events from Arkiv
    pub async fn bind_offers_listener(&self) -> Result<(), DiscoveryInitError> {
        let discovery = self.clone();
        //let client = self.inner.arkiv.clone();

        // Get starting block number - use last remembered block if available, otherwise current block
        let starting_block = 0;

        // First, load all existing offers to setup initial state.
        // Later we will listen only for state changes.
        /*let offers = self.query_offers().await.map_err(|e| {
            DiscoveryInitError::GolemBaseInitFailed(format!("Failed to query offers: {}", e))
        })?;*/

        self.register_incoming_offers(vec![]).await.map_err(|e| {
            DiscoveryInitError::GolemBaseInitFailed(format!("Failed to register offers: {}", e))
        })?;

        let handle = tokio::task::spawn_local(async move {
            discovery
                .offers_events_loop(starting_block)
                .await
                .inspect_err(|e| log::error!("Error in GolemBase events listener: {}", e))
                .ok();
        });

        // Store the task handle
        {
            let mut task_handle = self.inner.websocket_task.lock().unwrap();
            *task_handle = Some(handle);
        }

        Ok(())
    }

    /// Starts listening for offers (queries existing offers and starts websocket listener)
    /// This is called when the first demand is created
    pub async fn start_listening(&self) -> Result<(), DiscoveryError> {
        // If running as indexer, listener is already started in bind_gsb
        if self.inner.config.run_as_indexer {
            log::debug!("Running as indexer - listener already started, skipping");
            return Ok(());
        }

        // Check if already listening
        {
            let task_handle = self.inner.websocket_task.lock().unwrap();
            if task_handle.is_some() {
                log::debug!("Already listening for offers, skipping start");
                return Ok(());
            }
        }

        log::info!("Starting to listen for offers (first demand created)");
        self.bind_offers_listener()
            .await
            .map_err(|e| DiscoveryError::GolemBaseError(format!("Binding offers listener: {e}")))?;
        Ok(())
    }

    /// Stops listening for offers (stops websocket listener but keeps offers in database)
    pub async fn stop_listening(&self) -> Result<(), DiscoveryError> {
        // If running as indexer, keep listening even when no demands
        if self.inner.config.run_as_indexer {
            log::info!("Running as indexer - keeping listener active, skipping stop");
            return Ok(());
        }

        log::info!("Stopping listening for offers (last demand removed)");

        // Get and clear the task handle
        {
            let mut task_handle = self.inner.websocket_task.lock().unwrap();
            if let Some(handle) = task_handle.take() {
                handle.abort();
            }
        };

        Ok(())
    }

    /// Function doesn't bind any GSB handlers.
    /// It's only used to sync with GolemBase node and initialize Discovery struct state.
    pub async fn bind_gsb(&self, _gsb: GsbBindPoints) -> Result<(), DiscoveryInitError> {
        log::info!("Arkiv Configuration:");
        log::info!("  Network: {:?}", self.inner.config.get_network_type());
        log::info!("  RPC URL: {}", self.inner.config.get_rpc_url());
        log::info!("  WebSocket URL: {}", self.inner.config.get_ws_url());
        log::info!("  Faucet URL: {}", self.inner.config.get_faucet_url());
        log::info!("  L2 RPC URL: {}", self.inner.config.get_l2_rpc_url());
        log::info!(
            "  Fund Preallocated: {}",
            self.inner.config.fund_preallocated()
        );

        /*

        self.sync_client().await.map_err(|e| {
            log::error!("Failed to sync with GolemBase node: {}", e);
            DiscoveryInitError::GolemBaseInitFailed(e.to_string())
        })?;

        self.initialize_account().await.map_err(|e| {
            log::error!("Failed to initialize accounts on GolemBase: {}", e);
            DiscoveryInitError::GolemBaseInitFailed(e.to_string())
        })?;


         */

        // Remove all existing offers from previous runs. Offers are volatile, so it doesn't make
        // any sense to keep them after restart and they pollute GolemBase. Offers should expire
        // after some period of time, so this step is not essential, but in case we restart after crash
        // the old Offers would remain.

        // Start Arkiv listener that loads offers and listens for updates
        // Only if MARKET_RUN_AS_INDEXER is enabled (otherwise we wait for first demand)
        if self.inner.config.run_as_indexer {
            log::info!("MARKET_RUN_AS_INDEXER enabled - starting offer listener immediately");
            self.bind_offers_listener().await?;
        } else {
            log::info!("MARKET_RUN_AS_INDEXER disabled - will start listening when first demand is created");
        }

        //self.bind_identity_handlers(gsb.local_addr()).await?;
        //self.bind_fund_handler(gsb.local_addr()).await?;
        Ok(())
    }

    pub(crate) async fn get_last_bcast_ts(&self) -> DateTime<Utc> {
        Utc::now()
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
            .call(ARKIV_CALLER.to_string(), OffersBcast { offer_ids: ids })
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
                    ARKIV_CALLER.to_string(),
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
