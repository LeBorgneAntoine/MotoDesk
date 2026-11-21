-- A restored backup is older than what the cloud already holds from this PC. The
-- number of this PC's own changes the cloud has and the restored base lacks is kept
-- here, until someone has read the warning, so it is not lost with the next restart.
ALTER TABLE sync_state ADD COLUMN seq_gap INTEGER NOT NULL DEFAULT 0;
