CREATE TABLE IF NOT EXISTS accounts (
    id TEXT PRIMARY KEY NOT NULL,
    email TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    disabled INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS access_tokens (
    token_hash TEXT PRIMARY KEY NOT NULL,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    device_id TEXT NOT NULL,
    expires_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS refresh_tokens (
    token_hash TEXT PRIMARY KEY NOT NULL,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    device_id TEXT NOT NULL,
    expires_at INTEGER NOT NULL,
    revoked_at INTEGER
);

CREATE TABLE IF NOT EXISTS entities (
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    id TEXT NOT NULL,
    type TEXT NOT NULL,
    rev INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    updated_by TEXT NOT NULL,
    data TEXT NOT NULL,
    byte_size INTEGER NOT NULL,
    PRIMARY KEY (account_id, id)
);
CREATE INDEX IF NOT EXISTS entities_account_type ON entities(account_id, type);
CREATE INDEX IF NOT EXISTS entities_account_updated_at ON entities(account_id, updated_at);
CREATE INDEX IF NOT EXISTS access_tokens_account_expires ON access_tokens(account_id, expires_at);
CREATE INDEX IF NOT EXISTS refresh_tokens_account_expires ON refresh_tokens(account_id, expires_at);

CREATE TABLE IF NOT EXISTS telemetry_batches (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    received_at INTEGER NOT NULL,
    event_count INTEGER NOT NULL
);
