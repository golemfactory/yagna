use crate::diesel::ExpressionMethods;
use crate::error::DbError;
use crate::error::DbResult;
use crate::models::cycle::{
    create_batch_cycle_based_on_cron, create_batch_cycle_based_on_interval, parse_cron_str,
    DbPayBatchCycle,
};
use crate::schema::pay_batch_cycle::dsl;
use chrono::{DateTime, Duration, Utc};
use diesel::{self, BoolExpressionMethods, OptionalExtension, QueryDsl, RunQueryDsl};
use lazy_static::lazy_static;
use std::env;
use ya_client_model::NodeId;
use ya_persistence::executor::{do_with_transaction, AsDao, ConnType, PoolType};
use ya_persistence::types::AdaptTimestamp;

pub struct BatchCycleDao<'c> {
    pool: &'c PoolType,
}

impl<'c> AsDao<'c> for BatchCycleDao<'c> {
    fn as_dao(pool: &'c PoolType) -> Self {
        Self { pool }
    }
}

fn get_default_payment_cycle_interval() -> chrono::Duration {
    Duration::from_std(
        humantime::parse_duration(
            &env::var("PAYMENT_CYCLE_DEFAULT_INTERVAL").unwrap_or("24h".to_string()),
        )
        .expect("Failed to parse PAYMENT_CYCLE_DEFAULT_INTERVAL"),
    )
    .expect("Failed to convert PAYMENT_CYCLE_DEFAULT_INTERVAL to chrono::Duration")
}

fn get_default_payment_cycle_extra_pay_time() -> chrono::Duration {
    Duration::from_std(
        humantime::parse_duration(
            &env::var("PAYMENT_CYCLE_DEFAULT_EXTRA_PAY_TIME").unwrap_or("1h".to_string()),
        )
        .expect("Failed to parse PAYMENT_CYCLE_DEFAULT_EXTRA_PAY_TIME"),
    )
    .expect("Failed to convert PAYMENT_CYCLE_DEFAULT_EXTRA_PAY_TIME to chrono::Duration")
}

lazy_static! {
    pub static ref PAYMENT_CYCLE_DEFAULT_INTERVAL: Duration = get_default_payment_cycle_interval();
    pub static ref PAYMENT_CYCLE_DEFAULT_EXTRA_PAY_TIME: Duration =
        get_default_payment_cycle_extra_pay_time();
}

