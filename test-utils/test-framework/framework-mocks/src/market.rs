pub mod legacy;

use chrono::{Duration, Utc};
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;

use ya_agreement_utils::agreement::expand;
use ya_agreement_utils::{OfferTemplate, ProposalView};
use ya_client_model::market::agreement;
use ya_client_model::market::proposal;
use ya_client_model::market::{Agreement, AgreementListEntry, Demand, Offer, Role};
use ya_client_model::NodeId;
use ya_core_model::market;
use ya_market::testing::{AgreementId, Owner, ProposalId, SubscriptionId};
use ya_service_bus::typed as bus;

/// Market that doesn't wrap real Market module, but simulates it's
/// behavior by providing GSB bindings for crucial messages.
#[derive(Clone)]
pub struct FakeMarket {
    name: String,
    _testdir: PathBuf,

    inner: Arc<RwLock<FakeMarketInner>>,
}

pub struct FakeMarketInner {
    agreements: HashMap<AgreementId, Agreement>,
}

impl FakeMarket {
    pub fn new(name: &str, testdir: &Path) -> Self {
        FakeMarket {
            name: name.to_string(),
            _testdir: testdir.to_path_buf(),
            inner: Arc::new(RwLock::new(FakeMarketInner {
                agreements: HashMap::new(),
            })),
        }
    }

    pub async fn bind_gsb(&self) -> anyhow::Result<()> {
        log::info!("FakeMarket ({}) - binding GSB", self.name);

        let self_ = self.clone();
        bus::bind_with_caller(
            market::local::BUS_ID,
            move |sender: String, msg: market::GetAgreement| {
                let self_ = self_.clone();
                async move { self_.get_agreement_handler(sender, msg).await }
            },
        );
        let self_ = self.clone();
        bus::bind_with_caller(
            market::local::BUS_ID,
            move |sender: String, msg: market::ListAgreements| {
                let self_ = self_.clone();
                async move { self_.list_agreements_handler(sender, msg).await }
            },
        );
        Ok(())
    }

    async fn get_agreement_handler(
        &self,
        _sender_id: String,
        msg: market::GetAgreement,
    ) -> Result<Agreement, market::RpcMessageError> {
        let owner = match msg.role {
            Role::Provider => Owner::Provider,
            Role::Requestor => Owner::Requestor,
        };

        let agreement_id = AgreementId::from_client(&msg.agreement_id, owner)
            .map_err(|e| market::RpcMessageError::Market(e.to_string()))?;

        self.get_agreement(agreement_id.clone())
            .await
            .ok_or_else(|| {
                market::RpcMessageError::NotFound(format!("Agreement id: {agreement_id}"))
            })
    }

    async fn list_agreements_handler(
        &self,
        _sender_id: String,
        msg: market::ListAgreements,
    ) -> Result<Vec<market::AgreementListEntry>, market::RpcMessageError> {
        let lock = self.inner.read().await;
        let agreements = lock
            .agreements
            .iter()
            .filter(|(_, agreement)| {
                msg.app_session_id.is_none() || agreement.app_session_id == msg.app_session_id
            })
            .filter(|(_, agreement)| msg.state.is_none() || agreement.state == msg.state.unwrap())
            .filter(|(_, agreement)| {
                msg.before_date.is_none() || agreement.timestamp < msg.before_date.unwrap()
            })
            .filter(|(_, agreement)| {
                msg.after_date.is_none() || agreement.timestamp > msg.after_date.unwrap()
            })
            .map(|(id, agreement)| AgreementListEntry {
                id: agreement.agreement_id.clone(),
                timestamp: agreement.timestamp,
                approved_date: agreement.approved_date,
                role: match id.owner() {
                    Owner::Provider => Role::Provider,
                    Owner::Requestor => Role::Requestor,
                },
            })
            .collect();

        Ok(agreements)
    }

    pub async fn get_agreement(&self, agreement_id: AgreementId) -> Option<Agreement> {
        self.inner
            .read()
            .await
            .agreements
            .get(&agreement_id)
            .cloned()
    }

