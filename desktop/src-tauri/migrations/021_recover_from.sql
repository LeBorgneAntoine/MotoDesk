-- Where the numbering of this PC's own changes stood in the database before the cloud
-- showed that it was behind (a restored backup). Recovery asks the cloud for this PC's
-- changes after that number.
ALTER TABLE sync_state ADD COLUMN seq_recover_from INTEGER NOT NULL DEFAULT 0;
