ALTER TABLE pay_payment
    ADD COLUMN signature VARCHAR(32);
ALTER TABLE pay_payment
    ADD COLUMN signed_bytes VARCHAR(32);
