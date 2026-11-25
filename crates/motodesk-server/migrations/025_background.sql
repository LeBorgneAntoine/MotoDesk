-- Keep the app running in the background when its window is closed, and start it with the
-- Windows session (see `background.rs`). NULL means "not chosen": a PC of a local network is on,
-- any other installation is off.
ALTER TABLE sync_state ADD COLUMN background_enabled INTEGER;
