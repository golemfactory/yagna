use chrono::{Duration, NaiveDateTime, Utc};
use structopt::StructOpt;

use ya_market::testing::cleaner::clean;
use ya_market::testing::dao::TestingDao;
use ya_market::testing::events_helper::{generate_event, TestMarketEvent};
use ya_market::testing::mock_agreement::generate_agreement;
use ya_market::testing::mock_offer::{generate_demand, generate_offer};
use ya_market::testing::proposal_util::{generate_negotiation, generate_proposal};
use ya_market::testing::{
    Agreement, AgreementDao, DbConfig, DbProposal, Demand, DemandDao, MarketsNetwork, Negotiation,
    Offer, OfferDao,
};
use ya_persistence::executor::PoolType;

fn future() -> NaiveDateTime {
    (Utc::now() + Duration::days(10)).naive_utc()
}

fn past() -> NaiveDateTime {
    (Utc::now() - Duration::days(91)).naive_utc()
}

fn db_config() -> DbConfig {
    DbConfig::from_iter(&[""])
}

#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_agreement() {
    let _ = env_logger::builder().try_init();
    let valid_agreement = generate_agreement(1, future());
    let expired_agreement = generate_agreement(2, past());
    let db = MarketsNetwork::new(None).await.init_database("testnode");
    let agreement_dao = db.as_dao::<AgreementDao>();
    agreement_dao.save(valid_agreement.clone()).await.unwrap();
    agreement_dao.save(expired_agreement.clone()).await.unwrap();
    clean(db.clone(), &db_config()).await;
    assert_eq!(
        <PoolType as TestingDao<Agreement>>::exists(&db.clone().pool, valid_agreement.id).await,
        true
    );
    assert_eq!(
        <PoolType as TestingDao<Agreement>>::exists(&db.clone().pool, expired_agreement.id).await,
        false
    );
}

#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_demand() {
    // insert two demands (expired & active) with negotiations
    let valid_demand = generate_demand(
        "c76161077d0343ab85ac986eb5f6ea38-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53",
        future(),
        );
    let expired_demand = generate_demand(
        "c76161077d0343ab85ac986eb5f6ea38-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a54",
        past(),
        );
    let db = MarketsNetwork::new(None).await.init_database("testnode");
    let demand_dao = db.as_dao::<DemandDao>();
    demand_dao.insert(&valid_demand).await.unwrap();
    demand_dao.insert(&expired_demand).await.unwrap();
    clean(db.clone(), &db_config()).await;
    assert_eq!(
        <PoolType as TestingDao<Demand>>::exists(&db.clone().pool, valid_demand.id).await,
        true
    );
    assert_eq!(
        <PoolType as TestingDao<Demand>>::exists(&db.clone().pool, expired_demand.id).await,
        false
    );
}

#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_offer() {
    // insert two offers with negotiations
    let valid_offer = generate_offer(
        "c76161077d0343ab85ac986eb5f6ea38-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53",
        future(),
        );
    let expired_offer = generate_offer(
        "c76161077d0343ab85ac986eb5f6ea38-edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a54",
        past(),
        );
    let db = MarketsNetwork::new(None).await.init_database("testnode");
    let offer_dao = db.as_dao::<OfferDao>();
    let validation_ts = (Utc::now() - Duration::days(100)).naive_utc();
    offer_dao
        .put(valid_offer.clone(), validation_ts.clone())
        .await
        .unwrap();
    offer_dao
        .put(expired_offer.clone(), validation_ts.clone())
        .await
        .unwrap();
    clean(db.clone(), &db_config()).await;
    assert_eq!(
        <PoolType as TestingDao<Offer>>::exists(&db.clone().pool, valid_offer.id).await,
        true
    );
    assert_eq!(
        <PoolType as TestingDao<Offer>>::exists(&db.clone().pool, expired_offer.id).await,
        false
    );
}

#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_events() {
    // insert two events
    let db = MarketsNetwork::new(None).await.init_database("testnode");
    let valid_event = generate_event(1, future());
    let expired_event = generate_event(2, past());
    <PoolType as TestingDao<TestMarketEvent>>::raw_insert(&db.clone().pool, valid_event.clone())
        .await
        .unwrap();
    <PoolType as TestingDao<TestMarketEvent>>::raw_insert(&db.clone().pool, expired_event.clone())
        .await
        .unwrap();
    clean(db.clone(), &db_config()).await;
    assert_eq!(
        <PoolType as TestingDao<TestMarketEvent>>::exists(&db.clone().pool, valid_event.id).await,
        true
    );
    assert_eq!(
        <PoolType as TestingDao<TestMarketEvent>>::exists(&db.clone().pool, expired_event.id).await,
        false
    );
}

#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_proposal() {
    let _ = env_logger::builder().try_init();
    let db = MarketsNetwork::new(None).await.init_database("testnode");
    let valid_negotiation = generate_negotiation(None);
    let expired_negotiation = generate_negotiation(None);
    <PoolType as TestingDao<Negotiation>>::raw_insert(&db.clone().pool, valid_negotiation.clone())
        .await
        .unwrap();
    <PoolType as TestingDao<Negotiation>>::raw_insert(
        &db.clone().pool,
        expired_negotiation.clone(),
    )
    .await
    .unwrap();
    let mut valid_proposals: Vec<DbProposal> = vec![];
    let mut expired_proposals: Vec<DbProposal> = vec![];

    for i in 0..10 {
        let proposal = if i == 0 {
            // first proposal is valid making whole negotiation valid
            generate_proposal(1, future(), valid_negotiation.id.clone())
        } else {
            generate_proposal(1, past(), valid_negotiation.id.clone())
        };
        <PoolType as TestingDao<DbProposal>>::raw_insert(&db.clone().pool, proposal.clone())
            .await
            .unwrap();
        valid_proposals.push(proposal.clone());
    }

    for i in 0..10 {
        let proposal = generate_proposal(i * 10, past(), expired_negotiation.id.clone());
        <PoolType as TestingDao<DbProposal>>::raw_insert(&db.clone().pool, proposal.clone())
            .await
            .unwrap();
        expired_proposals.push(proposal.clone());
    }
    clean(db.clone(), &db_config()).await;
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
        <PoolType as TestingDao<Negotiation>>::exists(&db.clone().pool, expired_negotiation.id)
            .await,
        false
    );
    for proposal in expired_proposals.into_iter() {
        assert_eq!(
            <PoolType as TestingDao<DbProposal>>::exists(&db.clone().pool, proposal.id).await,
            false
        );
    }
}

#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_proposal_lotsa_negotiations() {
    // Due to diesel limitations we have to take care of processing
    // big amount of negotiations (manually) #672
    let _ = env_logger::builder().try_init();
    let db = MarketsNetwork::new(None).await.init_database("testnode");
    let mut expired_negotiations: Vec<Negotiation> = vec![];
    for i in 1..1500 {
        let expired_negotiation = generate_negotiation(None);
        <PoolType as TestingDao<Negotiation>>::raw_insert(
            &db.clone().pool,
            expired_negotiation.clone(),
        )
        .await
        .unwrap();
        let proposal = generate_proposal(i, past(), expired_negotiation.id.clone());
        <PoolType as TestingDao<DbProposal>>::raw_insert(&db.clone().pool, proposal.clone())
            .await
            .unwrap();
        expired_negotiations.push(expired_negotiation);
    }
    clean(db.clone(), &db_config()).await;
    for n in expired_negotiations {
        assert_eq!(
            <PoolType as TestingDao<Negotiation>>::exists(&db.clone().pool, n.id).await,
            false
        );
    }
}
