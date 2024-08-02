import crypto from "crypto";
import {increaseAgreementAmountAccepted, increaseAgreementAmountDue} from "./aggreement.js";

export function insertActivity(db, activity) {
    let query = `INSERT INTO pay_activity (
        id,
        owner_id,
        role,
        agreement_id,
        total_amount_due,
        total_amount_accepted,
        total_amount_scheduled,
        total_amount_paid
          )
    VALUES (
        '${activity.id}',
        '${activity.owner_id}',
        '${activity.role}',
        '${activity.agreement_id}',
        '${activity.total_amount_due}',
        '${activity.total_amount_accepted}',
        '${activity.total_amount_scheduled}',
        '${activity.total_amount_paid}'
    )`;

    db.prepare(query).run();
}

export function createActivity(agreement_id, owner, role) {
    let activity = {
        id: crypto.randomBytes(16).toString("hex"),
        owner_id: owner,
        role: role,
        agreement_id: agreement_id,
        total_amount_due: 0,
        total_amount_accepted: 0,
        total_amount_scheduled: 0,
        total_amount_paid: 0,
    }
    return activity;
}

export function increaseActivityAndAgreementAmountDue(db, activity, amount) {
    increaseAgreementAmountDue(db, activity.agreement_id, amount);
    let query = `UPDATE pay_activity 
                SET total_amount_due = total_amount_due + ${amount}
                WHERE id = '${activity.id}'`;
    db.prepare(query).run();
}

export function increaseActivityAndAgreementAmountAccepted(db, activity, amount) {
    increaseAgreementAmountAccepted(db, activity.agreement_id, amount);
    let query = `UPDATE pay_activity 
                SET total_amount_accepted = total_amount_accepted + ${amount}
                WHERE id = '${activity.id}'`;
    db.prepare(query).run();
}
