-- Create categories table
-- Budget categories for organizing spending (e.g., "Groceries", "Entertainment")

CREATE TABLE IF NOT EXISTS categories (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    budget_id UUID NOT NULL REFERENCES budgets(id) ON DELETE CASCADE,

    name VARCHAR(50) NOT NULL,
    allocated_amount NUMERIC(12,2) NOT NULL DEFAULT 0,

    -- UI customization
    color_hex CHAR(7) NOT NULL DEFAULT '#64748b',

    -- Timestamps
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- Constraints
    CONSTRAINT chk_categories_allocated CHECK (allocated_amount >= 0),
    CONSTRAINT chk_categories_color CHECK (color_hex ~ '^#[0-9A-Fa-f]{6}$'),
    CONSTRAINT chk_categories_name_length CHECK (LENGTH(TRIM(name)) >= 1)
);

-- Primary query: Get all categories for a budget
CREATE INDEX idx_categories_budget ON categories(budget_id);
