#![allow(clippy::too_many_arguments)]

use actix_http::body::BoxBody;
use actix_http::Request;
use actix_service::Service as ActixService;
use actix_web::{dev::ServiceResponse, test, App};
use anyhow::{anyhow, Context, Result};
use std::path::Path;
use std::{fs, path::PathBuf, sync::Arc, time::Duration};
use testcontainers::core::ContainerPort;
use testcontainers::ContainerAsync;
use ya_core_model::market::local::{self};
use ya_core_model::market::{self, FundGolemBaseResponse};
use ya_core_model::NodeId;
use ya_service_bus::timeout::IntoTimeoutFuture;
use ya_service_bus::RpcEndpoint;

use ya_market::testing::IdentityGSB;
use ya_market::testing::{
    callback::*, discovery::error::*, negotiation::error::*, AgreementApproved, AgreementCancelled,
    AgreementCommitted, AgreementReceived, AgreementRejected, AgreementTerminated, Config,
    DbMixedExecutor, Discovery, DiscoveryBuilder, DiscoveryConfig, EventsListeners,
    GolemBaseNetwork, IdentityApi, InitialProposalReceived, MarketService, MarketServiceExt,
    Matcher, Offer, ProposalReceived, ProposalRejected, QueryOfferError, ScannerSet,
    SubscriptionId, SubscriptionStore,
};

use ya_core_model::bus::GsbBindPoints;
use ya_market::testing::negotiation::{provider, requestor};
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::{auth::dummy::DummyAuth, Identity};

use ya_framework_basic::mocks::net::{gsb_market_prefixes, gsb_prefixes, IMockBroadcast, IMockNet};

use crate::identity::RealIdentity;
use crate::net::MockNet;

use testcontainers::{
    core::{logs::LogFrame, WaitFor},
    runners::AsyncRunner,
    GenericImage, ImageExt,
};

/// Instantiates market test nodes inside one process.
///
/// @note This is a legacy implementation used in market test suite. New testing tools were
/// created since then (test-utils/test-framework/framework-mocks/src/node.rs) and the goal
/// is to slowly unify both implementations.
pub struct MarketsNetwork {
    net: MockNet,
    nodes: Vec<MockNode>,

    /// GolemBase which will be dropped on the end of the test.
    #[allow(dead_code)]
    golembase: ContainerAsync<GenericImage>,

    test_dir: PathBuf,
    test_name: String,

    config: Arc<Config>,
}

pub struct MockNode {
    pub name: String,
    /// For now only mock default Identity.
    pub identity_api: Arc<dyn IdentityApi>,
    pub identity: RealIdentity,
    pub kind: MockNodeKind,
    pub gsb: GsbBindPoints,
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

impl MockNode {
    pub fn new(
        name: &str,
        test_name: &str,
        identity_api: Arc<dyn IdentityApi>,
        identity: RealIdentity,
        kind: MockNodeKind,
    ) -> Self {
        let gsb = gsb_prefixes(test_name, name);
        Self {
            name: name.to_string(),
            identity_api,
            identity,
            kind,
            gsb,
        }
    }

    pub async fn bind_gsb(&self, test_name: &str, name: &str) -> Result<GsbBindPoints> {
        self.identity.bind_gsb().await?;
        self.kind.bind_gsb(test_name, name).await?;
        Ok(self.gsb.clone())
    }

