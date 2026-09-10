use chrono::{Duration, Utc};

use ya_client::model::market::{AgreementEventType, AgreementTerminationNotice};
use ya_framework_mocks::net::MockNet;
use ya_market::testing::agreement_utils::{gen_reason, negotiate_agreement};
use ya_market::testing::{MarketsNetwork, PostTerminationNoticeError};

const REQ_NAME: &str = "Node-1";
const PROV_NAME: &str = "Node-2";

/// Provider posts a termination notice: the Requestor gets an
/// `AgreementTerminationNoticeEvent`, the Agreement stays Approved and the
/// notice does not prevent either party from terminating it.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_termination_notice_event() {
    let network = MarketsNetwork::new(None, MockNet::new())
        .await
        .add_market_instance(REQ_NAME)
        .await
        .add_market_instance(PROV_NAME)
        .await;

    let req_market = network.get_market(REQ_NAME);
    let req_id = network.get_default_id(REQ_NAME);
    let prov_id = network.get_default_id(PROV_NAME);
    let prov_market = network.get_market(PROV_NAME);

    let negotiation = negotiate_agreement(
        &network,
        REQ_NAME,
        PROV_NAME,
        "negotiation",
        "r-session",
        "p-session",
    )
    .await
    .unwrap();

    // Take timestamp to filter AgreementApproved which should happen before.
    let reference_timestamp = Utc::now();
    let termination_deadline = Utc::now() + Duration::minutes(5);

    prov_market
        .provider_engine
        .post_termination_notice(
            prov_id.clone(),
            negotiation.p_agreement.into_client(),
            AgreementTerminationNotice {
                termination_deadline,
                reason: Some(gen_reason("Provider shutdown")),
            },
            2.0,
        )
        .await
        .unwrap();

    // == REQUESTOR sees the notice event.
    let events = req_market
        .query_agreement_events(&None, 3.0, Some(2), reference_timestamp, &req_id)
        .await
        .unwrap();

    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].agreement_id,
        negotiation.r_agreement.into_client()
    );
    match &events[0].event_type {
        AgreementEventType::AgreementTerminationNoticeEvent {
            termination_deadline: event_deadline,
            reason,
        } => {
            // Timestamps are stored with microsecond precision.
            assert_eq!(
                event_deadline.timestamp_micros(),
                termination_deadline.timestamp_micros()
            );
            assert_eq!(reason.as_ref().unwrap().message, "Provider shutdown");
        }
        e => panic!(
            "Expected AgreementEventType::AgreementTerminationNoticeEvent, got: {:?}",
            e
        ),
    };

    // == PROVIDER records the notice too.
    let events = prov_market
        .query_agreement_events(&None, 3.0, Some(2), reference_timestamp, &prov_id)
        .await
        .unwrap();

    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0].event_type,
        AgreementEventType::AgreementTerminationNoticeEvent { .. }
    ));

    // Requestor may terminate at any time, notice or not.
    req_market
        .terminate_agreement(
            req_id.clone(),
            negotiation.r_agreement.into_client(),
            Some(gen_reason("Finished")),
        )
        .await
        .unwrap();
}

/// A termination notice is informational only. The Provider may terminate
/// immediately, before the announced deadline and with an arbitrary reason
/// code.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_provider_termination_is_not_blocked_by_notice() {
    let network = MarketsNetwork::new(None, MockNet::new())
        .await
        .add_market_instance(REQ_NAME)
        .await
        .add_market_instance(PROV_NAME)
        .await;

    let prov_id = network.get_default_id(PROV_NAME);
    let prov_market = network.get_market(PROV_NAME);

    let negotiation = negotiate_agreement(
        &network,
        REQ_NAME,
        PROV_NAME,
        "negotiation",
        "r-session",
        "p-session",
    )
    .await
    .unwrap();

    prov_market
        .provider_engine
        .post_termination_notice(
            prov_id.clone(),
            negotiation.p_agreement.into_client(),
            AgreementTerminationNotice {
                termination_deadline: Utc::now() + Duration::hours(24),
                reason: Some(gen_reason("Provider shutdown")),
            },
            2.0,
        )
        .await
        .unwrap();

    let mut reason = gen_reason("Provider changed plans");
    reason.extra = serde_json::json!({ "golem.provider.code": "AnyCustomCode" });

    prov_market
        .terminate_agreement(prov_id, negotiation.p_agreement.into_client(), Some(reason))
        .await
        .unwrap();
}

