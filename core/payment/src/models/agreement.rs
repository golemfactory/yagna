use crate::schema::pay_agreement;
use serde_json::Value;
use ya_agreement_utils::agreement::{expand, TypedPointer};
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
    pub total_amount_scheduled: BigDecimalField,
    pub total_amount_paid: BigDecimalField,
    pub app_session_id: Option<String>,
}

impl WriteObj {
    pub fn new(agreement: Agreement, role: Role) -> Self {
        let provider_id = *agreement.provider_id();
        let requestor_id = *agreement.requestor_id();
        let (owner_id, peer_id) = match &role {
            Role::Provider => (provider_id, requestor_id),
            Role::Requestor => (requestor_id, provider_id),
        };

        let demand_properties = expand(agreement.demand.properties);
        let offer_properties = expand(agreement.offer.properties);

        let payment_platform = demand_properties
            .pointer("/golem/com/payment/chosen-platform")
            .as_typed(Value::as_str)
            .expect("Demand property golem.com.payment.chosen-platform does not exist")
            .to_owned();
        let payee_addr = offer_properties
            .pointer(format!("/golem/com/payment/platform/{}/address", payment_platform).as_str())
            .as_typed(Value::as_str)
            .map(ToOwned::to_owned)
            .unwrap_or_else(|_| provider_id.to_string().to_lowercase());
        let payer_addr = demand_properties
            .pointer(format!("/golem/com/payment/platform/{}/address", payment_platform).as_str())
            .as_typed(Value::as_str)
            .map(ToOwned::to_owned)
            .unwrap_or_else(|_| requestor_id.to_string().to_lowercase());

        Self {
            id: agreement.agreement_id,
            owner_id,
            role,
            peer_id,
            payee_addr,
            payer_addr,
            payment_platform,
            total_amount_due: Default::default(),
            total_amount_accepted: Default::default(),
            total_amount_scheduled: Default::default(),
            total_amount_paid: Default::default(),
            app_session_id: agreement.app_session_id,
        }
    }
}

pub type ReadObj = WriteObj;

#[cfg(test)]
mod tests {
    use crate::models::agreement::WriteObj;
    use chrono::{Duration, Utc};
    use serde_json::json;
    use std::ops::Add;
    use ya_client_model::market::agreement::State;
    use ya_client_model::market::{Agreement, Demand, Offer};
    use ya_persistence::types::Role;

    fn mock_agreement_with_demand_properties(properties: serde_json::Value) -> Agreement {
        let demand = Demand::new(
            properties,
            "()".to_string(),
            "demand_id".to_string(),
            Default::default(),
            Default::default(),
        );

        let offer = Offer::new(
            json!({}),
            "()".to_string(),
            "offer_id".to_string(),
            Default::default(),
            Default::default(),
        );

        Agreement::new(
            "agreement_id".to_string(),
            demand,
            offer,
            Utc::now().add(Duration::days(1)),
            State::Proposal,
            Utc::now(),
        )
    }

    #[test]
    #[should_panic(expected = "Offer property golem.com.payment.chosen-platform does not exist")]
    fn cannot_create_agreement_without_chosen_platform() {
        let agreement = mock_agreement_with_demand_properties(json!({}));
        let role: Role = Role::Provider;

        WriteObj::new(agreement, role);
    }

    #[test]
    fn create_agreement_with_valid_chosen_platform() {
        let agreement = mock_agreement_with_demand_properties(json!({
            "golem.com.payment.chosen-platform": "test-network"
        }));
        let role: Role = Role::Provider;

        let result = WriteObj::new(agreement, role);
        assert_eq!(result.payment_platform, "test-network");
    }
}
