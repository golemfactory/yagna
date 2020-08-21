use crate::schema::pay_agreement;
use crate::DEFAULT_PAYMENT_PLATFORM;
use ya_client_model::market::Agreement;
use ya_client_model::NodeId;
use ya_persistence::types::{BigDecimalField, Role};

#[derive(Queryable, Debug, Identifiable, Insertable)]
#[table_name = "pay_agreement"]
#[primary_key(id, owner_id)]
pub struct WriteObj {
    pub id: String,
    pub owner_id: NodeId,
    pub role: Role,
    pub peer_id: NodeId,
    pub payee_addr: String,
    pub payer_addr: String,
    pub payment_platform: String,
    pub total_amount_due: BigDecimalField,
    pub total_amount_accepted: BigDecimalField,
    pub total_amount_paid: BigDecimalField,
}

impl WriteObj {
    pub fn new(agreement: Agreement, role: Role) -> Self {
        // FIXME: Provider & requestor ID should be non-optional NodeId fields
        let provider_id: NodeId = agreement.offer.provider_id.unwrap().parse().unwrap();
        let requestor_id: NodeId = agreement.demand.requestor_id.unwrap().parse().unwrap();
        let (owner_id, peer_id) = match &role {
            Role::Provider => (provider_id.clone(), requestor_id.clone()),
            Role::Requestor => (requestor_id.clone(), provider_id.clone()),
        };
        Self {
            id: agreement.agreement_id,
            owner_id,
            role,
            peer_id,
            payee_addr: provider_id.to_string().to_lowercase(), // TODO: Allow to specify different account
            payer_addr: requestor_id.to_string().to_lowercase(), // TODO: Allow to specify different account
            payment_platform: DEFAULT_PAYMENT_PLATFORM.to_string(), // TODO: Allow to specify different platform
            total_amount_due: Default::default(),
            total_amount_accepted: Default::default(),
            total_amount_paid: Default::default(),
        }
    }
}

pub type ReadObj = WriteObj;
