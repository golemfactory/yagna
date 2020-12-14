DROP TABLE pay_allocation;

CREATE TABLE pay_allocation(
    id VARCHAR(50) NOT NULL PRIMARY KEY,
    owner_id VARCHAR(50) NOT NULL,
    payment_platform VARCHAR(50) NOT NULL,
    address VARCHAR(50) NOT NULL,
    total_amount VARCHAR(32) NOT NULL,
    spent_amount VARCHAR(32) NOT NULL,
    remaining_amount VARCHAR(32) NOT NULL,
    timestamp DATETIME DEFAULT(STRFTIME('%Y-%m-%d %H:%M:%f', 'NOW')),
    timeout DATETIME NULL,
    make_deposit BOOLEAN NOT NULL,
    released BOOLEAN NOT NULL DEFAULT FALSE
);