fn get_or_insert_default_entry_private(
    conn: &ConnType,
    node_id: NodeId,
    platform: String,
) -> DbResult<DbPayBatchCycle> {
    let mut loop_count = 0;
    loop {
        let existing_entry: Option<DbPayBatchCycle> = dsl::pay_batch_cycle
            .filter(
                dsl::owner_id
                    .eq(node_id.to_string())
                    .and(dsl::platform.eq(platform.clone())),
            )
            .first(conn)
            .optional()?;

        if let Some(entry) = existing_entry {
            break Ok(entry);
        } else {
            let batch_cycle = create_batch_cycle_based_on_interval(
                node_id,
                platform.clone(),
                *PAYMENT_CYCLE_DEFAULT_INTERVAL,
                *PAYMENT_CYCLE_DEFAULT_EXTRA_PAY_TIME,
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
}

impl<'c> BatchCycleDao<'c> {
    pub async fn get_or_insert_default(
        &self,
        node_id: NodeId,
        platform: String,
    ) -> DbResult<DbPayBatchCycle> {
        do_with_transaction(
            self.pool,
            "pay_batch_cycle_get_or_insert_default",
            move |conn| get_or_insert_default_entry_private(conn, node_id, platform),
        )
        .await
    }

    pub async fn mark_process_and_next(
        &self,
        node_id: NodeId,
        platform: String,
    ) -> DbResult<DbPayBatchCycle> {
        do_with_transaction(self.pool, "pay_batch_cycle_update", move |conn| {
            let mut entry = get_or_insert_default_entry_private(conn, node_id, platform.clone())?;

            let now = Utc::now();
            let cycle_last_process = Some(now.adapt());
            if let Some(cycle_interval) = entry.cycle_interval.clone() {
                entry.cycle_next_process = (now + cycle_interval.0).adapt();
            } else if let Some(cycle_cron) = &entry.cycle_cron {
                let schedule = parse_cron_str(cycle_cron).map_err(|err| {
                    DbError::Query(format!(
                        "Failed to parse cron expression: {} {}",
                        cycle_cron, err
                    ))
                })?;

                entry.cycle_next_process = schedule
                    .upcoming(Utc)
                    .next()
                    .ok_or(DbError::Query(format!(
                        "Failed to get next running time for cron expression: {}",
                        cycle_cron
                    )))?
                    .adapt();
            }
            diesel::update(
                dsl::pay_batch_cycle.filter(
                    dsl::owner_id
                        .eq(node_id.to_string())
                        .and(dsl::platform.eq(platform.clone())),
                ),
            )
            .set((
                dsl::cycle_last_process.eq(cycle_last_process),
                dsl::cycle_next_process.eq(entry.cycle_next_process.clone()),
            ))
            .execute(conn)?;
            Ok(entry)
        })
        .await
    }

    pub async fn create_or_update(
        &self,
        owner_id: NodeId,
        platform: String,
        interval: Option<Duration>,
        cron: Option<String>,
        safe_payout: Option<Duration>,
        next_running_time: Option<DateTime<Utc>>,
    ) -> DbResult<DbPayBatchCycle> {
        let now = Utc::now().adapt();
        let cycle = if let Some(interval) = interval {
            match create_batch_cycle_based_on_interval(
                owner_id,
                platform.clone(),
                interval,
                safe_payout.unwrap_or(*PAYMENT_CYCLE_DEFAULT_EXTRA_PAY_TIME),
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
            match create_batch_cycle_based_on_cron(
                owner_id,
                platform.clone(),
                &cron,
                safe_payout.unwrap_or(*PAYMENT_CYCLE_DEFAULT_EXTRA_PAY_TIME),
            ) {
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
                .filter(
                    dsl::owner_id
                        .eq(owner_id.to_string())
                        .and(dsl::platform.eq(platform.clone())),
                )
                .first(conn)
                .optional()?;
            if let Some(mut entry) = existing_entry {
                entry.cycle_interval = cycle.cycle_interval.clone();
                entry.cycle_cron = cycle.cycle_cron;
                entry.cycle_next_process = cycle.cycle_next_process;
                entry.cycle_max_interval = cycle.cycle_max_interval;
                entry.cycle_extra_pay_time = cycle.cycle_extra_pay_time;

                if let Some(cycle_last_process) = entry.cycle_last_process.clone() {
                    if let Some(interval) = cycle.cycle_interval {
                        let max_next_running_time = cycle_last_process.0 + interval.0;
                        if let Some(next_running_time) = next_running_time {
                            entry.cycle_next_process =
                                std::cmp::min(next_running_time, max_next_running_time.and_utc())
                                    .adapt();
                        } else {
                            entry.cycle_next_process = max_next_running_time.adapt();
                        }
                    }
                }
                log::info!("Updating batch cycle {:?}", entry);
                diesel::update(
                    dsl::pay_batch_cycle.filter(
                        dsl::owner_id
                            .eq(owner_id.to_string())
                            .and(dsl::platform.eq(platform.clone())),
                    ),
                )
                .set((
                    dsl::updated_ts.eq(now),
                    dsl::cycle_interval.eq(entry.cycle_interval),
                    dsl::cycle_cron.eq(entry.cycle_cron),
                    dsl::cycle_last_process.eq(entry.cycle_last_process),
                    dsl::cycle_next_process.eq(entry.cycle_next_process),
                    dsl::cycle_max_interval.eq(entry.cycle_max_interval),
                    dsl::cycle_extra_pay_time.eq(entry.cycle_extra_pay_time),
                ))
                .execute(conn)?;
            } else {
                diesel::insert_into(dsl::pay_batch_cycle)
                    .values(&cycle)
                    .execute(conn)?;
            };
            let existing_entry: DbPayBatchCycle = dsl::pay_batch_cycle
                .filter(
                    dsl::owner_id
                        .eq(owner_id.to_string())
                        .and(dsl::platform.eq(platform.clone())),
                )
                .first(conn)?;
            Ok(existing_entry)
        })
        .await
    }
}
