use chrono::{DateTime, Duration, Utc};

use crate::db::model::AgreementId;

use crate::testing::proposal_util::{exchange_proposals_exclusive, NegotiationHelper};
use crate::testing::MarketsNetwork;
use crate::testing::OwnerType;

pub struct NegotiationAgreementHelper {
    pub negotiation: NegotiationHelper,
    pub p_agreement: AgreementId,
    pub r_agreement: AgreementId,
    pub confirm_timestamp: DateTime<Utc>,
}

pub async fn negotiate_agreement(
    network: &MarketsNetwork,
    req_name: &str,
    prov_name: &str,
    match_on: &str,
    r_session: &str,
    p_session: &str,
) -> Result<NegotiationAgreementHelper, anyhow::Error> {
    let req_mkt = network.get_market(req_name);
    let prov_mkt = network.get_market(prov_name);

    let req_id = network.get_default_id(req_name);
    let prov_id = network.get_default_id(prov_name);

    let negotiation = exchange_proposals_exclusive(network, req_name, prov_name, match_on).await?;

    let r_agreement = req_mkt
        .requestor_engine
        .create_agreement(
            req_id.clone(),
            &negotiation.proposal_id,
            Utc::now() + Duration::hours(1),
        )
        .await?;

    let confirm_timestamp = Utc::now();
    req_mkt
        .requestor_engine
        .confirm_agreement(req_id.clone(), &r_agreement, Some(r_session.to_string()))
        .await?;

    let p_agreement = r_agreement.clone().translate(OwnerType::Provider);
    prov_mkt
        .provider_engine
        .approve_agreement(
            prov_id.clone(),
            &p_agreement,
            Some(p_session.to_string()),
            0.2,
        )
        .await?;

    req_mkt
        .requestor_engine
        .wait_for_approval(&r_agreement, 0.2)
        .await?;

    Ok(NegotiationAgreementHelper {
        negotiation,
        p_agreement,
        r_agreement,
        confirm_timestamp,
    })
}
