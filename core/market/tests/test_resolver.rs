use std::{future::Future, time::Duration};
use tokio::time::{timeout, Timeout};

use ya_market::testing::{
    client::{sample_demand, sample_offer},
    MarketsNetwork,
};

/// Test adds Offer on single node. Resolver should not emit Proposal.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_single_not_resolve_offer() {
    // given
    let _ = env_logger::builder().try_init();
    let mut network = MarketsNetwork::new(None)
        .await
        .add_matcher_instance("Node-1")
        .await;

    let id1 = network.get_default_id("Node-1");
    let provider = network.get_matcher("Node-1");
    let offer = sample_offer();

    // when
    let _offer = provider.subscribe_offer(&offer, &id1).await.unwrap();

    // then
    let listener = network.get_event_listeners("Node-1");
    assert!(timeout3s(listener.proposal_receiver.recv()).await.is_err());
}

/// Test adds Offer and Demand. Resolver should emit Proposal on Demand node.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_resolve_offer_demand() {
    // given
    let _ = env_logger::builder().try_init();
    let mut network = MarketsNetwork::new(None)
        .await
        .add_matcher_instance("Provider-1")
        .await
        .add_matcher_instance("Requestor-1")
        .await;

    let id1 = network.get_default_id("Provider-1");
    let provider = network.get_matcher("Provider-1");
    let id2 = network.get_default_id("Requestor-1");
    let requestor = network.get_matcher("Requestor-1");

    // when: Add Offer on Provider
    let offer = provider
        .subscribe_offer(&sample_offer(), &id1)
        .await
        .unwrap();
    // and: Add Demand on Requestor
    let demand = requestor
        .subscribe_demand(&sample_demand(), &id2)
        .await
        .unwrap();

    // then: It should be resolved on Requestor
    let listener = network.get_event_listeners("Requestor-1");
    let proposal = timeout3s(listener.proposal_receiver.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(proposal.offer, offer);
    assert_eq!(proposal.demand, demand);

    // and: but not resolved on Provider.
    let listener = network.get_event_listeners("Provider-1");
    assert!(timeout3s(listener.proposal_receiver.recv()).await.is_err());
}

/// Test adds Demand on single node. Resolver should not emit Proposal.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_single_not_resolve_demand() {
    // given
    let _ = env_logger::builder().try_init();
    let mut network = MarketsNetwork::new(None)
        .await
        .add_matcher_instance("Node-1")
        .await;

    let demand = sample_demand();
    let id1 = network.get_default_id("Node-1");
    let requestor = network.get_matcher("Node-1");

    // when
    let _demand = requestor.subscribe_demand(&demand, &id1).await.unwrap();

    // then
    let listener = network.get_event_listeners("Node-1");
    assert!(timeout3s(listener.proposal_receiver.recv()).await.is_err());
}

/// Test adds Offer on two nodes and Demand third. Resolver should emit two Proposals on Demand node.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_resolve_2xoffer_demand() {
    // given
    let _ = env_logger::builder().try_init();
    let mut network = MarketsNetwork::new(None)
        .await
        .add_matcher_instance("Provider-1")
        .await
        .add_matcher_instance("Provider-2")
        .await
        .add_matcher_instance("Requestor-1")
        .await;

    let id1 = network.get_default_id("Provider-1");
    let provider1 = network.get_matcher("Provider-1");
    let id2 = network.get_default_id("Provider-2");
    let provider2 = network.get_matcher("Provider-2");
    let id3 = network.get_default_id("Requestor-1");
    let requestor = network.get_matcher("Requestor-1");

    // when: Add Offer on Provider-1
    let offer1 = provider1
        .subscribe_offer(&sample_offer(), &id1)
        .await
        .unwrap();
    // when: Add Offer on Provider-2
    let offer2 = provider2
        .subscribe_offer(&sample_offer(), &id2)
        .await
        .unwrap();
    // and: Add Demand on Requestor
    let demand = requestor
        .subscribe_demand(&sample_demand(), &id3)
        .await
        .unwrap();

    // then: It should be resolved on Requestor two times
    let listener = network.get_event_listeners("Requestor-1");
    let proposal1 = timeout3s(listener.proposal_receiver.recv())
        .await
        .unwrap()
        .unwrap();
    let proposal2 = timeout3s(listener.proposal_receiver.recv())
        .await
        .unwrap()
        .unwrap();

    assert_eq!(proposal1.demand, demand);
    assert_eq!(proposal2.demand, demand);

    // Check if we got Proposals for both Offers. This check should be
    // order independent, since we don't force any ordering rules on Proposals.
    let proposals = vec![proposal1, proposal2];
    assert!(proposals.iter().any(|proposal| proposal.offer == offer1));
    assert!(proposals.iter().any(|proposal| proposal.offer == offer2));

    // and: but not resolved on Provider-1
    let listener = network.get_event_listeners("Provider-1");
    assert!(timeout3s(listener.proposal_receiver.recv()).await.is_err());
    // and: not on Provider-2.
    let listener = network.get_event_listeners("Provider-2");
    assert!(timeout3s(listener.proposal_receiver.recv()).await.is_err());
}

fn timeout3s<T: Future>(fut: T) -> Timeout<T> {
    timeout(Duration::from_secs(3), fut)
}
