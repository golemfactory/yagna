use crate::error::DbResult;
use crate::models::cycle::{
    createBatchCycleBasedOnCron, createBatchCycleBasedOnInterval, DbPayBatchCycle,
};
use crate::schema::pay_batch_cycle::dsl;
use anyhow::anyhow;
use chrono::{DateTime, Duration, Utc};
use diesel::{self, OptionalExtension, QueryDsl, RunQueryDsl};
use ya_client_model::NodeId;
use ya_persistence::executor::{do_with_transaction, AsDao, PoolType};
use ya_persistence::types::{AdaptTimestamp};
use crate::diesel::ExpressionMethods;

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
                        .optional().into();
                    if let Some(entry) = existing_entry {
                        break Ok(entry);
                    } else {
                        let allocation = createBatchCycleBasedOnInterval(
                            node_id.to_string(),
                            DEFAULT_INTERVAL,
                            DEFAULT_EXTRA_TIME_FOR_PAYMENT,
                        ).expect("Failed to create default batch cycle");
                        diesel::insert_into(dsl::pay_batch_cycle)
                            .values(allocation.clone())
                            .execute(conn)?;
                    }
                    loop_count += 1;
                    if loop_count > 1 {
                        return Err(anyhow!(
                            "Failed to insert default batch cycle"
                        )
                        .into());
                    }
                }
            },
        )
        .await
    }

    pub async fn create(
        &self,
        owner_id: String,
        interval: Option<Duration>,
        cron: Option<String>,
        next_running_time: DateTime<Utc>,
    ) -> DbResult<()> {
        let now = Utc::now().adapt();
        let cycle = if let Some(interval) = interval {
            match createBatchCycleBasedOnInterval(
                owner_id.to_string(),
                interval,
                DEFAULT_EXTRA_TIME_FOR_PAYMENT,
            ) {
                Ok(cycle) => cycle,
                Err(err) => {
                    return Err(err.into());
                }
            }
        } else if let Some(cron) = cron {
            match createBatchCycleBasedOnCron(
                owner_id,
                &cron,
                DEFAULT_EXTRA_TIME_FOR_PAYMENT,
            ) {
                Ok(cycle) => cycle,
                Err(err) => {
                    return Err(err.into());
                }
            }
        } else {
            return Err(anyhow!("Either interval or cron must be provided".to_string(),).into());
        };
        do_with_transaction(self.pool, "pay_batch_cycle_create", move |conn| {
            diesel::insert_into(dsl::pay_batch_cycle)
                .values(&cycle)
                .execute(conn)?;
            Ok(())
        })
        .await
    }
}
