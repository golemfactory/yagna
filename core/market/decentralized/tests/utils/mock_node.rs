use anyhow::{anyhow, Context, Result};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use ya_client::model::market::RequestorEvent;
use ya_market_decentralized::protocol::{
    CallbackHandler, Discovery, DiscoveryBuilder, OfferReceived, OfferUnsubscribed, RetrieveOffers,
};
use ya_market_decentralized::testing::mock_offer::generate_identity;
use ya_market_decentralized::testing::negotiation::messages::{
    AgreementApproved, AgreementCancelled, AgreementReceived, AgreementRejected,
    InitialProposalReceived, ProposalReceived, ProposalRejected,
};
use ya_market_decentralized::testing::negotiation::{provider, requestor};
use ya_market_decentralized::testing::{DemandError, OfferError, QueryEventsError};
use ya_market_decentralized::{Demand, MarketService, Offer, SubscriptionId};
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;

use super::{bcast::BCast, mock_net::MockNet};

#[cfg(feature = "bcast-singleton")]
use super::bcast::singleton::BCastService;
#[cfg(not(feature = "bcast-singleton"))]
use super::bcast::BCastService;
use crate::utils::mock_net::gsb_prefixes;

/// Instantiates market test nodes inside one process.
pub struct MarketsNetwork {
    // net: MockNet,
    markets: Vec<MarketNode>,
    discoveries: Vec<DiscoveryNode>,
    negotiations: Vec<NegotiationNode>,
    test_dir: PathBuf,
    test_name: String,
}

/// Store all object associated with single market
/// for example: Database
pub struct MarketNode {
    pub market: Arc<MarketService>,
    pub name: String,
    /// For now only mock default Identity.
    pub id: Identity,
    /// Direct access to underlying database.
    pub db: DbExecutor,
}

/// Stores mock discovery node, that doesn't include full
/// Market implementation, but only Discovery interface.
/// Necessary to emulate wrong nodes behavior.
pub struct DiscoveryNode {
    discovery: Discovery,
    name: String,
    /// For now only mock default Identity.
    id: Identity,
}

/// Stores mock negotiation interfaces, that doesn't include full
/// Market implementation.
/// Necessary to emulate wrong nodes behavior.
pub struct NegotiationNode {
    provider: provider::NegotiationApi,
    requestor: requestor::NegotiationApi,
    name: String,
    /// For now only mock default Identity.
    id: Identity,
}

impl MarketsNetwork {
    /// Remember that test_name should be unique between all tests.
    /// It will be used to create directories and GSB binding points,
    /// to avoid potential name clashes.
    pub async fn new<Str: AsRef<str>>(test_name: Str) -> Self {
        let test_dir = prepare_test_dir(&test_name).unwrap();

        MockNet::default().bind_gsb();
        // let net = MockNet::new().unwrap();

        MarketsNetwork {
            // net,
            markets: vec![],
            discoveries: vec![],
            negotiations: vec![],
            test_dir,
            test_name: test_name.as_ref().to_string(),
        }
    }

    pub async fn add_market_instance<Str: AsRef<str>>(mut self, name: Str) -> Result<Self> {
        let db = self.init_database(name.as_ref())?;
        let market = Arc::new(MarketService::new(&db)?);

        let (public_gsb_prefix, local_gsb_prefix) = gsb_prefixes(&self.test_name, name.as_ref());
        market
            .bind_gsb(&public_gsb_prefix, &local_gsb_prefix)
            .await?;

        let market_node = MarketNode {
            name: name.as_ref().to_string(),
            id: generate_identity(name.as_ref()),
            market,
            db,
        };
        BCastService::default().register(&market_node.id.identity, &self.test_name);
        // self.net
        //     .register_node(&market_node.id.identity, &public_gsb_prefix)
        //     .await?;
        MockNet::default().register_node(&market_node.id.identity, &public_gsb_prefix);
        self.markets.push(market_node);
        Ok(self)
    }

