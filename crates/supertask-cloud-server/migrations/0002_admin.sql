ALTER TABLE accounts ADD COLUMN role TEXT NOT NULL DEFAULT 'user';
CREATE INDEX IF NOT EXISTS accounts_role ON accounts(role);