    pub async fn fund(&self, wallet: NodeId) -> Result<FundGolemBaseResponse> {
        let gsb = market::bus_bindpoints(Some(self.gsb.clone()));
        let response = local::build_discovery_bindpoint(&gsb)
            .local()
            .send(market::FundGolemBase {
                wallet: Some(wallet),
            })
            .await??;
        Ok(response)
    }
}

impl MockNodeKind {
    pub async fn bind_gsb(&self, test_name: &str, name: &str) -> Result<GsbBindPoints> {
        let gsb = gsb_market_prefixes(gsb_prefixes(test_name, name));

        match self {
            MockNodeKind::Market(market) => {
                market.bind_gsb(gsb.clone()).await?;
            }
            MockNodeKind::Matcher { matcher, .. } => {
                matcher.bind_gsb(gsb.clone()).await?;
            }
            MockNodeKind::Discovery(discovery) => {
                discovery.bind_gsb(gsb.clone()).await?;
            }
            MockNodeKind::Negotiation {
                provider,
                requestor,
            } => {
                provider.bind_gsb(gsb.clone()).await?;
                requestor.bind_gsb(gsb.clone()).await?;
            }
        }

        Ok(gsb)
    }
}

impl MarketsNetwork {
    /// Remember that test_name should be unique between all tests.
    /// It will be used to create directories and GSB binding points,
    /// to avoid potential name clashes.
    pub async fn new(testdir: impl AsRef<Path>, net: MockNet) -> Self {
        let gen_test_name = || {
            let nonce = rand::random::<u128>();
            format!("test-{:#32x}", nonce)
        };

        let testdir = testdir.as_ref();
        let test_name = testdir
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(gen_test_name);
        log::info!("Initializing MarketsNetwork. TestName={}", test_name);

        net.bind_gsb();

        let config = create_market_config_for_test();
        let golembase = Self::init_golembase(&config).await.unwrap();

        MarketsNetwork {
            net,
            nodes: vec![],
            test_dir: prepare_test_dir(testdir).unwrap(),
            test_name,
            config: Arc::new(config),
            golembase,
        }
    }

    pub async fn init_golembase(config: &Config) -> Result<ContainerAsync<GenericImage>> {
        let ws_port = config.discovery.get_ws_url().port().unwrap_or(8545);
        let timeout = Duration::from_secs(60);
        let log_consumer =
            |line: &LogFrame| log::info!("[GolemBase]: {}", String::from_utf8_lossy(&line.bytes()));

        let container = GenericImage::new("golemnetwork/golembase-op-geth", "latest")
            .with_wait_for(WaitFor::message_on_stderr("FS scan times"))
            .with_mapped_port(ws_port, ContainerPort::Tcp(ws_port))
            .with_log_consumer(log_consumer)
            .with_cmd([
                "--dev",
                "--http",
                "--http.api",
                "eth,web3,net,debug,golembase",
                "--verbosity",
                "5",
                "--http.addr",
                "0.0.0.0",
                "--http.port",
                &ws_port.to_string(),
                "--http.corsdomain",
                "*",
                "--http.vhosts",
                "*",
                "--ws",
                "--ws.addr",
                "0.0.0.0",
                "--ws.port",
                &ws_port.to_string(),
            ])
            .start()
            .timeout(Some(timeout))
            .await
            .map_err(|e| {
                anyhow!(
                    "Timeout ({}) starting GolemBase instance: {e}",
                    humantime::format_duration(timeout)
                )
            })?
            .map_err(|e| anyhow!("Failed to start GolemBase instance: {}", e))?;

        // Slow down a bit to allow GolemBase to start
        tokio::time::sleep(Duration::from_millis(1000)).await;

        Ok(container)
    }

    /// Config will be used to initialize all consecutive Nodes.
    pub fn with_config(mut self, config: Arc<Config>) -> Self {
        self.config = config;
        self
    }

    async fn add_node(
        mut self,
        name: &str,
        identity_api: Arc<dyn IdentityApi>,
        identity: RealIdentity,
        node_kind: MockNodeKind,
    ) -> MarketsNetwork {
        let node = MockNode::new(name, &self.test_name, identity_api, identity, node_kind);

        let gsb = node.bind_gsb(&self.test_name, name).await.unwrap();
        let node_id = node.identity_api.default_identity().await.unwrap();

        log::info!("Creating mock node '{}': [{}].", name, &node_id);
        self.net.register_for_broadcasts(&node_id, &self.test_name);
        self.net.register_node(&node_id, gsb.public_addr());

        node.fund(node_id).await.unwrap();

        self.nodes.push(node);
        self
    }

    pub async fn break_networking_for(&self, node_name: &str) -> Result<()> {
        for id in self.list_ids(node_name).await {
            self.net.unregister_node(&id.identity)?
        }
        Ok(())
    }

    pub async fn enable_networking_for(&self, node_name: &str) -> Result<()> {
        for id in self.list_ids(node_name).await {
            let gsb = gsb_prefixes(&self.test_name, node_name);
            self.net.register_node(&id.identity, gsb.public_addr());
        }
        Ok(())
    }

