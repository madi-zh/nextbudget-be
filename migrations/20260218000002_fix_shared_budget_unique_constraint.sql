-- Fix unique constraint to allow shared budgets for the same owner/month/year
-- The old constraint (owner_id, month, year) prevents creating shared budgets
-- when a personal budget already exists for that period.

ALTER TABLE budgets DROP CONSTRAINT uq_budgets_owner_month_year;
ALTER TABLE budgets ADD CONSTRAINT uq_budgets_owner_month_year_shared UNIQUE (owner_id, month, year, is_shared, name);
