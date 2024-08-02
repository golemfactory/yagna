import {formatDatePaymentsFormat} from "./utils.js";
import { v4 as uuidv4 } from 'uuid';

export function createInvoice(activities, status, amount) {
    //date + 30 days
    let due_date = new Date();
    due_date.setUTCDate(due_date.getUTCDate() + 30);

    let owner_id = activities[0].owner_id;
    let agreement_id = activities[0].agreement_id;
    let role = activities[0].role;

    let invoice = {
        id: uuidv4(),
        owner_id: owner_id,
        role: role,
        agreement_id: agreement_id,
        status: status,
        timestamp: formatDatePaymentsFormat(new Date()),
        amount: amount,
        payment_due_date: formatDatePaymentsFormat(due_date),
        invoice_ids: activities.map(a => a.id),
        send_accept: 0,
        send_reject: 0,
    }
    return invoice;
}

export function insertInvoice(db, invoice) {
    let query = `INSERT INTO pay_invoice (
        id,
        owner_id,
        role,
        agreement_id,
        status,
        timestamp,
        amount,
        payment_due_date,
        send_accept,
        send_reject
          )
    VALUES (
        '${invoice.id}',
        '${invoice.owner_id}',
        '${invoice.role}',
        '${invoice.agreement_id}',
        '${invoice.status}',
        '${invoice.timestamp}',
        '${invoice.amount}',
        '${invoice.payment_due_date}',
        '${invoice.send_accept}',
        '${invoice.send_reject}'
    )`;
    //console.log(query);
    db.prepare(query).run();

    invoice.invoice_ids.forEach(activity_id => {
        let query = `INSERT INTO pay_invoice_x_activity (invoice_id, activity_id, owner_id)
           VALUES ('${invoice.id}', '${activity_id}', '${invoice.owner_id}')`;
        db.prepare(query).run();
    });
}