    pub async fn add_discovery_instance<Str: AsRef<str>>(
        mut self,
        name: Str,
        offer_received: impl CallbackHandler<OfferReceived>,
        offer_unsubscribed: impl CallbackHandler<OfferUnsubscribed>,
        retrieve_offers: impl CallbackHandler<RetrieveOffers>,
    ) -> Result<Self> {
        let (public_gsb_prefix, local_gsb_prefix) = gsb_prefixes(&self.test_name, name.as_ref());

        let discovery = DiscoveryBuilder::default()
            .add_handler(offer_received)
            .add_handler(offer_unsubscribed)
            .add_handler(retrieve_offers)
            .build();
        discovery
            .bind_gsb(&public_gsb_prefix, &local_gsb_prefix)
            .await?;

        let discovery_node = DiscoveryNode {
            name: name.as_ref().to_string(),
            id: generate_identity(name.as_ref()),
            discovery,
        };

        BCastService::default().register(&discovery_node.id.identity, &self.test_name);
        // self.net
        //     .register_node(&discovery_node.id.identity, &public_gsb_prefix)
        //     .await?;
        MockNet::default().register_node(&discovery_node.id.identity, &public_gsb_prefix);
        self.discoveries.push(discovery_node);
        Ok(self)
    }

    pub async fn add_provider_negotiation_api<Str: AsRef<str>>(
        self,
        name: Str,
        prov_initial_proposal_received: impl CallbackHandler<InitialProposalReceived>,
        prov_proposal_received: impl CallbackHandler<ProposalReceived>,
        prov_proposal_rejected: impl CallbackHandler<ProposalRejected>,
        prov_agreement_received: impl CallbackHandler<AgreementReceived>,
        prov_agreement_cancelled: impl CallbackHandler<AgreementCancelled>,
    ) -> Result<Self> {
        self.add_negotiation_api(
            name.as_ref(),
            prov_initial_proposal_received,
            prov_proposal_received,
            prov_proposal_rejected,
            prov_agreement_received,
            prov_agreement_cancelled,
            default::empty_on_proposal_received,
            default::empty_on_proposal_rejected,
            default::empty_on_agreement_approved,
            default::empty_on_agreement_rejected,
        )
        .await
    }

    pub async fn add_requestor_negotiation_api<Str: AsRef<str>>(
        self,
        name: Str,
        req_proposal_received: impl CallbackHandler<ProposalReceived>,
        req_proposal_rejected: impl CallbackHandler<ProposalRejected>,
        req_agreement_approved: impl CallbackHandler<AgreementApproved>,
        req_agreement_rejected: impl CallbackHandler<AgreementRejected>,
    ) -> Result<Self> {
        self.add_negotiation_api(
            name.as_ref(),
            default::empty_on_initial_proposal,
            default::empty_on_proposal_received,
            default::empty_on_proposal_rejected,
            default::empty_on_agreement_received,
            default::empty_on_agreement_cancelled,
            req_proposal_received,
            req_proposal_rejected,
            req_agreement_approved,
            req_agreement_rejected,
        )
        .await
    }

