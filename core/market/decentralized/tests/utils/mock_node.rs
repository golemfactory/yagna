use anyhow::{anyhow, Context, Result};
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use ya_client::model::NodeId;
use ya_market_decentralized::protocol::{
    CallbackHandler, Discovery, OfferReceived, OfferUnsubscribed, RetrieveOffers,
};
use ya_market_decentralized::testing::negotiation::messages::{
    AgreementApproved, AgreementReceived, AgreementRejected, InitialProposalReceived,
    ProposalReceived, ProposalRejected,
};
use ya_market_decentralized::testing::negotiation::provider;
use ya_market_decentralized::testing::negotiation::requestor;
use ya_market_decentralized::{MarketService, SubscriptionId};
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;

use super::mock_net::MockNet;

/// Instantiates market test nodes inside one process.
pub struct MarketsNetwork {
    net: MockNet,
    markets: Vec<MarketNode>,
    discoveries: Vec<DiscoveryNode>,
    negotiations: Vec<NegotiationNode>,
    test_dir: PathBuf,
    test_name: String,
}

/// Store all object associated with single market
/// for example: Database
pub struct MarketNode {
    market: Arc<MarketService>,
    name: String,
    /// For now only mock default Identity.
    id: Identity,
    /// Direct access to underlying database.
    db: DbExecutor,
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
        let net = MockNet::new().unwrap();

        MarketsNetwork {
            net,
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

        let (public_gsb_prefix, local_gsb_prefix) =
            self.net.gsb_prefixes(&self.test_name, name.as_ref());
        market
            .bind_gsb(&public_gsb_prefix, &local_gsb_prefix)
            .await?;

        let market_node = MarketNode {
            name: name.as_ref().to_string(),
            id: generate_identity(name.as_ref()),
            market,
            db,
        };

        self.net
            .register_node(&market_node.id.identity, &public_gsb_prefix)
            .await?;
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
        let (public_gsb_prefix, local_gsb_prefix) =
            self.net.gsb_prefixes(&self.test_name, name.as_ref());

        let discovery = Discovery::new(offer_received, offer_unsubscribed, retrieve_offers)?;
        discovery
            .bind_gsb(&public_gsb_prefix, &local_gsb_prefix)
            .await?;

        let discovery_node = DiscoveryNode {
            name: name.as_ref().to_string(),
            id: generate_identity(name.as_ref()),
            discovery,
        };

        self.net
            .register_node(&discovery_node.id.identity, &public_gsb_prefix)
            .await?;
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
        prov_agreement_cancelled: impl CallbackHandler<AgreementRejected>,
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
            default::empty_on_agreement_rejected,
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
        prov_agreement_cancelled: impl CallbackHandler<AgreementRejected>,
        req_proposal_received: impl CallbackHandler<ProposalReceived>,
        req_proposal_rejected: impl CallbackHandler<ProposalRejected>,
        req_agreement_approved: impl CallbackHandler<AgreementApproved>,
        req_agreement_rejected: impl CallbackHandler<AgreementRejected>,
    ) -> Result<Self> {
        let (public_gsb_prefix, local_gsb_prefix) =
            self.net.gsb_prefixes(&self.test_name, name.as_ref());

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

        self.net
            .register_node(&node.id.identity, &public_gsb_prefix)
            .await?;
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

pub fn prepare_test_dir<Str: AsRef<str>>(dir_name: Str) -> Result<PathBuf, anyhow::Error> {
    let test_dir: PathBuf = test_data_dir().join(dir_name.as_ref());

    if test_dir.exists() {
        fs::remove_dir_all(&test_dir)
            .with_context(|| format!("Removing test directory: {}", test_dir.display()))?;
    }
    fs::create_dir_all(&test_dir)
        .with_context(|| format!("Creating test directory: {}", test_dir.display()))?;
    Ok(test_dir)
}

fn generate_identity(name: &str) -> Identity {
    let random_node_id: String = thread_rng().sample_iter(&Alphanumeric).take(20).collect();

    Identity {
        name: name.to_string(),
        role: "manager".to_string(),
        identity: NodeId::from(random_node_id[..].as_bytes()),
    }
}

/// Facilitates waiting for broadcast propagation.
pub async fn wait_for_bcast(
    grace_millis: u64,
    market: &MarketService,
    subscription_id: &SubscriptionId,
    stop_is_some: bool,
) -> Result<()> {
    let steps = 20;
    let wait_step = Duration::from_millis(grace_millis / steps);
    let matcher = market.matcher.clone();
    for _ in 0..steps {
        tokio::time::delay_for(wait_step).await;
        if matcher.get_offer(&subscription_id).await?.is_some() == stop_is_some {
            break;
        }
    }
    Ok(())
}

pub mod default {
    use ya_market_decentralized::protocol::{
        DiscoveryRemoteError, OfferReceived, OfferUnsubscribed, Propagate, RetrieveOffers,
        StopPropagateReason,
    };
    use ya_market_decentralized::testing::negotiation::errors::{AgreementError, ProposalError};
    use ya_market_decentralized::testing::negotiation::messages::{
        AgreementApproved, AgreementReceived, AgreementRejected, InitialProposalReceived,
        ProposalReceived, ProposalRejected,
    };
    use ya_market_decentralized::Offer;

    pub async fn empty_on_offer_received(
        _caller: String,
        _msg: OfferReceived,
    ) -> Result<Propagate, ()> {
        Ok(Propagate::False(StopPropagateReason::AlreadyExists))
    }

    pub async fn empty_on_offer_unsubscribed(
        _caller: String,
        _msg: OfferUnsubscribed,
    ) -> Result<Propagate, ()> {
        Ok(Propagate::False(StopPropagateReason::AlreadyUnsubscribed))
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
}
