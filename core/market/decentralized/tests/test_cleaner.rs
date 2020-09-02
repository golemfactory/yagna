use anyhow::Result;
use chrono::{Duration, NaiveDateTime, Utc};
use std::str::FromStr;
use ya_client::model::NodeId;
use ya_market_decentralized::testing::cleaner::clean;
use ya_market_decentralized::testing::dao::TestingDao;
use ya_market_decentralized::testing::{
    Agreement, AgreementDao, AgreementState, DbProposal, Demand, DemandDao, EventType, IssuerType, MarketsNetwork,
    Negotiation, Offer, OfferDao, OwnerType, ProposalId, ProposalState, SubscriptionId,
};
use ya_market_decentralized::testing::events_helper::TestMarketEvent;
use ya_persistence::executor::PoolType;

fn future() -> NaiveDateTime {
    (Utc::now() + Duration::days(10)).naive_utc()
}

fn past() -> NaiveDateTime {
    (Utc::now() - Duration::days(91)).naive_utc()
}

fn generate_negotiation(agreement_id: Option<ProposalId>) -> Negotiation {
    use uuid::Uuid;
    Negotiation {
        id: format!("{}", Uuid::new_v4()),
        subscription_id: SubscriptionId::from_str("c76161077d0343ab85ac986eb5f6ea38-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53",).unwrap(),
        offer_id: SubscriptionId::from_str("c76161077d0343ab85ac986eb5f6ea38-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53",).unwrap(),
        demand_id: SubscriptionId::from_str("c76161077d0343ab85ac986eb5f6ea38-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53",).unwrap(),
        provider_id: NodeId::from_str("0xbabe000000000000000000000000000000000000").unwrap(),
        requestor_id: NodeId::from_str("0xbabe000000000000000000000000000000000000").unwrap(),
        agreement_id,
    }
}

fn generate_agreement(unifier: i64, valid_to: NaiveDateTime) -> Agreement {
    Agreement {
        id: ProposalId::generate_id(
                &SubscriptionId::from_str("c76161077d0343ab85ac986eb5f6ea38-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53",).unwrap(),
                &SubscriptionId::from_str("c76161077d0343ab85ac986eb5f6ea38-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53",).unwrap(),
                // Add parametrized integer - unifier to ensure unique ids
                &(Utc::now() + Duration::days(unifier)).naive_utc(),
                OwnerType::Requestor,
        ),
        offer_properties: "".to_string(),
        offer_constraints: "".to_string(),
        demand_properties: "".to_string(),
        demand_constraints: "".to_string(),
        offer_id: SubscriptionId::from_str("c76161077d0343ab85ac986eb5f6ea38-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53",).unwrap(),
        demand_id: SubscriptionId::from_str("c76161077d0343ab85ac986eb5f6ea38-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53",).unwrap(),
        provider_id: NodeId::from_str("0xbabe000000000000000000000000000000000000").unwrap(),
        requestor_id: NodeId::from_str("0xbabe000000000000000000000000000000000000").unwrap(),
        creation_ts: Utc::now().naive_utc(),
        valid_to,
        approved_date: None,
        state: AgreementState::Proposal,
        proposed_signature: None,
        approved_signature: None,
        committed_signature: None,
    }
}

#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
async fn test_agreement() -> Result<()> {
    let valid_agreement = generate_agreement(1, future());
    let expired_agreement = generate_agreement(2, past());
    let db = MarketsNetwork::new("market-cleaner-agreement")
        .await
        .init_database("testnode")?;
    let agreement_dao = db.as_dao::<AgreementDao>();
    agreement_dao.save(valid_agreement.clone()).await?;
    agreement_dao.save(expired_agreement.clone()).await?;
    clean(db.clone()).await;
    assert_eq!(
        <PoolType as TestingDao<Agreement>>::exists(&db.clone().pool, valid_agreement.id).await,
        true
    );
    assert_eq!(
        <PoolType as TestingDao<Agreement>>::exists(&db.clone().pool, expired_agreement.id).await,
        false
    );
    Ok(())
}

