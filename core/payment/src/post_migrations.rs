use crate::error::DbResult;
use crate::timeout_lock::MutexTimeoutExt;

use bigdecimal::{BigDecimal, Zero};
use diesel::sql_types::Text;
use diesel::RunQueryDsl;
use std::str::FromStr;
use std::sync::Arc;

use crate::processor::DB_LOCK_TIMEOUT;
use ya_persistence::executor::{do_with_transaction, DbExecutor};

pub async fn process_post_migration_jobs(
    db_executor: Arc<tokio::sync::Mutex<DbExecutor>>,
) -> DbResult<()> {
    let db_executor = db_executor
        .timeout_lock(DB_LOCK_TIMEOUT)
        .await
        .expect("db lock timeout");

    /*
    -- we have to run this query but by hand because of lack of decimal support:
    UPDATE pay_agreement
    SET total_amount_paid = cast(total_amount_paid + (SELECT sum(total_amount_paid)
        FROM pay_activity s
        WHERE s.owner_id = pay_agreement.owner_id
        AND s.role = pay_agreement.role
        AND s.agreement_id = pay_agreement.id) AS VARCHAR)
    WHERE EXISTS (SELECT 1 FROM pay_activity s2 WHERE s2.owner_id = pay_agreement.owner_id
        AND s2.role = pay_agreement.role
        AND s2.agreement_id = pay_agreement.id);
    */

    #[derive(QueryableByName, PartialEq, Debug)]
    struct JobRecord {
        #[sql_type = "Text"]
        job: String,
    }

    #[derive(QueryableByName, PartialEq, Debug)]
    struct AgreementActivityRecord {
        #[sql_type = "Text"]
        agreement_id: String,
        #[sql_type = "Text"]
        owner_id: String,
        #[sql_type = "Text"]
        role: String,
        #[sql_type = "Text"]
        total_amount_paid_agreement: String,
        #[sql_type = "Text"]
        total_amount_paid_activity: String,
    }

    do_with_transaction(&db_executor.pool, "run_post_migration", move |conn| {
        const JOB_NAME: &str = "sum_activities_into_agreement";
        let job_records = diesel::sql_query(
            r#"
                SELECT job FROM pay_post_migration WHERE done IS NULL AND job = ?
            "#,
        )
        .bind::<Text, _>(JOB_NAME)
        .load::<JobRecord>(conn)?;
        let job_record = job_records.first();

        if let Some(job_record) = job_record {
            log::info!("Running post migration job: sum_activities_into_agreement");

            let records: Vec<AgreementActivityRecord> = diesel::sql_query(
                r#"
                    SELECT pag.id AS agreement_id,
                          pag.owner_id AS owner_id,
                          pag.role AS role,
                          pag.total_amount_paid AS total_amount_paid_agreement,
                          pac.total_amount_paid AS total_amount_paid_activity
                    FROM pay_agreement AS pag
                    JOIN pay_activity AS pac
                        ON pac.agreement_id = pag.id
                            AND pac.owner_id = pag.owner_id
                            AND pac.role = pag.role
                    ORDER BY agreement_id
                "#,
            )
            .load(conn)?;

            let mut current_idx: usize = 0;
            if let Some(first_record) = records.get(current_idx) {
                let mut current_sum: BigDecimal = Zero::zero();
                let mut current_agreement_id = first_record.agreement_id.clone();

                while current_idx < records.len() {
                    let record = &records
                        .get(current_idx)
                        .expect("record has to be found on index");

                    current_sum +=
                        BigDecimal::from_str(&records[current_idx].total_amount_paid_activity)
                            .unwrap_or_default();

                    let write_total_sum = records
                        .get(current_idx + 1)
                        .map(|rec| rec.agreement_id != current_agreement_id.as_str())
                        .unwrap_or(true);
                    if write_total_sum {
                        current_sum += BigDecimal::from_str(&record.total_amount_paid_agreement)
                            .unwrap_or_default();

                        diesel::sql_query(
                            r#"
                                UPDATE pay_agreement
                                SET total_amount_paid = $1
                                WHERE id = $2
                                    AND owner_id = $3
                                    AND role = $4
                            "#,
                        )
                        .bind::<Text, _>(current_sum.to_string())
                        .bind::<Text, _>(current_agreement_id)
                        .bind::<Text, _>(&record.owner_id)
                        .bind::<Text, _>(&record.role)
                        .execute(conn)?;
                        current_sum = Zero::zero();
                        current_agreement_id = records
                            .get(current_idx + 1)
                            .map(|rec| rec.agreement_id.clone())
                            .unwrap_or_default();
                    }
                    current_idx += 1;
                }
            }

            log::info!("Post migration job: sum_activities_into_agreement done. Marking as done.");
            let marked = diesel::sql_query(
                r#"
                        UPDATE pay_post_migration
                        SET done = STRFTIME('%Y-%m-%d %H:%M:%f', 'NOW'),
                            result = 'ok'
                        WHERE job = ?
                    "#,
            )
            .bind::<Text, _>(JOB_NAME)
            .execute(conn)?;
            if marked != 1 {
                log::error!("Post migration job: sum_activities_into_agreement not marked as done");
            }
        } else {
            log::info!("No post migration jobs to run");
        }
        Ok(())
    })
    .await
}
