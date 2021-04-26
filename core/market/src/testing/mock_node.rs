use actix_http::{body::Body, Request};
use actix_service::Service as ActixService;
use actix_web::{dev::ServiceResponse, test, App};
use anyhow::{anyhow, bail, Context, Result};
use regex::Regex;
use std::collections::HashMap;
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
use crate::config::{Config, DiscoveryConfig};
use crate::db::dao::ProposalDao;
use crate::db::model::{Demand, Offer, Proposal, ProposalId, SubscriptionId};
use crate::identity::IdentityApi;
use crate::matcher::error::{DemandError, QueryOfferError};
use crate::matcher::EventsListeners;
use crate::negotiation::error::*;
use crate::protocol::callback::*;
use crate::protocol::discovery::{builder::DiscoveryBuilder, error::*, message::*, Discovery};
use crate::protocol::negotiation::messages::*;
use crate::testing::mock_identity::MockIdentity;
use crate::testing::mock_node::default::*;

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

fn testname_from_backtrace(bn: &str) -> String {
    log::info!("Test name to regex match: {}", &bn);
    // Extract test name
    let captures = Regex::new(r"(.*)::(.*)::\{\{.*")
        .unwrap()
        .captures(&bn)
        .unwrap();
    let filename = captures.get(1).unwrap().as_str().to_string();
    let testname = captures.get(2).unwrap().as_str().to_string();

    format!("{}.{}", filename, testname)
}

