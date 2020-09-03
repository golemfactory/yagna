use actix_http::{body::Body, Request};
use actix_service::Service as ActixService;
use actix_web::{dev::ServiceResponse, test, App};
use anyhow::{anyhow, Context, Result};
use std::{fs, path::PathBuf, sync::Arc, time::Duration};

use ya_client::model::market::RequestorEvent;
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::{auth::dummy::DummyAuth, Identity};

use crate::MarketService;

#[cfg(feature = "bcast-singleton")]
use super::bcast::singleton::BCastService;
use super::bcast::BCast;
#[cfg(not(feature = "bcast-singleton"))]
use super::bcast::BCastService;
use super::mock_net::{gsb_prefixes, MockNet};
use super::negotiation::{provider, requestor};
use super::{store::SubscriptionStore, Matcher};
use crate::config::Config;
use crate::db::model::{Demand, Offer, SubscriptionId};
use crate::identity::IdentityApi;
use crate::matcher::error::{DemandError, QueryOfferError};
use crate::matcher::EventsListeners;
use crate::negotiation::error::QueryEventsError;
use crate::protocol::callback::*;
use crate::protocol::discovery::{builder::DiscoveryBuilder, error::*, message::*, Discovery};
use crate::protocol::negotiation::messages::*;
use crate::testing::mock_identity::MockIdentity;
use crate::testing::mock_node::default::{
    empty_on_get_offers, empty_on_offer_unsubscribed, empty_on_offers_ids_received,
    empty_on_offers_received,
};

/// Instantiates market test nodes inside one process.
pub struct MarketsNetwork {
    nodes: Vec<MockNode>,
    test_dir: PathBuf,
    test_name: String,
    config: Arc<Config>,
}

pub struct MockNode {
    pub name: String,
    /// For now only mock default Identity.
    pub mock_identity: Arc<MockIdentity>,
    pub kind: MockNodeKind,
}

/// Internal object associated with single Node
pub enum MockNodeKind {
    /// Full Market Service
    Market(Arc<MarketService>),
    /// Just Matcher sub-service and event listener.
    /// Used to check resolver behaviour.
    Matcher {
        matcher: Matcher,
        listeners: EventsListeners,
    },
    /// Stores mock discovery node, that doesn't include full
    /// Market implementation, but only Discovery interface.
    /// Necessary to emulate wrong nodes behavior.
    Discovery(Discovery),
    /// Stores mock negotiation interfaces, that doesn't include full
    /// Market implementation.
    /// Necessary to emulate wrong nodes behavior.
    Negotiation {
        provider: provider::NegotiationApi,
        requestor: requestor::NegotiationApi,
    },
}

impl MockNodeKind {
    pub async fn bind_gsb(&self, test_name: &str, name: &str) -> Result<String> {
        let (public, local) = gsb_prefixes(test_name, name);

        match self {
            MockNodeKind::Market(market) => market.bind_gsb(&public, &local).await?,
            MockNodeKind::Matcher { matcher, .. } => matcher.bind_gsb(&public, &local).await?,
            MockNodeKind::Discovery(discovery) => discovery.bind_gsb(&public, &local).await?,
            MockNodeKind::Negotiation {
                provider,
                requestor,
            } => {
                provider.bind_gsb(&public, &local).await?;
                requestor.bind_gsb(&public, &local).await?;
            }
        }

        Ok(public)
    }
}

impl MarketsNetwork {
    /// Remember that test_name should be unique between all tests.
    /// It will be used to create directories and GSB binding points,
    /// to avoid potential name clashes.
    pub async fn new(test_name: &str) -> Self {
        let _ = env_logger::builder().try_init();
        let test_dir = prepare_test_dir(&test_name).unwrap();

        MockNet::default().bind_gsb();

        // Disable cyclic broadcasts by default.
        let mut config = Config::default();
        config.discovery.num_bcasted_offers = 0;
        config.discovery.num_bcasted_unsubscribes = 0;

        MarketsNetwork {
            nodes: vec![],
            test_dir,
            test_name: test_name.to_string(),
            config: Arc::new(config),
        }
    }

    /// Config will be used to initialize all consecutive Nodes.
    pub fn with_config(mut self, config: Arc<Config>) -> Self {
        self.config = config;
        self
    }

