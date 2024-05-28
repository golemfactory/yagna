-- Your SQL goes here
alter table pay_allocation
    add extend_timeout_by integer;

alter table pay_allocation
    add accepted_amount;

alter table pay_allocation
    add rec_version integer not null default 0;

drop view pay_debit_note_event_read;

create table pay_debit_note_event_dg_tmp
(
    owner_id      VARCHAR(50)                                             not null,
    debit_note_id VARCHAR(50)                                             not null,
    event_type    VARCHAR(50)                                             not null
        references pay_event_type,
    timestamp     DATETIME default (STRFTIME('%Y-%m-%d %H:%M:%f', 'NOW')) not null,
    details       TEXT,
    CONSTRAINT pay_debit_note_event_pk primary key (owner_id, debit_note_id, event_type),
    foreign key (owner_id, debit_note_id) references pay_debit_note
);

insert into pay_debit_note_event_dg_tmp(debit_note_id, owner_id, event_type, timestamp, details)
select debit_note_id, owner_id, event_type, timestamp, details
from pay_debit_note_event;

drop table pay_debit_note_event;

alter table pay_debit_note_event_dg_tmp
    rename to pay_debit_note_event;

create index pay_debit_note_event_owner_idx
    on pay_debit_note_event (owner_id);

create index pay_debit_note_event_timestamp_idx
    on pay_debit_note_event (timestamp);

CREATE VIEW pay_debit_note_event_read AS
SELECT
    dn.role,
    dne.debit_note_id,
    dne.owner_id,
    dne.event_type,
    dne.timestamp,
    dne.details,
    agr.app_session_id
FROM
    pay_debit_note_event dne
        INNER JOIN pay_debit_note dn ON dne.owner_id = dn.owner_id AND dne.debit_note_id = dn.id
        INNER JOIN pay_activity act ON dne.owner_id = act.owner_id AND dn.activity_id = act.id
        INNER JOIN pay_agreement agr ON dne.owner_id = agr.owner_id AND act.agreement_id = agr.id;

ALTER TABLE main.pay_activity
    ADD accepted_close_amount varchar(32) null;