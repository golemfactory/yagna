import sqlite3 from 'sqlite3';
import { v4 as uuidv4 } from 'uuid';
import crypto from "crypto";

const yagna_dir = process.env.YAGNA_DIR || '../../yagnadir'
const payments_sql_file = yagna_dir + '/payment.db'
const db = new sqlite3.Database(payments_sql_file);
import {formatDatePaymentsFormat} from './utils.js';

function insertAggreement(agreement) {
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
    console.log(query);
    db.run(query);
}

db.serialize(() => {

    let owner = "0xed16665465c8f9bf680edb8b2cd5a7575ef8da2e"
    let peer = '0x141bcf190037140c5e589ad38e303c2626d48886';
    let pay_platform = 'erc20-holesky-tglm';
    let app_session_id = uuidv4();
    let agreement_id = crypto.randomBytes(32).toString("hex");
    let created_date = formatDatePaymentsFormat(new Date());
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
    insertAggreement(agreement);


    /*
    const stmt = db.prepare("INSERT INTO lorem VALUES (?)");
    for (let i = 0; i < 10; i++) {
        stmt.run("Ipsum " + i);
    }
    stmt.finalize();

    db.each("SELECT rowid AS id, info FROM lorem", (err, row) => {
        console.log(row.id + ": " + row.info);
    });*/
});

db.close();