    async fn add_node(
        mut self,
        name: &str,
        identity_api: Arc<MockIdentity>,
        node_kind: MockNodeKind,
    ) -> Result<MarketsNetwork> {
        let public_gsb_prefix = node_kind.bind_gsb(&self.test_name, name).await?;

        let node = MockNode {
            name: name.to_string(),
            mock_identity: identity_api,
            kind: node_kind,
        };

        let node_id = node.mock_identity.default.clone().identity;
        BCastService::default().register(&node_id, &self.test_name);
        MockNet::default().register_node(&node_id, &public_gsb_prefix);

        self.nodes.push(node);
        Ok(self)
    }

    pub fn break_networking_for(&self, node_name: &str) -> Result<()> {
        let id = self.get_default_id(node_name);
        MockNet::default().unregister_node(&id.identity)
    }

    pub fn enable_networking_for(&self, node_name: &str) -> Result<()> {
        let id = self.get_default_id(node_name);
        let (public_gsb_prefix, _) = gsb_prefixes(&self.test_name, node_name);

        MockNet::default().register_node(&id.identity, &public_gsb_prefix);
        Ok(())
    }

    pub async fn add_market_instance(self, name: &str) -> Result<Self> {
        let db = self.init_database(name)?;
        let identity_api = MockIdentity::new(name);
        let market = Arc::new(MarketService::new(
            &db,
            identity_api.clone() as Arc<dyn IdentityApi>,
            self.config.clone(),
        )?);
        self.add_node(name, identity_api, MockNodeKind::Market(market))
            .await
    }

    pub async fn add_matcher_instance(self, name: &str) -> Result<Self> {
        let db = self.init_database(name)?;
        db.apply_migration(crate::db::migrations::run_with_output)?;

        let store = SubscriptionStore::new(db.clone(), self.config.clone());
        let identity_api = MockIdentity::new(name);

        let (matcher, listeners) = Matcher::new(store, identity_api.clone(), self.config.clone())?;
        self.add_node(
            name,
            identity_api,
            MockNodeKind::Matcher { matcher, listeners },
        )
        .await
    }

    pub async fn add_discovery_instance(
        self,
        name: &str,
        builder: DiscoveryBuilder,
    ) -> Result<Self> {
        let identity_api = MockIdentity::new(name);
        let discovery = builder
            .add_data(identity_api.clone() as Arc<dyn IdentityApi>)
            .build();
        self.add_node(name, identity_api, MockNodeKind::Discovery(discovery))
            .await
    }

    pub fn discovery_builder() -> DiscoveryBuilder {
        DiscoveryBuilder::default()
            .add_handler(empty_on_offers_received)
            .add_handler(empty_on_offers_ids_received)
            .add_handler(empty_on_offer_unsubscribed)
            .add_handler(empty_on_get_offers)
    }

