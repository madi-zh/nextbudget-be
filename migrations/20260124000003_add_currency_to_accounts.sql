ALTER TABLE accounts ADD COLUMN currency CHAR(3) NOT NULL DEFAULT 'USD' REFERENCES currencies(code);
CREATE INDEX idx_accounts_owner_currency ON accounts(owner_id, currency);