fn generate_demand(id: &str, expiration_ts: NaiveDateTime) -> Demand {
    Demand {
        id: SubscriptionId::from_str(id).unwrap(),
        properties: "".to_string(),
        constraints: "".to_string(),
        node_id: NodeId::from_str("0xbabe000000000000000000000000000000000000").unwrap(),
        creation_ts: Utc::now().naive_utc(),
        insertion_ts: None,
        expiration_ts,
    }
}

#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
async fn test_demand() -> Result<()> {
    // insert two demands (expired & active) with negotiations
    let valid_demand = generate_demand(
        "c76161077d0343ab85ac986eb5f6ea38-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53",
        future(),
        );
    let expired_demand = generate_demand(
        "c76161077d0343ab85ac986eb5f6ea38-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a54",
        past(),
        );
    let db = MarketsNetwork::new("market-cleaner-demand")
        .await
        .init_database("testnode")?;
    let demand_dao = db.as_dao::<DemandDao>();
    demand_dao.insert(&valid_demand).await?;
    demand_dao.insert(&expired_demand).await?;
    clean(db.clone()).await;
    assert_eq!(
        <PoolType as TestingDao<Demand>>::exists(&db.clone().pool, valid_demand.id).await,
        true
    );
    assert_eq!(
        <PoolType as TestingDao<Demand>>::exists(&db.clone().pool, expired_demand.id).await,
        false
    );
    Ok(())
}

fn generate_offer(id: &str, expiration_ts: NaiveDateTime) -> Offer {
    Offer {
        id: SubscriptionId::from_str(id).unwrap(),
        properties: "".to_string(),
        constraints: "".to_string(),
        node_id: NodeId::from_str("0xbabe000000000000000000000000000000000000").unwrap(),
        creation_ts: Utc::now().naive_utc(),
        insertion_ts: None,
        expiration_ts,
    }
}

#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
async fn test_offer() -> Result<()> {
    // insert two offers with negotiations
    let valid_offer = generate_offer(
        "c76161077d0343ab85ac986eb5f6ea38-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53",
        future(),
        );
    let expired_offer = generate_offer(
        "c76161077d0343ab85ac986eb5f6ea38-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a54",
        past(),
        );
    let db = MarketsNetwork::new("market-cleaner-offer")
        .await
        .init_database("testnode")?;
    let offer_dao = db.as_dao::<OfferDao>();
    let validation_ts = (Utc::now() - Duration::days(100)).naive_utc();
    offer_dao.insert(valid_offer.clone(), validation_ts.clone()).await?;
    offer_dao.insert(expired_offer.clone(), validation_ts.clone()).await?;
    clean(db.clone()).await;
    assert_eq!(
        <PoolType as TestingDao<Offer>>::exists(&db.clone().pool, valid_offer.id).await,
        true
    );
    assert_eq!(
        <PoolType as TestingDao<Offer>>::exists(&db.clone().pool, expired_offer.id).await,
        false
    );
    Ok(())
}

fn generate_event(id: i32, timestamp: NaiveDateTime) -> TestMarketEvent {
    TestMarketEvent {
        id,
        subscription_id: SubscriptionId::from_str("c76161077d0343ab85ac986eb5f6ea38-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53",).unwrap(),
        event_type: EventType::ProviderProposal,
        artifact_id: ProposalId::generate_id(
                &SubscriptionId::from_str("c76161077d0343ab85ac986eb5f6ea38-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53",).unwrap(),
                &SubscriptionId::from_str("c76161077d0343ab85ac986eb5f6ea38-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53",).unwrap(),
                &Utc::now().naive_utc(),
                OwnerType::Requestor,
        ),
        timestamp,
    }
}