impl MarketsNetwork {
    /// Remember that test_name should be unique between all tests.
    /// It will be used to create directories and GSB binding points,
    /// to avoid potential name clashes.
    pub async fn new(test_name: Option<&str>) -> Self {
        let _ = env_logger::builder().try_init();
        // level 1 is this function.
        // level 2 is <core::future::from_generator::GenFuture<T> as
        // core::future::future::Future>::poll::XXX> (async)
        // We want to know the caller.
        let mut bn = crate::testing::backtrace_util::generate_backtraced_name(Some(3));
        // Special case for mac&windows. Tests are run in adifferent way on those systems and we
        // have to dive one less level down the stack to find the caller (test_* module).
        if !bn.starts_with("test_") {
            bn = crate::testing::backtrace_util::generate_backtraced_name(Some(2));
        }
        bn = testname_from_backtrace(&bn);

        let test_name = test_name.unwrap_or(&bn).to_string();
        log::info!("Intializing MarketsNetwork. tn={}", test_name);

        MockNet::default().bind_gsb();

        MarketsNetwork {
            nodes: vec![],
            test_dir: prepare_test_dir(&test_name).unwrap(),
            test_name,
            config: Arc::new(create_market_config_for_test()),
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
    ) -> MarketsNetwork {
        let public_gsb_prefix = node_kind.bind_gsb(&self.test_name, name).await.unwrap();

        let node = MockNode {
            name: name.to_string(),
            mock_identity: identity_api,
            kind: node_kind,
        };

        let node_id = node.mock_identity.get_default_id().clone().identity;
        log::info!("Creating mock node {}: [{}].", name, &node_id);
        BCastService::default().register(&node_id, &self.test_name);
        MockNet::default().register_node(&node_id, &public_gsb_prefix);

        self.nodes.push(node);
        self
    }

    pub fn break_networking_for(&self, node_name: &str) -> Result<()> {
        for (_, id) in self.list_ids(node_name) {
            MockNet::default().unregister_node(&id.identity)?
        }
        Ok(())
    }

    pub fn enable_networking_for(&self, node_name: &str) -> Result<()> {
        for (_, id) in self.list_ids(node_name) {
            let (public_gsb_prefix, _) = gsb_prefixes(&self.test_name, node_name);
            MockNet::default().register_node(&id.identity, &public_gsb_prefix);
        }
        Ok(())
    }

    pub async fn add_market_instance(self, name: &str) -> Self {
        let db = self.create_database(name);
        let identity_api = MockIdentity::new(name);
        let market = Arc::new(
            MarketService::new(
                &db,
                identity_api.clone() as Arc<dyn IdentityApi>,
                self.config.clone(),
            )
            .unwrap(),
        );
        self.add_node(name, identity_api, MockNodeKind::Market(market))
            .await
    }

    pub async fn add_matcher_instance(self, name: &str) -> Self {
        let db = self.init_database(name);

        let store = SubscriptionStore::new(db.clone(), self.config.clone());
        let identity_api = MockIdentity::new(name);

        let (matcher, listeners) =
            Matcher::new(store, identity_api.clone(), self.config.clone()).unwrap();
        self.add_node(
            name,
            identity_api,
            MockNodeKind::Matcher { matcher, listeners },
        )
        .await
    }

    pub async fn add_discovery_instance(self, name: &str, builder: DiscoveryBuilder) -> Self {
        let identity_api = MockIdentity::new(name);
        let discovery = builder
            .add_data(identity_api.clone() as Arc<dyn IdentityApi>)
            .with_config(self.config.discovery.clone())
            .build();
        self.add_node(name, identity_api, MockNodeKind::Discovery(discovery))
            .await
    }

    pub fn discovery_builder(&self) -> DiscoveryBuilder {
        DiscoveryBuilder::default()
            .with_config(self.config.discovery.clone())
            .add_handler(empty_on_offers_retrieved)
            .add_handler(empty_on_offers_bcast)
            .add_handler(empty_on_offer_unsubscribed_bcast)
            .add_handler(empty_on_retrieve_offers)
    }

    pub async fn add_provider_negotiation_api(
        self,
        name: &str,
        prov_initial_proposal_received: impl CallbackHandler<InitialProposalReceived>,
        prov_proposal_received: impl CallbackHandler<ProposalReceived>,
        prov_proposal_rejected: impl CallbackHandler<ProposalRejected>,
        prov_agreement_received: impl CallbackHandler<AgreementReceived>,
        prov_agreement_cancelled: impl CallbackHandler<AgreementCancelled>,
        prov_agreement_terminated: impl CallbackHandler<AgreementTerminated>,
        prov_agreement_committed: impl CallbackHandler<AgreementCommitted>,
    ) -> Self {
        self.add_negotiation_api(
            name,
            prov_initial_proposal_received,
            prov_proposal_received,
            prov_proposal_rejected,
            prov_agreement_received,
            prov_agreement_cancelled,
            prov_agreement_terminated,
            prov_agreement_committed,
            default::empty_on_proposal_received,
            default::empty_on_proposal_rejected,
            default::empty_on_agreement_approved,
            default::empty_on_agreement_rejected,
            default::empty_on_agreement_terminated,
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
        req_agreement_terminated: impl CallbackHandler<AgreementTerminated>,
    ) -> Self {
        self.add_negotiation_api(
            name,
            default::empty_on_initial_proposal,
            default::empty_on_proposal_received,
            default::empty_on_proposal_rejected,
            default::empty_on_agreement_received,
            default::empty_on_agreement_cancelled,
            default::empty_on_agreement_terminated,
            default::empty_on_agreement_committed,
            req_proposal_received,
            req_proposal_rejected,
            req_agreement_approved,
            req_agreement_rejected,
            req_agreement_terminated,
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
        prov_agreement_terminated: impl CallbackHandler<AgreementTerminated>,
        prov_agreement_committed: impl CallbackHandler<AgreementCommitted>,
        req_proposal_received: impl CallbackHandler<ProposalReceived>,
        req_proposal_rejected: impl CallbackHandler<ProposalRejected>,
        req_agreement_approved: impl CallbackHandler<AgreementApproved>,
        req_agreement_rejected: impl CallbackHandler<AgreementRejected>,
        req_agreement_terminated: impl CallbackHandler<AgreementTerminated>,
    ) -> Self {
        let provider = provider::NegotiationApi::new(
            prov_initial_proposal_received,
            prov_proposal_received,
            prov_proposal_rejected,
            prov_agreement_received,
            prov_agreement_cancelled,
            prov_agreement_terminated,
            prov_agreement_committed,
        );

        let requestor = requestor::NegotiationApi::new(
            req_proposal_received,
            req_proposal_rejected,
            req_agreement_approved,
            req_agreement_rejected,
            req_agreement_terminated,
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
            .get_default_id()
            .clone()
    }

    pub fn create_identity(&self, node_name: &str, id_name: &str) -> Identity {
        let mock_identity = self
            .nodes
            .iter()
            .find(|node| &node.name == node_name)
            .map(|node| node.mock_identity.clone())
            .unwrap();
        let id = mock_identity.new_identity(id_name);

        let node_id = id.identity.clone();
        let (public_gsb_prefix, _) = gsb_prefixes(&self.test_name, node_name);

        MockNet::default().register_node(&node_id, &public_gsb_prefix);
        return id;
    }

    pub fn list_ids(&self, node_name: &str) -> HashMap<String, Identity> {
        self.nodes
            .iter()
            .find(|node| &node.name == node_name)
            .map(|node| node.mock_identity.list_ids())
            .unwrap()
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

    fn create_database(&self, name: &str) -> DbExecutor {
        let db_path = self.instance_dir(name);
        let db = DbExecutor::from_data_dir(&db_path, "yagna")
            .map_err(|e| anyhow!("Failed to create db [{:?}]. Error: {}", db_path, e))
            .unwrap();
        db
    }

    pub fn init_database(&self, name: &str) -> DbExecutor {
        let db = self.create_database(name);
        db.apply_migration(crate::db::migrations::run_with_output)
            .unwrap();
        db
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
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("test-workdir")
}

fn escape_path(path: &str) -> String {
    // Windows can't handle colons
    path.replace("::", "_").to_string()
}

pub fn prepare_test_dir(dir_name: &str) -> Result<PathBuf> {
    let test_dir: PathBuf = test_data_dir().join(escape_path(dir_name).as_str());

    log::info!(
        "[MockNode] Preparing test directory: {}",
        test_dir.display()
    );
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
    async fn get_proposal(&self, id: &ProposalId) -> Result<Proposal, GetProposalError>;
    async fn get_proposal_from_db(
        &self,
        proposal_id: &ProposalId,
    ) -> Result<Proposal, anyhow::Error>;
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

    async fn get_proposal(&self, id: &ProposalId) -> Result<Proposal, GetProposalError> {
        self.provider_engine.common.get_proposal(None, id).await
    }

    async fn get_proposal_from_db(
        &self,
        proposal_id: &ProposalId,
    ) -> Result<Proposal, anyhow::Error> {
        let db = self.db.clone();
        Ok(
            match db.as_dao::<ProposalDao>().get_proposal(proposal_id).await? {
                Some(proposal) => proposal,
                None => bail!("Proposal [{}] not found", proposal_id),
            },
        )
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
        ApproveAgreementError, CommitAgreementError, CounterProposalError, GsbAgreementError,
        ProposeAgreementError, RejectProposalError, TerminateAgreementError,
    };

    pub async fn empty_on_offers_retrieved(
        _caller: String,
        _msg: OffersRetrieved,
    ) -> Result<Vec<SubscriptionId>, ()> {
        Ok(vec![])
    }

    pub async fn empty_on_offers_bcast(
        _caller: String,
        _msg: OffersBcast,
    ) -> Result<Vec<SubscriptionId>, ()> {
        Ok(vec![])
    }

    pub async fn empty_on_retrieve_offers(
        _caller: String,
        _msg: RetrieveOffers,
    ) -> Result<Vec<Offer>, DiscoveryRemoteError> {
        Ok(vec![])
    }

    pub async fn empty_on_offer_unsubscribed_bcast(
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
    ) -> Result<(), RejectProposalError> {
        Ok(())
    }

    pub async fn empty_on_agreement_received(
        _caller: String,
        _msg: AgreementReceived,
    ) -> Result<(), ProposeAgreementError> {
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
    ) -> Result<(), GsbAgreementError> {
        Ok(())
    }

    pub async fn empty_on_agreement_cancelled(
        _caller: String,
        _msg: AgreementCancelled,
    ) -> Result<(), GsbAgreementError> {
        Ok(())
    }

    pub async fn empty_on_agreement_committed(
        _caller: String,
        _msg: AgreementCommitted,
    ) -> Result<(), CommitAgreementError> {
        Ok(())
    }

    pub async fn empty_on_agreement_terminated(
        _caller: String,
        _msg: AgreementTerminated,
    ) -> Result<(), TerminateAgreementError> {
        Ok(())
    }
}

pub fn create_market_config_for_test() -> Config {
    // Discovery config to be used only in tests.
    let discovery = DiscoveryConfig {
        max_bcasted_offers: 100,
        max_bcasted_unsubscribes: 100,
        mean_cyclic_bcast_interval: Duration::from_millis(200),
        mean_cyclic_unsubscribes_interval: Duration::from_millis(200),
        offer_broadcast_delay: Duration::from_millis(200),
        unsub_broadcast_delay: Duration::from_millis(200),
    };

    Config {
        discovery,
        subscription: Default::default(),
        events: Default::default(),
    }
}

/// Assure that all given nodes have the same knowledge about given Subscriptions (Offers).
/// Wait if needed at most 2,5s ( = 10 x 250ms).
pub async fn assert_offers_broadcasted<'a, S>(mkts: &[&MarketService], subscriptions: S)
where
    S: IntoIterator<Item = &'a SubscriptionId>,
    <S as IntoIterator>::IntoIter: Clone,
{
    let subscriptions = subscriptions.into_iter();
    let mut all_broadcasted = false;
    'retry: for _i in 0..10 {
        for subscription in subscriptions.clone() {
            for mkt in mkts {
                if mkt.get_offer(&subscription).await.is_err() {
                    // Every 150ms we should get at least one broadcast from each Node.
                    // After a few tries all nodes should have the same knowledge about Offers.
                    tokio::time::delay_for(Duration::from_millis(250)).await;
                    continue 'retry;
                }
            }
        }
        all_broadcasted = true;
        break;
    }
    assert!(
        all_broadcasted,
        "At least one of the offers was not propagated to all nodes"
    );
}

/// Assure that all given nodes have the same knowledge about given Subscriptions (Offers).
/// Wait if needed at most 1,5s ( = 10 x 150ms).
pub async fn assert_unsunbscribes_broadcasted<'a, S>(mkts: &[&MarketService], subscriptions: S)
where
    S: IntoIterator<Item = &'a SubscriptionId>,
    <S as IntoIterator>::IntoIter: Clone,
{
    let subscriptions = subscriptions.into_iter();
    let mut all_broadcasted = false;
    'retry: for _i in 0..10 {
        for subscription in subscriptions.clone() {
            for mkt in mkts {
                let expect_error = QueryOfferError::Unsubscribed(subscription.clone()).to_string();
                match mkt.get_offer(&subscription).await {
                    Err(e) => assert_eq!(e.to_string(), expect_error),
                    Ok(_) => {
                        // Every 150ms we should get at least one broadcast from each Node.
                        // After a few tries all nodes should have the same knowledge about Offers.
                        tokio::time::delay_for(Duration::from_millis(150)).await;
                        continue 'retry;
                    }
                }
            }
        }
        all_broadcasted = true;
        break;
    }
    assert!(
        all_broadcasted,
        "At least one of the offer unsubscribes was not propagated to all nodes"
    );
}
