ALTER TABLE users ADD COLUMN default_currency CHAR(3) NOT NULL DEFAULT 'USD' REFERENCES currencies(code);
