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
    increaseAgreementAmountDue, getAgreement, increaseAgreementAmountAccepted
} from './aggreement.js';
import {
    createActivity,
    increaseActivityAndAgreementAmountAccepted,
    increaseActivityAndAgreementAmountDue,
    insertActivity
} from "./activity.js";
import {createDebitNote, getLastDebitNote, insertDebitNote} from "./debit.js";
import {createInvoice, insertInvoice} from "./invoice.js";


let owner = "0xed16665465c8f9bf680edb8b2cd5a7575ef8da2e"
let peer = '0x141bcf190037140c5e589ad38e303c2626d48886';
let pay_platform = 'erc20-holesky-tglm';
let app_session_id = uuidv4();
let created_date = formatDatePaymentsFormat(new Date());
let agreement = createAgreement(owner, peer, pay_platform, app_session_id, created_date);
insertAgreement(db, agreement);

let activity1 = createActivity(agreement.id, owner, 'R', created_date);
insertActivity(db, activity1);
let activity2 = createActivity(agreement.id, owner, 'R', created_date);
insertActivity(db, activity2);





function finishAgreement(amount_due, agreement_id) {
    db.transaction((amount_due, agreement_id) => {
        let bigAmountDue = BigNumber(amount_due);
        let agreement = getAgreement(db, agreement_id);
        let diff = bigAmountDue.minus(agreement.total_amount_accepted);
        if (diff.isNegative()) {
            throw new Error("Amount due is smaller than total amount due");
        }
        if (diff.isZero()) {
            console.log("OK - Amount due is equal to total amount due");
        } else {
            console.log("Increasing agreement by: ", diff.toString());
            increaseAgreementAmountAccepted(db, agreement_id, diff);
        }
        insertInvoice(db, createInvoice([activity1, activity2], 'ACCEPTED', amount_due));
        
    })(amount_due, agreement_id);
}

function debitNoteIncoming(amount_due, activity) {
    db.transaction((amount_due, activity) => {
        let bigAmountDue = BigNumber(amount_due);
        let lastDebitNote = getLastDebitNote(db, activity);
        if (lastDebitNote) {
            console.log("lastDebitNote amount", BigNumber(lastDebitNote.total_amount_due).toString());
        }
        let diff = lastDebitNote ? bigAmountDue.minus(lastDebitNote.total_amount_due) : bigAmountDue;
        console.log("diff", diff.toString());

        let lastDebitNoteId = lastDebitNote ? lastDebitNote.id : null;
        let debit_nonce = lastDebitNote ? lastDebitNote.debit_nonce + 1 : 0;
        let debitNote = createDebitNote(activity, lastDebitNoteId, debit_nonce, 'ACCEPTED', amount_due);
        insertDebitNote(db, debitNote);
        increaseActivityAndAgreementAmountAccepted(db, activity, diff);
    })(amount_due, activity);
}


debitNoteIncoming("5.44", activity1);
debitNoteIncoming("6.436666666666666666666666666666", activity1);
debitNoteIncoming("7.0", activity1);

debitNoteIncoming("15.44", activity2);
debitNoteIncoming("16.436666666666666666666666666666", activity2);
debitNoteIncoming("17.0", activity2);

//increaseAgreementAmountAccepted(db, agreement, 1);

finishAgreement("25.0", agreement.id);

db.close();