    pub async fn add_market_instance(self, name: &str) -> Self {
        let db = self.create_database(name);
        let gsb = self.node_gsb_prefixes(name);

        let identity_api = IdentityGSB::new(gsb.clone());
        let identity = RealIdentity::new(self.net.clone(), &self.test_dir.join(name), name)
            .with_prefixed_gsb(Some(gsb));

        let market = Arc::new(
            MarketService::new(
                &db,
                identity_api.clone() as Arc<dyn IdentityApi>,
                self.config.clone(),
            )
            .unwrap(),
        );

        self.add_node(name, identity_api, identity, MockNodeKind::Market(market))
            .await
    }

    pub async fn add_matcher_instance(self, name: &str) -> Self {
        let db = self.init_database(name);
        let gsb = self.node_gsb_prefixes(name);
        let scan_set = ScannerSet::new(db.clone());

        let store = SubscriptionStore::new(db.clone(), scan_set, self.config.clone());

        let identity_api = IdentityGSB::new(gsb.clone());
        let identity = RealIdentity::new(self.net.clone(), &self.test_dir.join(name), name)
            .with_prefixed_gsb(Some(gsb));

        let (matcher, listeners) =
            Matcher::new(store, identity_api.clone(), self.config.clone()).unwrap();
        self.add_node(
            name,
            identity_api,
            identity,
            MockNodeKind::Matcher { matcher, listeners },
        )
        .await
    }

    pub async fn add_discovery_instance(self, name: &str, builder: DiscoveryBuilder) -> Self {
        let _db = self.init_database(name);
        let gsb = self.node_gsb_prefixes(name);

        let identity_api = IdentityGSB::new(gsb.clone());
        let identity = RealIdentity::new(self.net.clone(), &self.test_dir.join(name), name)
            .with_prefixed_gsb(Some(gsb));

        let discovery = builder
            .add_data(identity_api.clone() as Arc<dyn IdentityApi>)
            .with_config(self.config.discovery.clone())
            .build()
            .unwrap();
        self.add_node(
            name,
            identity_api,
            identity,
            MockNodeKind::Discovery(discovery),
        )
        .await
    }

