import Database from 'better-sqlite3';
import { v4 as uuidv4 } from 'uuid';
import crypto from "crypto";
import BigNumber from "bignumber.js";
const yagna_dir = process.env.YAGNA_DIR || '../../yagnadir'
const payments_sql_file = yagna_dir + '/payment.db'
const db = new Database(payments_sql_file);
import {formatDatePaymentsFormat} from './utils.js';
import {
    createAgreement,
    insertAgreement,
    increaseAgreementAmountDue
} from './aggreement.js';
import {
    createActivity,
    increaseActivityAndAgreementAmountAccepted,
    increaseActivityAndAgreementAmountDue,
    insertActivity
} from "./activity.js";
import {createDebitNote, getLastDebitNote, insertDebitNote} from "./debit.js";
import {ethers} from "ethers";


    let owner = "0xed16665465c8f9bf680edb8b2cd5a7575ef8da2e"
    let peer = '0x141bcf190037140c5e589ad38e303c2626d48886';
    let pay_platform = 'erc20-holesky-tglm';
    let app_session_id = uuidv4();
    let created_date = formatDatePaymentsFormat(new Date());
    let agreement = createAgreement(owner, peer, pay_platform, app_session_id, created_date);
    insertAgreement(db, agreement);

    let activity1 = createActivity(agreement.id, owner, 'R');
    insertActivity(db, activity1);
    let activity2 = createActivity(agreement.id, owner, 'R');
    insertActivity(db, activity2);



    increaseActivityAndAgreementAmountAccepted(db, activity1, 100);

    const insertDebitNoteTransaction = db.transaction((amount_due, activity) => {
        let bigAmountDue = BigNumber(amount_due);
        let lastDebitNote = getLastDebitNote(db, activity);
        if (lastDebitNote) {
            console.log("lastDebitNote amount", BigNumber(lastDebitNote.total_amount_due).toString());
        }
        let diff = lastDebitNote ? bigAmountDue.minus(lastDebitNote.total_amount_due) : bigAmountDue;
        console.log("diff", diff.toString());


        let lastDebitNoteId = lastDebitNote ? lastDebitNote.id : null;
        let debitNote = createDebitNote(activity, lastDebitNoteId, 'RECEIVED', amount_due);
        insertDebitNote(db, debitNote);
        increaseActivityAndAgreementAmountDue(db, activity, diff);
    });
    insertDebitNoteTransaction("5.44", activity1);
    insertDebitNoteTransaction("6.436666666666666666666666666666", activity1);
    insertDebitNoteTransaction("7.0", activity1);

    insertDebitNoteTransaction("15.44", activity2);
    insertDebitNoteTransaction("16.436666666666666666666666666666", activity2);
    insertDebitNoteTransaction("17.0", activity2);

    /*
    const stmt = db.prepare("INSERT INTO lorem VALUES (?)");
    for (let i = 0; i < 10; i++) {
        stmt.run("Ipsum " + i);
    }
    stmt.finalize();

    db.each("SELECT rowid AS id, info FROM lorem", (err, row) => {
        console.log(row.id + ": " + row.info);
    });*/

db.close();