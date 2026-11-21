-- A change the cloud refuses used to stay at the very front of the queue (changes are
-- sent oldest first), so enough of them hid everything behind them. Each change now
-- remembers how many times it was refused, why, and when it may be tried again.
ALTER TABLE change_log ADD COLUMN push_attempts INTEGER NOT NULL DEFAULT 0;
ALTER TABLE change_log ADD COLUMN push_error TEXT;
ALTER TABLE change_log ADD COLUMN retry_after TEXT;