    pub fn discovery_builder(&self) -> DiscoveryBuilder {
        DiscoveryBuilder::default()
            .with_config(self.config.discovery.clone())
            .add_handler(default::empty_on_offers_retrieved)
            .add_handler(default::empty_on_offers_bcast)
            .add_handler(default::empty_on_offer_unsubscribed_bcast)
            .add_handler(default::empty_on_retrieve_offers)
            .add_handler(default::empty_query_offers_handler)
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

        let gsb = self.node_gsb_prefixes(name);

        let identity_api = IdentityGSB::new(gsb.clone());
        let identity = RealIdentity::new(self.net.clone(), &self.test_dir.join(name), name)
            .with_prefixed_gsb(Some(gsb));

        self.add_node(
            name,
            identity_api,
            identity,
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

    pub async fn get_default_id(&self, node_name: &str) -> Identity {
        let api = self
            .nodes
            .iter()
            .find(|node| node.name == node_name)
            .map(|node| node.identity_api.clone())
            .unwrap();
        let id = api.default_identity().await.unwrap();
        Identity {
            identity: id,
            name: "".to_string(),
            role: "".to_string(),
        }
    }

    pub async fn create_identity(&self, node_name: &str, id_name: &str) -> Identity {
        let node = self
            .nodes
            .iter()
            .find(|node| node.name == node_name)
            .unwrap();

        let id = node.identity.create_identity(id_name).await.unwrap();

        // Sleep to allow propagating identity events to market to register the account
        // related to the identity.
        tokio::time::sleep(Duration::from_millis(100)).await;
        node.fund(id.node_id).await.unwrap();

        Identity {
            identity: id.node_id,
            name: id_name.to_string(),
            role: "".to_string(),
        }
    }

    pub async fn list_ids(&self, node_name: &str) -> Vec<Identity> {
        let node = self
            .nodes
            .iter()
            .find(|node| node.name == node_name)
            .unwrap();
        node.identity_api
            .list()
            .await
            .unwrap()
            .into_iter()
            .map(|info| Identity {
                identity: info.node_id,
                name: info.alias.unwrap_or_default(),
                role: "".to_string(),
            })
            .collect()
    }

    pub async fn get_rest_app(
        &self,
        node_name: &str,
    ) -> impl ActixService<Request, Response = ServiceResponse<BoxBody>, Error = actix_web::Error>
    {
        let market = self.get_market(node_name);
        let identity = self.get_default_id(node_name).await;

        test::init_service(
            App::new()
                .wrap(DummyAuth::new(identity))
                .service(MarketService::bind_rest(market)),
        )
        .await
    }

    fn create_database(&self, name: &str) -> DbMixedExecutor {
        let db_path = self.instance_dir(name);
        let db_name = self.node_gsb_prefixes(name).local_addr().to_string();

        let disk_db = DbExecutor::from_data_dir(&db_path, "yagna")
            .map_err(|e| anyhow!("Failed to create db [{:?}]. Error: {}", db_path, e))
            .unwrap();
        let ram_db = DbExecutor::in_memory(&db_name)
            .map_err(|e| {
                anyhow!(
                    "Failed to create in memory db [{:?}]. Error: {}",
                    db_name,
                    e
                )
            })
            .unwrap();

        DbMixedExecutor::new(disk_db, ram_db)
    }

    pub fn init_database(&self, name: &str) -> DbMixedExecutor {
        let db = self.create_database(name);
        MarketService::apply_migrations(&db).unwrap();
        db
    }

    fn instance_dir(&self, name: &str) -> PathBuf {
        let dir = self.test_dir.join(name);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    pub fn node_gsb_prefixes(&self, node_name: &str) -> GsbBindPoints {
        gsb_prefixes(&self.test_name, node_name)
    }

    pub fn market_gsb_prefixes(&self, node_name: &str) -> GsbBindPoints {
        gsb_market_prefixes(gsb_prefixes(&self.test_name, node_name))
    }
}

pub fn prepare_test_dir(test_dir: &Path) -> Result<PathBuf> {
    log::info!(
        "[MockNode] Preparing test directory: {}",
        test_dir.display()
    );
    if test_dir.exists() {
        fs::remove_dir_all(test_dir)
            .with_context(|| format!("Removing test directory: {}", test_dir.display()))?;
    }
    fs::create_dir_all(test_dir)
        .with_context(|| format!("Creating test directory: {}", test_dir.display()))?;
    Ok(test_dir.to_path_buf())
}

#[macro_export]
macro_rules! assert_err_eq {
    ($expected:expr, $actual:expr $(,)*) => {
        assert_eq!($expected.to_string(), $actual.unwrap_err().to_string())
    };
}

pub mod default {
    use super::*;
    use ya_market::testing::{
        AgreementApproved, AgreementCancelled, AgreementCommitted, AgreementReceived,
        AgreementRejected, AgreementTerminated, InitialProposalReceived, OffersBcast,
        OffersRetrieved, ProposalReceived, ProposalRejected, QueryOffers, QueryOffersResult,
        RetrieveOffers, UnsubscribedOffersBcast,
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

    pub async fn empty_query_offers_handler(
        _caller: String,
        _msg: QueryOffers,
    ) -> Result<QueryOffersResult, DiscoveryRemoteError> {
        Ok(QueryOffersResult::default())
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
    ) -> Result<(), AgreementProtocolError> {
        Ok(())
    }

    pub async fn empty_on_agreement_rejected(
        _caller: String,
        _msg: AgreementRejected,
    ) -> Result<(), AgreementProtocolError> {
        Ok(())
    }

    pub async fn empty_on_agreement_cancelled(
        _caller: String,
        _msg: AgreementCancelled,
    ) -> Result<(), AgreementProtocolError> {
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
        network: GolemBaseNetwork::Local,
        ..DiscoveryConfig::from_env().unwrap()
    };

    let mut cfg = Config::from_env().unwrap();
    cfg.discovery = discovery;
    cfg
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
    'retry: for _i in 0..30 {
        for subscription in subscriptions.clone() {
            for mkt in mkts {
                if mkt.get_offer(subscription).await.is_err() {
                    // Every 150ms we should get at least one broadcast from each Node.
                    // After a few tries all nodes should have the same knowledge about Offers.
                    tokio::time::sleep(Duration::from_millis(250)).await;
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

/// Assure that all given nodes have the same knowledge about given Offer Unsubscribes.
/// Wait if needed at most 2,5s ( = 10 x 250ms).
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
                match mkt.get_offer(subscription).await {
                    Err(e) => assert_eq!(e.to_string(), expect_error),
                    Ok(_) => {
                        // Every 150ms we should get at least one broadcast from each Node.
                        // After a few tries all nodes should have the same knowledge about Offers.
                        tokio::time::sleep(Duration::from_millis(250)).await;
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
