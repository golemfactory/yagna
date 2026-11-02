use bigdecimal::BigDecimal;
use diesel::RunQueryDsl;
use ya_client_model::payment::{ActivityPayment, AgreementPayment, Payment};
use ya_client_model::NodeId;
use ya_core_model::signable::Signable;
use ya_payment::dao::PaymentDao;
use ya_payment::error::DbError;
use ya_payment::models::{activity, agreement};
use ya_payment::schema::{pay_activity, pay_agreement};
use ya_persistence::executor::{do_with_transaction, DbExecutor};
use ya_persistence::types::Role;

const PAYMENT_ID: &str = "shared-driver-payment";
const WALLET: &str = "0x9000000000000000000000000000000000000000";
const CONFIRMATION: &[u8] = b"shared-transfer-confirmation";

fn owners() -> [NodeId; 2] {
    [
        "0x1000000000000000000000000000000000000000"
            .parse()
            .unwrap(),
        "0x2000000000000000000000000000000000000000"
            .parse()
            .unwrap(),
    ]
}

fn peers() -> [NodeId; 4] {
    [3, 4, 5, 6].map(|n| format!("0x{n:040x}").parse().unwrap())
}

fn test_db() -> DbExecutor {
    let db = DbExecutor::in_memory(&format!("payment-peers-{}", uuid::Uuid::new_v4())).unwrap();
    db.apply_migration(ya_payment::migrations::MIGRATIONS)
        .unwrap();
    db
}

async fn create_payment(
    db: &DbExecutor,
    payment_id: &str,
    owner: NodeId,
    peer: NodeId,
    wallet: &str,
) {
    let agreement_id = format!("agreement-{owner}-{peer}");
    let activity_id = format!("activity-{owner}-{peer}");
    let agreement = agreement::WriteObj {
        id: agreement_id.clone(),
        owner_id: owner,
        peer_id: peer,
        role: Role::Requestor,
        payee_addr: wallet.to_string(),
        payer_addr: owner.to_string(),
        payment_platform: "erc20-test-tglm".to_string(),
        total_amount_due: Default::default(),
        total_amount_accepted: Default::default(),
        total_amount_scheduled: Default::default(),
        total_amount_paid: Default::default(),
        app_session_id: Some(format!("session-{peer}")),
        created_ts: None,
        updated_ts: None,
    };
    let activity = activity::WriteObj::new(
        activity_id.clone(),
        owner,
        Role::Requestor,
        agreement_id.clone(),
    );
    do_with_transaction(&db.pool, "seed_payment_peer", move |conn| {
        diesel::insert_into(pay_agreement::table)
            .values(agreement)
            .execute(conn)?;
        diesel::insert_into(pay_activity::table)
            .values(activity)
            .execute(conn)?;
        Ok::<_, DbError>(())
    })
    .await
    .unwrap();

    db.as_dao::<PaymentDao>()
        .create_new(
            payment_id.to_string(),
            owner,
            peer,
            owner.to_string(),
            wallet.to_string(),
            "erc20-test-tglm".to_string(),
            BigDecimal::from(3),
            CONFIRMATION.to_vec(),
            vec![ActivityPayment {
                activity_id,
                amount: BigDecimal::from(1),
                allocation_id: None,
            }],
            vec![AgreementPayment {
                agreement_id,
                amount: BigDecimal::from(2),
                allocation_id: None,
            }],
        )
        .await
        .unwrap();
}

async fn shared_payment_db() -> DbExecutor {
    let db = test_db();
    for owner in owners() {
        for peer in peers() {
            create_payment(&db, PAYMENT_ID, owner, peer, WALLET).await;
        }
    }
    db
}

fn assert_peer_documents(payment: &Payment, owner: NodeId, peer: NodeId) {
    assert_eq!(payment.payer_id, owner);
    assert_eq!(payment.payee_id, peer);
    assert_eq!(payment.amount, BigDecimal::from(3));
    assert_eq!(payment.activity_payments.len(), 1);
    assert_eq!(payment.agreement_payments.len(), 1);
    assert_eq!(
        payment.activity_payments[0].activity_id,
        format!("activity-{owner}-{peer}")
    );
    assert_eq!(
        payment.agreement_payments[0].agreement_id,
        format!("agreement-{owner}-{peer}")
    );
    assert_eq!(
        &payment.activity_payments[0].amount + &payment.agreement_payments[0].amount,
        payment.amount
    );
}

