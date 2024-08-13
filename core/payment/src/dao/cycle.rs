use crate::diesel::ExpressionMethods;
use crate::error::DbError;
use crate::error::DbResult;
use crate::models::cycle::{
    create_batch_cycle_based_on_cron, create_batch_cycle_based_on_interval, DbPayBatchCycle,
};
use crate::schema::pay_batch_cycle::dsl;
use chrono::{DateTime, Duration, Utc};
use diesel::{self, OptionalExtension, QueryDsl, RunQueryDsl};
use ya_client_model::NodeId;
use ya_persistence::executor::{do_with_transaction, AsDao, PoolType};
use ya_persistence::types::AdaptTimestamp;

pub struct BatchCycleDao<'c> {
    pool: &'c PoolType,
}

impl<'c> AsDao<'c> for BatchCycleDao<'c> {
    fn as_dao(pool: &'c PoolType) -> Self {
        Self { pool }
    }
}

const DEFAULT_INTERVAL: Duration = Duration::minutes(5);
const DEFAULT_EXTRA_TIME_FOR_PAYMENT: Duration = Duration::minutes(4);

impl<'c> BatchCycleDao<'c> {
    pub async fn get_or_insert_default(&self, node_id: NodeId) -> DbResult<DbPayBatchCycle> {
        do_with_transaction(
            self.pool,
            "pay_batch_cycle_get_or_insert_default",
            move |conn| {
                let mut loop_count = 0;
                loop {
                    let existing_entry: Option<DbPayBatchCycle> = dsl::pay_batch_cycle
                        .filter(dsl::owner_id.eq(node_id.to_string()))
                        .first(conn)
                        .optional()?;

                    if let Some(entry) = existing_entry {
                        break Ok(entry);
                    } else {
                        let batch_cycle = create_batch_cycle_based_on_interval(
                            node_id,
                            DEFAULT_INTERVAL,
                            DEFAULT_EXTRA_TIME_FOR_PAYMENT,
                        )
                        .expect("Failed to create default batch cycle");
                        diesel::insert_into(dsl::pay_batch_cycle)
                            .values(batch_cycle)
                            .execute(conn)?;
                    }
                    loop_count += 1;
                    if loop_count > 1 {
                        return Err(DbError::Query(
                            "Failed to insert default batch cycle".to_string(),
                        ));
                    }
                }
            },
        )
        .await
    }

    pub async fn create_or_update(
        &self,
        owner_id: NodeId,
        interval: Option<Duration>,
        cron: Option<String>,
        next_running_time: Option<DateTime<Utc>>,
    ) -> DbResult<DbPayBatchCycle> {
        let now = Utc::now().adapt();
        let cycle = if let Some(interval) = interval {
            match create_batch_cycle_based_on_interval(
                owner_id,
                interval,
                DEFAULT_EXTRA_TIME_FOR_PAYMENT,
            ) {
                Ok(cycle) => cycle,
                Err(err) => {
                    return Err(DbError::Query(format!(
                        "Error creating batch cycle based on interval {}",
                        err
                    )));
                }
            }
        } else if let Some(cron) = cron {
            match create_batch_cycle_based_on_cron(&owner_id, &cron, DEFAULT_EXTRA_TIME_FOR_PAYMENT)
            {
                Ok(cycle) => cycle,
                Err(err) => {
                    return Err(DbError::Query(format!(
                        "Error creating batch cycle based on cron {}",
                        err
                    )));
                }
            }
        } else {
            return Err(DbError::Query(
                "Either interval or cron must be provided".to_string(),
            ));
        };
        do_with_transaction(self.pool, "pay_batch_cycle_create", move |conn| {

            let existing_entry: Option<DbPayBatchCycle> = dsl::pay_batch_cycle
                .filter(dsl::owner_id.eq(owner_id.to_string()))
                .first(conn)
                .optional()?;
            if let Some(mut entry) = existing_entry {
                entry.cycle_interval = cycle.cycle_interval;
                entry.cycle_cron = cycle.cycle_cron;
                entry.cycle_next_process = cycle.cycle_next_process;
                entry.cycle_max_interval = cycle.cycle_max_interval;
                entry.cycle_max_pay_time = cycle.cycle_max_pay_time;

                diesel::update(dsl::pay_batch_cycle.filter(dsl::owner_id.eq(owner_id.to_string())))
                    .set((
                         dsl::updated_ts.eq(now),
                         dsl::cycle_interval.eq(entry.cycle_interval),
                         dsl::cycle_cron.eq(entry.cycle_cron),
                         dsl::cycle_last_process.eq(entry.cycle_last_process),
                         dsl::cycle_next_process.eq(entry.cycle_next_process),
                         dsl::cycle_max_interval.eq(entry.cycle_max_interval),
                         dsl::cycle_max_pay_time.eq(entry.cycle_max_pay_time),
                    ))
                    .execute(conn)?;
            } else {
                diesel::insert_into(dsl::pay_batch_cycle)
                    .values(&cycle)
                    .execute(conn)?;
            };
            let existing_entry: DbPayBatchCycle = dsl::pay_batch_cycle
                .filter(dsl::owner_id.eq(owner_id.to_string()))
                .first(conn)?;
            Ok(existing_entry)
        })
        .await
    }
}