    pub async fn add_negotiation_api<Str: AsRef<str>>(
        mut self,
        name: Str,
        prov_initial_proposal_received: impl CallbackHandler<InitialProposalReceived>,
        prov_proposal_received: impl CallbackHandler<ProposalReceived>,
        prov_proposal_rejected: impl CallbackHandler<ProposalRejected>,
        prov_agreement_received: impl CallbackHandler<AgreementReceived>,
        prov_agreement_cancelled: impl CallbackHandler<AgreementCancelled>,
        req_proposal_received: impl CallbackHandler<ProposalReceived>,
        req_proposal_rejected: impl CallbackHandler<ProposalRejected>,
        req_agreement_approved: impl CallbackHandler<AgreementApproved>,
        req_agreement_rejected: impl CallbackHandler<AgreementRejected>,
    ) -> Result<Self> {
        let (public_gsb_prefix, local_gsb_prefix) = gsb_prefixes(&self.test_name, name.as_ref());

        let provider_api = provider::NegotiationApi::new(
            prov_initial_proposal_received,
            prov_proposal_received,
            prov_proposal_rejected,
            prov_agreement_received,
            prov_agreement_cancelled,
        );
        provider_api
            .bind_gsb(&public_gsb_prefix, &local_gsb_prefix)
            .await?;

        let requestor_api = requestor::NegotiationApi::new(
            req_proposal_received,
            req_proposal_rejected,
            req_agreement_approved,
            req_agreement_rejected,
        );
        requestor_api
            .bind_gsb(&public_gsb_prefix, &local_gsb_prefix)
            .await?;

        let node = NegotiationNode {
            name: name.as_ref().to_string(),
            id: generate_identity(name.as_ref()),
            provider: provider_api,
            requestor: requestor_api,
        };

        // self.net
        //     .register_node(&node.id.identity, &public_gsb_prefix)
        //     .await?;
        MockNet::default().register_node(&node.id.identity, &public_gsb_prefix);
        self.negotiations.push(node);
        Ok(self)
    }

    pub fn get_market(&self, name: &str) -> Arc<MarketService> {
        self.markets
            .iter()
            .find(|node| node.name == name)
            .map(|node| node.market.clone())
            .unwrap()
    }

    pub fn get_discovery(&self, name: &str) -> Discovery {
        self.discoveries
            .iter()
            .find(|node| node.name == name)
            .map(|node| node.discovery.clone())
            .unwrap()
    }

    pub fn get_provider_negotiation_api(&self, name: &str) -> provider::NegotiationApi {
        self.negotiations
            .iter()
            .find(|node| node.name == name)
            .map(|node| node.provider.clone())
            .unwrap()
    }

    pub fn get_requestor_negotiation_api(&self, name: &str) -> requestor::NegotiationApi {
        self.negotiations
            .iter()
            .find(|node| node.name == name)
            .map(|node| node.requestor.clone())
            .unwrap()
    }

    pub fn get_default_id(&self, node_name: &str) -> Identity {
        self.markets
            .iter()
            .map(|node| (&node.name, &node.id))
            .chain(self.discoveries.iter().map(|node| (&node.name, &node.id)))
            .chain(self.negotiations.iter().map(|node| (&node.name, &node.id)))
            .find(|&(name, _id)| name == &node_name)
            .map(|(_name, id)| id.clone())
            .unwrap()
    }

    pub fn get_database(&self, name: &str) -> DbExecutor {
        self.markets
            .iter()
            .find(|node| node.name == name)
            .map(|node| node.db.clone())
            .unwrap()
    }

    fn init_database(&self, name: &str) -> Result<DbExecutor> {
        let db_path = self.instance_dir(name);
        let db = DbExecutor::from_data_dir(&db_path, "yagna")
            .map_err(|e| anyhow!("Failed to create db [{:?}]. Error: {}", db_path, e))?;
        Ok(db)
    }

    fn instance_dir(&self, name: &str) -> PathBuf {
        let dir = self.test_dir.join(name);
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}

fn test_data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/test-workdir")
}

pub fn prepare_test_dir<Str: AsRef<str>>(dir_name: Str) -> Result<PathBuf> {
    let test_dir: PathBuf = test_data_dir().join(dir_name.as_ref());

    if test_dir.exists() {
        fs::remove_dir_all(&test_dir)
            .with_context(|| format!("Removing test directory: {}", test_dir.display()))?;
    }
    fs::create_dir_all(&test_dir)
        .with_context(|| format!("Creating test directory: {}", test_dir.display()))?;
    Ok(test_dir)
}

/// Facilitates waiting for broadcast propagation.
pub async fn wait_for_bcast(
    grace_millis: u64,
    market: &MarketService,
    subscription_id: &SubscriptionId,
    stop_is_ok: bool,
) {
    let steps = 20;
    let wait_step = Duration::from_millis(grace_millis / steps);
    let store = market.matcher.store.clone();
    for _ in 0..steps {
        tokio::time::delay_for(wait_step).await;
        if store.get_offer(&subscription_id).await.is_ok() == stop_is_ok {
            break;
        }
    }
}