    pub async fn add_provider_negotiation_api(
        self,
        name: &str,
        prov_initial_proposal_received: impl CallbackHandler<InitialProposalReceived>,
        prov_proposal_received: impl CallbackHandler<ProposalReceived>,
        prov_proposal_rejected: impl CallbackHandler<ProposalRejected>,
        prov_agreement_received: impl CallbackHandler<AgreementReceived>,
        prov_agreement_cancelled: impl CallbackHandler<AgreementCancelled>,
    ) -> Result<Self> {
        self.add_negotiation_api(
            name,
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

    pub async fn add_requestor_negotiation_api(
        self,
        name: &str,
        req_proposal_received: impl CallbackHandler<ProposalReceived>,
        req_proposal_rejected: impl CallbackHandler<ProposalRejected>,
        req_agreement_approved: impl CallbackHandler<AgreementApproved>,
        req_agreement_rejected: impl CallbackHandler<AgreementRejected>,
    ) -> Result<Self> {
        self.add_negotiation_api(
            name,
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

    pub async fn add_negotiation_api(
        self,
        name: &str,
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
        let provider = provider::NegotiationApi::new(
            prov_initial_proposal_received,
            prov_proposal_received,
            prov_proposal_rejected,
            prov_agreement_received,
            prov_agreement_cancelled,
        );

        let requestor = requestor::NegotiationApi::new(
            req_proposal_received,
            req_proposal_rejected,
            req_agreement_approved,
            req_agreement_rejected,
        );

        let identity_api = MockIdentity::new(name);

        self.add_node(
            name,
            identity_api,
            MockNodeKind::Negotiation {
                provider,
                requestor,
            },
        )
        .await
    }

    pub fn get_market(&self, name: &str) -> Arc<MarketService> {
        self.nodes
            .iter()
            .find(|node| node.name == name)
            .map(|node| match &node.kind {
                MockNodeKind::Market(market) => market.clone(),
                _ => panic!("market expected"),
            })
            .unwrap()
    }

    pub fn get_matcher(&self, name: &str) -> &Matcher {
        self.nodes
            .iter()
            .find(|node| node.name == name)
            .map(|node| match &node.kind {
                MockNodeKind::Matcher { matcher, .. } => matcher,
                _ => panic!("discovery expected"),
            })
            .unwrap()
    }

    pub fn get_event_listeners(&mut self, name: &str) -> &mut EventsListeners {
        self.nodes
            .iter_mut()
            .find(|node| node.name == name)
            .map(|node| match &mut node.kind {
                MockNodeKind::Matcher { listeners, .. } => listeners,
                _ => panic!("discovery expected"),
            })
            .unwrap()
    }

    pub fn get_discovery(&self, name: &str) -> Discovery {
        self.nodes
            .iter()
            .find(|node| node.name == name)
            .map(|node| match &node.kind {
                MockNodeKind::Discovery(discovery) => discovery.clone(),
                _ => panic!("discovery expected"),
            })
            .unwrap()
    }

    pub fn get_provider_negotiation_api(&self, name: &str) -> provider::NegotiationApi {
        self.nodes
            .iter()
            .find(|node| node.name == name)
            .map(|node| match &node.kind {
                MockNodeKind::Negotiation { provider, .. } => provider.clone(),
                _ => panic!("negotiation expected"),
            })
            .unwrap()
    }

    pub fn get_requestor_negotiation_api(&self, name: &str) -> requestor::NegotiationApi {
        self.nodes
            .iter()
            .find(|node| node.name == name)
            .map(|node| match &node.kind {
                MockNodeKind::Negotiation { requestor, .. } => requestor.clone(),
                _ => panic!("negotiation expected"),
            })
            .unwrap()
    }

    pub fn get_default_id(&self, node_name: &str) -> Identity {
        self.nodes
            .iter()
            .find(|node| &node.name == node_name)
            .map(|node| node.mock_identity.clone())
            .unwrap()
            .default
            .clone()
    }

    pub async fn get_rest_app(
        &self,
        node_name: &str,
    ) -> impl ActixService<
        Request = Request,
        Response = ServiceResponse<Body>,
        Error = actix_http::error::Error,
    > {
        let market = self.get_market(node_name);
        let identity = self.get_default_id(node_name);

        test::init_service(
            App::new()
                .wrap(DummyAuth::new(identity))
                .service(MarketService::bind_rest(market)),
        )
        .await
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

    pub fn node_gsb_prefixes(&self, node_name: &str) -> (String, String) {
        gsb_prefixes(&self.test_name, node_name)
    }
}

fn test_data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/test-workdir")
}

pub fn prepare_test_dir(dir_name: &str) -> Result<PathBuf> {
    let test_dir: PathBuf = test_data_dir().join(dir_name);

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

#[macro_export]
macro_rules! assert_err_eq {
    ($expected:expr, $actual:expr $(,)*) => {
        assert_eq!($expected.to_string(), $actual.unwrap_err().to_string())
    };
}

#[async_trait::async_trait]
pub trait MarketServiceExt {
    async fn get_offer(&self, id: &SubscriptionId) -> Result<Offer, QueryOfferError>;
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
    async fn get_offer(&self, id: &SubscriptionId) -> Result<Offer, QueryOfferError> {
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
    use super::*;
    use crate::protocol::negotiation::error::{
        AgreementError, ApproveAgreementError, CounterProposalError, ProposalError,
    };

    pub async fn empty_on_offers_received(
        _caller: String,
        _msg: OffersRetrieved,
    ) -> Result<Vec<SubscriptionId>, ()> {
        Ok(vec![])
    }

    pub async fn empty_on_offers_ids_received(
        _caller: String,
        _msg: OffersBcast,
    ) -> Result<Vec<SubscriptionId>, ()> {
        Ok(vec![])
    }

    pub async fn empty_on_get_offers(
        _caller: String,
        _msg: RetrieveOffers,
    ) -> Result<Vec<Offer>, DiscoveryRemoteError> {
        Ok(vec![])
    }

    pub async fn empty_on_offer_unsubscribed(
        _caller: String,
        _msg: UnsubscribedOffersBcast,
    ) -> Result<Vec<SubscriptionId>, ()> {
        Ok(vec![])
    }

    pub async fn empty_on_initial_proposal(
        _caller: String,
        _msg: InitialProposalReceived,
    ) -> Result<(), CounterProposalError> {
        Ok(())
    }

    pub async fn empty_on_proposal_received(
        _caller: String,
        _msg: ProposalReceived,
    ) -> Result<(), CounterProposalError> {
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
    ) -> Result<(), ApproveAgreementError> {
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
