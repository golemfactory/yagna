



export function insertAgreement(db, agreement) {
    let query = `INSERT INTO pay_agreement (
                    id,
                    owner_id,
                    role,
                    peer_id,
                    payee_addr,
                    payer_addr,
                    payment_platform,
                    total_amount_due,
                    total_amount_accepted,
                    total_amount_scheduled,
                    total_amount_paid,
                    app_session_id,
                    created_ts,
                    updated_ts
                )
                VALUES (
                    '${agreement.id}',
                    '${agreement.owner_id}',
                    '${agreement.role}',
                    '${agreement.peer_id}',
                    '${agreement.payee_addr}',
                    '${agreement.payer_addr}',
                    '${agreement.payment_platform}',
                    '${agreement.total_amount_due}',
                    '${agreement.total_amount_accepted}',
                    '${agreement.total_amount_scheduled}',
                    '${agreement.total_amount_paid}',
                    '${agreement.app_session_id}',
                    '${agreement.created_ts}',
                    '${agreement.updated_ts}'
                )`;
    db.run(query);
}

export function createAgreement(owner, peer, pay_platform, app_session_id, agreement_id, created_date) {
    let agreement = {
        id: agreement_id,
        owner_id: owner,
        role: 'R',
        peer_id: peer,
        payee_addr: peer,
        payer_addr: owner,
        payment_platform: pay_platform,
        total_amount_due: 0,
        total_amount_accepted: 0,
        total_amount_scheduled: 0,
        total_amount_paid: 0,
        app_session_id: app_session_id,
        created_ts: created_date,
        updated_ts: created_date
    }
    return agreement;
}