#[async_trait::async_trait]
pub trait MarketServiceExt {
    async fn get_offer(&self, id: &SubscriptionId) -> Result<Offer, OfferError>;
    async fn get_demand(&self, id: &SubscriptionId) -> Result<Demand, DemandError>;
    async fn query_events(
        &self,
        subscription_id: &SubscriptionId,
        timeout: f32,
        max_events: Option<i32>,
    ) -> Result<Vec<RequestorEvent>, QueryEventsError>;
}

#[async_trait::async_trait]
impl MarketServiceExt for MarketService {
    async fn get_offer(&self, id: &SubscriptionId) -> Result<Offer, OfferError> {
        self.matcher.store.get_offer(id).await
    }

    async fn get_demand(&self, id: &SubscriptionId) -> Result<Demand, DemandError> {
        self.matcher.store.get_demand(id).await
    }

    async fn query_events(
        &self,
        subscription_id: &SubscriptionId,
        timeout: f32,
        max_events: Option<i32>,
    ) -> Result<Vec<RequestorEvent>, QueryEventsError> {
        self.requestor_engine
            .query_events(subscription_id, timeout, max_events)
            .await
    }
}

pub mod default {
    use ya_market_decentralized::protocol::negotiation::messages::AgreementCancelled;
    use ya_market_decentralized::protocol::{
        DiscoveryRemoteError, OfferReceived, OfferUnsubscribed, Propagate, Reason, RetrieveOffers,
    };
    use ya_market_decentralized::testing::negotiation::errors::{AgreementError, ProposalError};
    use ya_market_decentralized::testing::negotiation::messages::{
        AgreementApproved, AgreementReceived, AgreementRejected, InitialProposalReceived,
        ProposalReceived, ProposalRejected,
    };
    use ya_market_decentralized::testing::Offer;

    pub async fn empty_on_offer_received(
        _caller: String,
        _msg: OfferReceived,
    ) -> Result<Propagate, ()> {
        Ok(Propagate::No(Reason::AlreadyExists))
    }

    pub async fn empty_on_offer_unsubscribed(
        _caller: String,
        _msg: OfferUnsubscribed,
    ) -> Result<Propagate, ()> {
        Ok(Propagate::No(Reason::Unsubscribed))
    }

    pub async fn empty_on_retrieve_offers(
        _caller: String,
        _msg: RetrieveOffers,
    ) -> Result<Vec<Offer>, DiscoveryRemoteError> {
        Ok(vec![])
    }

    pub async fn empty_on_initial_proposal(
        _caller: String,
        _msg: InitialProposalReceived,
    ) -> Result<(), ProposalError> {
        Ok(())
    }

    pub async fn empty_on_proposal_received(
        _caller: String,
        _msg: ProposalReceived,
    ) -> Result<(), ProposalError> {
        Ok(())
    }

    pub async fn empty_on_proposal_rejected(
        _caller: String,
        _msg: ProposalRejected,
    ) -> Result<(), ProposalError> {
        Ok(())
    }

    pub async fn empty_on_agreement_received(
        _caller: String,
        _msg: AgreementReceived,
    ) -> Result<(), AgreementError> {
        Ok(())
    }

    pub async fn empty_on_agreement_approved(
        _caller: String,
        _msg: AgreementApproved,
    ) -> Result<(), AgreementError> {
        Ok(())
    }

    pub async fn empty_on_agreement_rejected(
        _caller: String,
        _msg: AgreementRejected,
    ) -> Result<(), AgreementError> {
        Ok(())
    }

    pub async fn empty_on_agreement_cancelled(
        _caller: String,
        _msg: AgreementCancelled,
    ) -> Result<(), AgreementError> {
        Ok(())
    }
}
