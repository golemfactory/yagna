import {formatDatePaymentsFormat} from "./utils.js";
import crypto from "crypto";
import { v4 as uuidv4 } from 'uuid';

export function createDebitNote(activity, prev_note, debit_nonce, status, total_amount_due) {
    //date + 30 days
    let due_date = new Date();
    due_date.setUTCDate(due_date.getUTCDate() + 30);
    let debit_note = {
        id: uuidv4(),
        owner_id: activity.owner_id,
        role: activity.role,
        previous_debit_note_id: prev_note,
        activity_id: activity.id,
        status: status,
        timestamp: formatDatePaymentsFormat(new Date()),
        total_amount_due: total_amount_due,
        usage_counter_vector: "[1, 1]",
        payment_due_date: formatDatePaymentsFormat(due_date),
        send_accept: 0,
        debit_nonce: debit_nonce
    }
    return debit_note;
}

export function getLastDebitNote(db, activity) {

    let query = `SELECT * FROM pay_debit_note 
WHERE activity_id='${activity.id}' 
AND role='${activity.role}'
AND NOT EXISTS (
    SELECT * FROM pay_debit_note as pdn 
    WHERE
        pdn.role = '${activity.role}' AND 
        pdn.previous_debit_note_id=pay_debit_note.id
    )`;
    let debits = db.prepare(query).all();

    if (debits.length > 1) {
        throw new Error("More than one debit note found");
    }
    if (debits.length == 1) {
        return debits[0];
    }
    return null;
}

export function insertDebitNote(db, debitNote) {
    let prev_note = debitNote.previous_debit_note_id ? `'${debitNote.previous_debit_note_id}'`: 'NULL';
    let query = `INSERT INTO pay_debit_note (
        id,
        owner_id,
        role,
        previous_debit_note_id,
        activity_id,
        status,
        timestamp,
        total_amount_due,
        usage_counter_vector,
        payment_due_date,
        send_accept,
        debit_nonce
          )
    VALUES (
        '${debitNote.id}',
        '${debitNote.owner_id}',
        '${debitNote.role}',
        ${prev_note},
        '${debitNote.activity_id}',
        '${debitNote.status}',
        '${debitNote.timestamp}',
        '${debitNote.total_amount_due}',
        '${debitNote.usage_counter_vector}',
        '${debitNote.payment_due_date}',
        '${debitNote.send_accept}',
        ${debitNote.debit_nonce}
    )`;
    //console.log(query);
    db.prepare(query).run();
}