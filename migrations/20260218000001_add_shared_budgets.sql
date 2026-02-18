ALTER TABLE budgets ADD COLUMN is_shared BOOLEAN NOT NULL DEFAULT false;
ALTER TABLE budgets ADD COLUMN name VARCHAR(100) NOT NULL DEFAULT '';

CREATE TABLE budget_members (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    budget_id UUID NOT NULL REFERENCES budgets(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT uq_budget_members UNIQUE (budget_id, user_id)
);

CREATE INDEX idx_budget_members_user ON budget_members(user_id);
CREATE INDEX idx_budget_members_budget ON budget_members(budget_id);