/// Only one notice may exist per Agreement: a repeated notice with the same
/// payload is acknowledged idempotently, a different payload is rejected.
#[cfg_attr(not(feature = "test-suite"), ignore)]
#[serial_test::serial]
async fn test_termination_notice_immutable() {
    let network = MarketsNetwork::new(None, MockNet::new())
        .await
        .add_market_instance(REQ_NAME)
        .await
        .add_market_instance(PROV_NAME)
        .await;

    let req_id = network.get_default_id(REQ_NAME);
    let prov_id = network.get_default_id(PROV_NAME);
    let prov_market = network.get_market(PROV_NAME);

    let negotiation = negotiate_agreement(
        &network,
        REQ_NAME,
        PROV_NAME,
        "negotiation",
        "r-session",
        "p-session",
    )
    .await
    .unwrap();

    let notice = AgreementTerminationNotice {
        termination_deadline: Utc::now() + Duration::minutes(5),
        reason: Some(gen_reason("Provider shutdown")),
    };

    // Deadline in the past is rejected before anything is sent.
    match prov_market
        .provider_engine
        .post_termination_notice(
            prov_id.clone(),
            negotiation.p_agreement.into_client(),
            AgreementTerminationNotice {
                termination_deadline: Utc::now() - Duration::minutes(5),
                reason: None,
            },
            2.0,
        )
        .await
    {
        Err(PostTerminationNoticeError::DeadlineNotInFuture(..)) => (),
        e => panic!("Expected DeadlineNotInFuture error, got: {:?}", e),
    };

    // The Requestor side of the Agreement can't post a notice.
    match network
        .get_market(REQ_NAME)
        .provider_engine
        .post_termination_notice(
            req_id.clone(),
            negotiation.r_agreement.into_client(),
            notice.clone(),
            2.0,
        )
        .await
    {
        Err(PostTerminationNoticeError::NotProvider(..)) => (),
        e => panic!("Expected NotProvider error, got: {:?}", e),
    };

    // Unknown Agreement id.
    match prov_market
        .provider_engine
        .post_termination_notice(
            prov_id.clone(),
            "unknown-agreement-id".to_string(),
            notice.clone(),
            2.0,
        )
        .await
    {
        Err(
            PostTerminationNoticeError::NotFound(..) | PostTerminationNoticeError::InvalidId(..),
        ) => {}
        e => panic!("Expected NotFound error, got: {:?}", e),
    };

    prov_market
        .provider_engine
        .post_termination_notice(
            prov_id.clone(),
            negotiation.p_agreement.into_client(),
            notice.clone(),
            2.0,
        )
        .await
        .unwrap();

    // Retrying with the same payload is acknowledged again.
    prov_market
        .provider_engine
        .post_termination_notice(
            prov_id.clone(),
            negotiation.p_agreement.into_client(),
            notice.clone(),
            2.0,
        )
        .await
        .unwrap();

    // A different payload conflicts with the recorded notice.
    match prov_market
        .provider_engine
        .post_termination_notice(
            prov_id.clone(),
            negotiation.p_agreement.into_client(),
            AgreementTerminationNotice {
                termination_deadline: notice.termination_deadline + Duration::minutes(10),
                reason: notice.reason.clone(),
            },
            2.0,
        )
        .await
    {
        Err(PostTerminationNoticeError::Conflict(..)) => (),
        e => panic!("Expected Conflict error, got: {:?}", e),
    };
}
