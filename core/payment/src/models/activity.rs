use crate::schema::pay_activity;
use ya_client_model::NodeId;
use ya_persistence::types::{BigDecimalField, Role};

#[derive(Debug, Insertable, Queryable, Identifiable)]
#[table_name = "pay_activity"]
#[primary_key(id, owner_id)]
pub struct WriteObj {
    pub id: String,
    pub owner_id: NodeId,
    pub role: Role,
    pub agreement_id: String,
    pub total_amount_due: BigDecimalField,
    pub total_amount_accepted: BigDecimalField,
    pub total_amount_scheduled: BigDecimalField,
    pub total_amount_paid: BigDecimalField,
}

impl WriteObj {
    pub fn new(id: String, owner_id: NodeId, role: Role, agreement_id: String) -> Self {
        Self {
            id,
            owner_id,
            role,
            agreement_id,
            total_amount_due: Default::default(),
            total_amount_accepted: Default::default(),
            total_amount_scheduled: Default::default(),
            total_amount_paid: Default::default(),
        }
    }
}

#[derive(Queryable, Debug, Identifiable)]
#[table_name = "pay_activity"]
#[primary_key(id, owner_id)]
pub struct ReadObj {
    pub id: String,
    pub owner_id: NodeId,
    pub role: Role,
    pub agreement_id: String,
    pub total_amount_due: BigDecimalField,
    pub total_amount_accepted: BigDecimalField,
    pub total_amount_scheduled: BigDecimalField,
    pub total_amount_paid: BigDecimalField,

    pub peer_id: NodeId,    // From Agreement
    pub payee_addr: String, // From Agreement
    pub payer_addr: String, // From Agreement
}
