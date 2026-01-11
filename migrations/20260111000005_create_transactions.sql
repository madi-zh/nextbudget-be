-- Create transactions table
-- Individual financial transactions linked to categories and optionally accounts

CREATE TABLE IF NOT EXISTS transactions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),

    -- Category is required (determines which budget this belongs to)
    category_id UUID NOT NULL REFERENCES categories(id) ON DELETE CASCADE,

    -- Account is optional; SET NULL preserves transaction history if account deleted
    account_id UUID REFERENCES accounts(id) ON DELETE SET NULL,

    -- Transaction details
    amount NUMERIC(12,2) NOT NULL,
    transaction_type VARCHAR(10) NOT NULL DEFAULT 'expense',
    transaction_date TIMESTAMPTZ NOT NULL,
    description VARCHAR(200),

    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- Constraints
    CONSTRAINT chk_transactions_amount CHECK (amount > 0),
    CONSTRAINT chk_transactions_type CHECK (transaction_type IN ('expense', 'income', 'transfer'))
);

-- Primary query: Get transactions for a category (for spent amount calculation)
CREATE INDEX idx_transactions_category ON transactions(category_id);

-- Query pattern: Get transactions for an account
CREATE INDEX idx_transactions_account ON transactions(account_id)
    WHERE account_id IS NOT NULL;

-- Query pattern: Get recent transactions, sorted by date
CREATE INDEX idx_transactions_date ON transactions(transaction_date DESC);

-- Query pattern: Filter by type within a category (for expense sums)
CREATE INDEX idx_transactions_category_type ON transactions(category_id, transaction_type);

-- Composite index for common query: transactions in date range for a category
CREATE INDEX idx_transactions_category_date
    ON transactions(category_id, transaction_date DESC);