#[actix_rt::test]
async fn shared_payment_lookup_keeps_documents_with_the_selected_peer() {
    let db = shared_payment_db().await;
    let dao: PaymentDao = db.as_dao();
    for owner in owners() {
        for peer in peers() {
            let payment = dao
                .get_for_peer(PAYMENT_ID.to_string(), owner, peer)
                .await
                .unwrap()
                .unwrap();
            assert_peer_documents(&payment.payload, owner, peer);
        }

        // The existing REST lookup lacks a peer parameter, but must still return
        // documents belonging only to the selected payment row.
        let payment = dao
            .get(PAYMENT_ID.to_string(), owner)
            .await
            .unwrap()
            .unwrap();
        assert_peer_documents(&payment.payload, owner, payment.payload.payee_id);
        assert!(dao
            .get_for_peer(PAYMENT_ID.to_string(), owner, owner)
            .await
            .unwrap()
            .is_none());
    }

    let payments = dao
        .get_for_confirmation(CONFIRMATION.to_vec(), Role::Requestor)
        .await
        .unwrap();
    assert_eq!(payments.len(), 8);
    for payment in payments {
        assert_peer_documents(&payment, payment.payer_id, payment.payee_id);
    }
}

#[actix_rt::test]
async fn shared_payment_listing_preserves_each_peers_documents_and_session() {
    let db = shared_payment_db().await;
    let dao: PaymentDao = db.as_dao();
    for owner in owners() {
        let payments = dao
            .get_for_node_id(owner, None, None, None, None, None)
            .await
            .unwrap();
        assert_eq!(payments.len(), 4);
        for peer in peers() {
            let payment = payments
                .iter()
                .find(|p| p.payload.payee_id == peer)
                .unwrap();
            assert_peer_documents(&payment.payload, owner, peer);

            let filtered = dao
                .get_for_node_id(
                    owner,
                    None,
                    None,
                    Some(format!("session-{peer}")),
                    None,
                    None,
                )
                .await
                .unwrap();
            assert_eq!(filtered.len(), 1);
            assert_peer_documents(&filtered[0].payload, owner, peer);
        }
    }
}

#[actix_rt::test]
async fn shared_payment_signatures_and_retries_are_isolated_by_owner_and_peer() {
    let db = shared_payment_db().await;
    let dao: PaymentDao = db.as_dao();
    let owner = owners()[0];
    let peer = peers()[0];
    let payment = dao
        .get_for_peer(PAYMENT_ID.to_string(), owner, peer)
        .await
        .unwrap()
        .unwrap();
    let signed_bytes = payment.payload.canonicalize().unwrap();
    dao.add_signature(
        PAYMENT_ID.to_string(),
        owner,
        peer,
        vec![1, 2, 3],
        signed_bytes.clone(),
    )
    .await
    .unwrap();
    dao.mark_sent(PAYMENT_ID.to_string(), owner, peer)
        .await
        .unwrap();

    for other_owner in owners() {
        let payments = dao
            .get_for_node_id(other_owner, None, None, None, None, None)
            .await
            .unwrap();
        for payment in payments {
            assert_peer_documents(&payment.payload, other_owner, payment.payload.payee_id);
            if other_owner == owner && payment.payload.payee_id == peer {
                let signature = payment.signature.unwrap();
                assert_eq!(signature.signature, vec![1, 2, 3]);
                assert_eq!(signature.signed_bytes, signed_bytes);
                payment.payload.verify_canonical(&signed_bytes).unwrap();
            } else {
                assert!(payment.signature.is_none());
            }
        }

        let unsent = dao.list_unsent(other_owner, None).await.unwrap();
        assert_eq!(unsent.len(), if other_owner == owner { 3 } else { 4 });
        for payment in unsent {
            assert_peer_documents(&payment, other_owner, payment.payee_id);
        }
        for other_peer in peers() {
            let unsent = dao
                .list_unsent(other_owner, Some(other_peer))
                .await
                .unwrap();
            if other_owner == owner && other_peer == peer {
                assert!(unsent.is_empty());
            } else {
                assert_eq!(unsent.len(), 1);
                assert_peer_documents(&unsent[0], other_owner, other_peer);
            }
        }
    }
}

#[actix_rt::test]
async fn single_peer_and_separate_wallet_payments_keep_their_documents() {
    let db = test_db();
    let owner = owners()[0];
    let dao: PaymentDao = db.as_dao();
    for (index, peer) in peers().iter().copied().enumerate() {
        let payment_id = format!("separate-payment-{index}");
        create_payment(&db, &payment_id, owner, peer, &peer.to_string()).await;
        let payment = dao
            .get_for_peer(payment_id, owner, peer)
            .await
            .unwrap()
            .unwrap();
        assert_peer_documents(&payment.payload, owner, peer);
        assert_eq!(payment.payload.payee_addr, peer.to_string());

        let payments = dao
            .get_for_node_id(owner, None, None, None, None, None)
            .await
            .unwrap();
        assert_eq!(payments.len(), index + 1);
        for payment in payments {
            assert_peer_documents(&payment.payload, owner, payment.payload.payee_id);
        }
    }
}