    pub async fn add_agreement(&self, agreement: Agreement) {
        let provider_id =
            AgreementId::from_client(&agreement.agreement_id, Owner::Provider).unwrap();
        let requestor_id =
            AgreementId::from_client(&agreement.agreement_id, Owner::Requestor).unwrap();

        let mut lock = self.inner.write().await;
        lock.agreements.insert(provider_id, agreement.clone());
        lock.agreements.insert(requestor_id, agreement);
    }

    pub fn create_fake_agreement(
        requestor_id: NodeId,
        provider_id: NodeId,
    ) -> anyhow::Result<Agreement> {
        let offer = Self::create_default_offer(provider_id)?;
        let demand = Self::create_default_demand(requestor_id)?;

        Self::agreement_from(offer, demand)
    }

    pub fn agreement_from(offer: ProposalView, demand: ProposalView) -> anyhow::Result<Agreement> {
        let timestamp = Utc::now();
        let agreement_id = ProposalId::generate_id(
            &SubscriptionId::from_str(&offer.id)?,
            &SubscriptionId::from_str(&demand.id)?,
            &timestamp.naive_utc(),
            Owner::Requestor,
        );
        Ok(Agreement {
            agreement_id: agreement_id.into_client(),
            demand: Demand {
                properties: demand.content.properties,
                constraints: demand.content.constraints,
                demand_id: demand.id,
                requestor_id: demand.issuer,
                timestamp: demand.timestamp,
                expiration: demand.timestamp + Duration::hours(1),
            },
            offer: Offer {
                properties: offer.content.properties,
                constraints: offer.content.constraints,
                offer_id: offer.id,
                provider_id: offer.issuer,
                timestamp: offer.timestamp,
                expiration: offer.timestamp + Duration::hours(1),
            },
            valid_to: timestamp + Duration::hours(2),
            approved_date: None,
            state: agreement::State::Approved,
            timestamp,
            app_session_id: None,
            proposed_signature: None,
            approved_signature: None,
            committed_signature: None,
        })
    }

    pub fn create_default_offer(provider_id: NodeId) -> anyhow::Result<ProposalView> {
        let template = OfferTemplate {
            properties: expand(serde_json::from_str(r#"{ "any": "thing" }"#).unwrap()),
            constraints: "()".to_string(),
        };
        Self::create_demand(provider_id, template)
    }

    pub fn create_offer(
        provider_id: NodeId,
        content: OfferTemplate,
    ) -> anyhow::Result<ProposalView> {
        let offer = ProposalView {
            id: "".to_string(),
            content: content.flatten(),
            issuer: provider_id,
            state: proposal::State::Accepted,
            timestamp: Utc::now(),
        };

        let id = subscription_id_from(&offer)?.to_string();
        Ok(ProposalView { id, ..offer })
    }

    pub fn create_default_demand(requestor_id: NodeId) -> anyhow::Result<ProposalView> {
        let basic_props = json!({
            "golem.com.payment.platform.erc20-holesky-tglm.address": requestor_id.to_string(),
            "golem.com.payment.protocol.version": 3,
            "golem.com.payment.chosen-platform": "erc20-holesky-tglm",
        });

        let template = OfferTemplate {
            properties: expand(basic_props),
            constraints: "()".to_string(),
        };
        Self::create_demand(requestor_id, template)
    }
    pub fn create_demand(
        requestor_id: NodeId,
        content: OfferTemplate,
    ) -> anyhow::Result<ProposalView> {
        let demand = ProposalView {
            id: "".to_string(),
            content: content.flatten(),
            issuer: requestor_id,
            state: proposal::State::Accepted,
            timestamp: Utc::now(),
        };

        let id = subscription_id_from(&demand)?.to_string();
        Ok(ProposalView { id, ..demand })
    }
}

fn subscription_id_from(template: &ProposalView) -> anyhow::Result<SubscriptionId> {
    let id = SubscriptionId::generate_id(
        &serde_json::to_string_pretty(&template.content.properties)?,
        &serde_json::to_string_pretty(&template.content.constraints)?,
        &template.issuer,
        &template.timestamp.naive_utc(),
        &(template.timestamp + Duration::hours(2)).naive_utc(),
    );
    Ok(id)
}
