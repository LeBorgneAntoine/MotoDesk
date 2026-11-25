-- Device-local assistant. Not an EntityType: conversations hold PII and
-- must not mesh-replicate.

CREATE TABLE assistant_conversations (
  id TEXT PRIMARY KEY NOT NULL,
  title TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  active_artifact_id TEXT
);

CREATE TABLE assistant_messages (
  id TEXT PRIMARY KEY NOT NULL,
  conversation_id TEXT NOT NULL REFERENCES assistant_conversations(id) ON DELETE CASCADE,
  role TEXT NOT NULL,
  content TEXT NOT NULL,
  created_at TEXT NOT NULL,
  seq INTEGER NOT NULL
);

CREATE TABLE assistant_artifacts (
  id TEXT PRIMARY KEY NOT NULL,
  conversation_id TEXT NOT NULL REFERENCES assistant_conversations(id) ON DELETE CASCADE,
  title TEXT NOT NULL,
  kind TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  seq INTEGER NOT NULL
);

CREATE INDEX assistant_conversations_updated_idx
  ON assistant_conversations(updated_at DESC);

CREATE INDEX assistant_messages_conv_seq_idx
  ON assistant_messages(conversation_id, seq);

CREATE INDEX assistant_artifacts_conv_seq_idx
  ON assistant_artifacts(conversation_id, seq);
