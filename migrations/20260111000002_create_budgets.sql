-- Create budgets table
-- Each budget represents a monthly financial plan for a user

CREATE TABLE IF NOT EXISTS budgets (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,

    -- Month is 0-indexed (0=January, 11=December) to match JavaScript Date
    month SMALLINT NOT NULL,
    year SMALLINT NOT NULL,

    -- Financial data
    total_income NUMERIC(12,2) NOT NULL DEFAULT 0,
    savings_rate NUMERIC(5,2) NOT NULL DEFAULT 0,

    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- Constraints
    CONSTRAINT chk_budgets_month CHECK (month >= 0 AND month <= 11),
    CONSTRAINT chk_budgets_year CHECK (year >= 2000 AND year <= 2100),
    CONSTRAINT chk_budgets_income CHECK (total_income >= 0),
    CONSTRAINT chk_budgets_savings_rate CHECK (savings_rate >= 0 AND savings_rate <= 100),

    -- Each user can have only one budget per month/year
    CONSTRAINT uq_budgets_owner_month_year UNIQUE (owner_id, month, year)
);

-- Index for listing all budgets for a user ordered by date
CREATE INDEX idx_budgets_owner_date ON budgets(owner_id, year DESC, month DESC);
