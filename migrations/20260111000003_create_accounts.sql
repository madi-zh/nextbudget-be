-- Create accounts table
-- Represents financial accounts (checking, savings, credit cards)

CREATE TABLE IF NOT EXISTS accounts (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,

    name VARCHAR(50) NOT NULL,
    account_type VARCHAR(10) NOT NULL,

    -- Balance can be negative (credit cards, overdrafts)
    balance NUMERIC(12,2) NOT NULL DEFAULT 0,

    -- UI customization
    color_hex CHAR(7) NOT NULL DEFAULT '#64748b',

    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- Constraints
    CONSTRAINT chk_accounts_type CHECK (account_type IN ('checking', 'savings', 'credit')),
    CONSTRAINT chk_accounts_color CHECK (color_hex ~ '^#[0-9A-Fa-f]{6}$'),
    CONSTRAINT chk_accounts_name_length CHECK (LENGTH(TRIM(name)) >= 1)
);

-- Primary query: List all accounts for a user
CREATE INDEX idx_accounts_owner ON accounts(owner_id);

-- Query pattern: Get accounts by type for summary calculations
CREATE INDEX idx_accounts_owner_type ON accounts(owner_id, account_type);
