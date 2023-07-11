
ALTER TABLE `transaction` RENAME COLUMN network TO network_old;
ALTER TABLE `transaction` ADD COLUMN network INTEGER NOT NULL DEFAULT 5;  -- 5 is goerli's network ID

UPDATE `transaction` SET network = network_old;

ALTER TABLE `transaction` DROP COLUMN network_old;
