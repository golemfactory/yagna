


CREATE TABLE pay_batch_cycle
(
    owner_id VARCHAR(50) NOT NULL,
    platform VARCHAR(50) NOT NULL,
    created_ts VARCHAR(50) NOT NULL,
    updated_ts VARCHAR(50) NOT NULL,
    cycle_interval VARCHAR(50),
    cycle_cron VARCHAR(50),
    cycle_last_process DATETIME,
    cycle_next_process DATETIME NOT NULL,
    cycle_max_interval VARCHAR(50) NOT NULL,
    cycle_max_pay_time VARCHAR(50) NOT NULL,

    CONSTRAINT PAY_BATCH_CYCLE_PK PRIMARY KEY(owner_id, platform),
    CONSTRAINT PAY_BATCH_CYCLE_CHECK_1 CHECK((cycle_interval IS NULL) <> (cycle_cron IS NULL))
)