#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
async fn test_events() -> Result<()> {
    // insert two events
    let db = MarketsNetwork::new("market-cleaner-event")
        .await
        .init_database("testnode")?;
    let valid_event = generate_event(1, future());
    let expired_event = generate_event(2, past());
    <PoolType as TestingDao<TestMarketEvent>>::raw_insert(&db.clone().pool, valid_event.clone()).await?;
    <PoolType as TestingDao<TestMarketEvent>>::raw_insert(&db.clone().pool, expired_event.clone()).await?;
    clean(db.clone()).await;
    assert_eq!(
        <PoolType as TestingDao<TestMarketEvent>>::exists(&db.clone().pool, valid_event.id).await,
        true
    );
    assert_eq!(
        <PoolType as TestingDao<TestMarketEvent>>::exists(&db.clone().pool, expired_event.id).await,
        false
    );
    Ok(())
}

fn generate_proposal(unifier: i64, expiration_ts: NaiveDateTime, negotiation_id: String) -> DbProposal {
    DbProposal {
        id: ProposalId::generate_id(
            &SubscriptionId::from_str("c76161077d0343ab85ac986eb5f6ea38-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53",).unwrap(),
            &SubscriptionId::from_str("c76161077d0343ab85ac986eb5f6ea38-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53",).unwrap(),
            // Add parametrized integer - unifier to ensure unique ids
            &(Utc::now() + Duration::days(unifier)).naive_utc(),
            OwnerType::Requestor,
        ),
        prev_proposal_id: None,
        issuer: IssuerType::Them,
        negotiation_id,
        properties: "".to_string(),
        constraints: "".to_string(),
        state: ProposalState::Initial,
        creation_ts: Utc::now().naive_utc(),
        expiration_ts,
    }
}

#[cfg_attr(not(feature = "market-test-suite"), ignore)]
#[actix_rt::test]
async fn test_proposal() -> Result<()> {
    let _ = env_logger::builder().try_init();
    let db = MarketsNetwork::new("market-cleaner-proposal")
        .await
        .init_database("testnode")?;
    let valid_negotiation = generate_negotiation(None);
    let expired_negotiation = generate_negotiation(None);
    <PoolType as TestingDao<Negotiation>>::raw_insert(&db.clone().pool, valid_negotiation.clone()).await?;
    <PoolType as TestingDao<Negotiation>>::raw_insert(&db.clone().pool, expired_negotiation.clone()).await?;
    let mut valid_proposals: Vec<DbProposal> = vec![];
    let mut expired_proposals: Vec<DbProposal> = vec![];

    for i in 0..10 {
        let proposal = if i == 0 {
            // first proposal is valid making whole negotiation valid
            generate_proposal(1, future(), valid_negotiation.id.clone())
        } else {
            generate_proposal(1, past(), valid_negotiation.id.clone())
        };
        <PoolType as TestingDao<DbProposal>>::raw_insert(&db.clone().pool, proposal.clone()).await?;
        valid_proposals.push(proposal.clone());
    }

    for i in 0..10 {
        let proposal = generate_proposal(i*10, past(), expired_negotiation.id.clone());
        <PoolType as TestingDao<DbProposal>>::raw_insert(&db.clone().pool, proposal.clone()).await?;
        expired_proposals.push(proposal.clone());
    }
    clean(db.clone()).await;
    assert_eq!(
        <PoolType as TestingDao<Negotiation>>::exists(&db.clone().pool, valid_negotiation.id).await,
        true
    );
    for proposal in valid_proposals.into_iter() {
        assert_eq!(
            <PoolType as TestingDao<DbProposal>>::exists(&db.clone().pool, proposal.id).await,
            true
        );
    }
    assert_eq!(
        <PoolType as TestingDao<Negotiation>>::exists(&db.clone().pool, expired_negotiation.id).await,
        false
    );
    for proposal in expired_proposals.into_iter() {
        assert_eq!(
            <PoolType as TestingDao<DbProposal>>::exists(&db.clone().pool, proposal.id).await,
            false
        );
    }
    Ok(())
}
