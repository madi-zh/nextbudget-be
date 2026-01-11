# BudgetFlow Database Schema Migration Plan

## Document Information
- **Version**: 1.0
- **Created**: 2026-01-11
- **Status**: Draft - Pending Review
- **Author**: Database Architecture Review

---

## Table of Contents
1. [Executive Summary](#1-executive-summary)
2. [Current State Analysis](#2-current-state-analysis)
3. [Schema Design Decisions](#3-schema-design-decisions)
4. [Migration Plan](#4-migration-plan)
5. [Complete SQL Definitions](#5-complete-sql-definitions)
6. [Index Strategy](#6-index-strategy)
7. [Rollback Procedures](#7-rollback-procedures)
8. [Query Pattern Optimization](#8-query-pattern-optimization)
9. [Testing Strategy](#9-testing-strategy)

---

## 1. Executive Summary

This document outlines the database schema migration plan for the BudgetFlow application. The plan addresses:

- **7 tables** total (1 existing, 6 new)
- **Migration sequence** respecting foreign key dependencies
- **UUID vs VARCHAR** primary key decision (recommendation: keep UUID)
- **Performance optimizations** through strategic indexing
- **Data integrity** via constraints and triggers

### Key Recommendations

| Decision Point | Recommendation | Rationale |
|----------------|----------------|-----------|
| User ID Type | Keep UUID | Already implemented, better security, native PostgreSQL support |
| Other Entity IDs | UUID | Consistency, distributed-system ready, no sequential guessing |
| Timestamp Storage | TIMESTAMPTZ | Already implemented, timezone-aware |
| Money Storage | NUMERIC(12,2) | Exact decimal arithmetic, no floating-point errors |
| Date Storage | TIMESTAMPTZ (not BIGINT) | Native date functions, timezone support, better indexing |

---

## 2. Current State Analysis

### 2.1 Existing Users Table

**Location**: `migrations/20250101000000_create_users_table.sql`

```sql
CREATE TABLE IF NOT EXISTS users (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    email VARCHAR(255) NOT NULL UNIQUE,
    password_hash VARCHAR(255) NOT NULL,
    full_name VARCHAR(100),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

**Rust Model** (`src/models.rs`):
- Uses `Uuid` type from `uuid` crate
- `DateTime<Utc>` for timestamps
- `TokenClaims.sub` is `Uuid`

### 2.2 Discrepancies with BACKEND_PLAN.md

| Aspect | Current Implementation | BACKEND_PLAN.md | Resolution |
|--------|----------------------|-----------------|------------|
| User ID Type | `UUID` | `VARCHAR(50)` | **Keep UUID** |
| User ID Column | `id` | `uid` | **Keep `id`** |
| Display Name | `full_name` (nullable) | `display_name` (NOT NULL) | **Keep `full_name`**, make NOT NULL in future migration |
| Password Hash Length | `VARCHAR(255)` | `VARCHAR(72)` | **Keep 255** (Argon2 hashes can exceed 72 chars) |
| Email Index | Missing | Present | **Add index** |

### 2.3 Why Keep UUID Over VARCHAR

1. **Already Implemented**: Changing would require data migration and Rust model updates
2. **Native PostgreSQL Support**: `gen_random_uuid()` is efficient and built-in
3. **Security**: UUIDs cannot be guessed sequentially (unlike auto-increment)
4. **Distributed Systems**: UUIDs can be generated client-side without coordination
5. **Storage Efficiency**: UUID is 16 bytes vs VARCHAR(36) which is 37+ bytes
6. **Index Performance**: Binary UUID comparisons are faster than string comparisons
7. **Type Safety**: Strong typing in Rust with `uuid::Uuid`

---

## 3. Schema Design Decisions

### 3.1 Primary Key Strategy

All tables will use **UUID** primary keys for consistency:

```sql
id UUID PRIMARY KEY DEFAULT gen_random_uuid()
```

**Rationale**:
- Consistency across all entities
- Prevents information leakage (no sequential IDs revealing record counts)
- Enables future sharding without ID collisions
- Client-side ID generation possible for offline-first features

### 3.2 Timestamp Strategy

All tables include audit timestamps:

```sql
created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
```

An `updated_at` trigger will be created once and shared across all tables.

### 3.3 Transaction Date: TIMESTAMPTZ vs BIGINT

**BACKEND_PLAN.md specifies**: `date BIGINT NOT NULL` (Unix timestamp)

**Recommendation**: Use `TIMESTAMPTZ` instead

| Factor | BIGINT (Unix) | TIMESTAMPTZ |
|--------|---------------|-------------|
| Storage | 8 bytes | 8 bytes |
| Range queries | Manual conversion | Native `BETWEEN`, `>=`, `<` |
| Timezone handling | Manual | Automatic |
| Date extraction | `to_timestamp()` | `DATE_TRUNC()`, `EXTRACT()` |
| Index efficiency | Good | Excellent with BRIN |
| Frontend compatibility | Direct use | Requires formatting |

**Decision**: Use `TIMESTAMPTZ` for richer query capabilities. The API layer can convert to/from Unix timestamps if the frontend requires it.

### 3.4 Money/Amount Storage

Using `NUMERIC(12,2)` for all monetary values:
- **Precision**: 12 total digits, 2 decimal places
- **Range**: -9,999,999,999.99 to 9,999,999,999.99
- **No floating-point errors**: Exact decimal arithmetic

### 3.5 Enum Strategy

Using `CHECK` constraints instead of PostgreSQL `ENUM` types:

```sql
type VARCHAR(10) NOT NULL CHECK (type IN ('checking', 'savings', 'credit'))
```

**Rationale**:
- Easier to modify (adding values doesn't require `ALTER TYPE`)
- Simpler migrations
- Works identically with SQLx

---

## 4. Migration Plan

### 4.1 Migration Order

Dependencies require this sequence:

```
Migration 1: Add email index to users (no dependencies)
Migration 2: Create refresh_tokens (depends on users)
Migration 3: Create budgets (depends on users)
Migration 4: Create accounts (depends on users)
Migration 5: Create categories (depends on budgets)
Migration 6: Create transactions (depends on categories, accounts)
Migration 7: Create audit_logs (depends on users, optional)
Migration 8: Create updated_at trigger function (utility)
Migration 9: Apply triggers to all tables
```

### 4.2 Dependency Graph

```
users
  |
  +---> refresh_tokens
  |
  +---> budgets
  |       |
  |       +---> categories
  |               |
  |               +---> transactions
  |                       ^
  +---> accounts ---------+
  |
  +---> audit_logs
```

---

## 5. Complete SQL Definitions

### 5.1 Migration: Add Email Index to Users

**File**: `migrations/YYYYMMDDHHMMSS_add_users_email_index.sql`

```sql
-- Add index on email for faster lookups during authentication
-- This is idempotent; the UNIQUE constraint already creates an index,
-- but we explicitly name it for clarity and potential future reference

-- Note: The UNIQUE constraint on email already creates an implicit index,
-- so this migration documents that fact. No additional index needed.

-- If you want an explicit named index (for documentation/tooling):
-- CREATE INDEX IF NOT EXISTS idx_users_email ON users(email);

-- Instead, let's add a check that ensures email is lowercase
-- (optional - implement in application layer instead if preferred)

COMMENT ON COLUMN users.email IS 'User email address, used for authentication. Unique constraint provides implicit index.';
```

### 5.2 Migration: Create Refresh Tokens Table

**File**: `migrations/YYYYMMDDHHMMSS_create_refresh_tokens.sql`

```sql
-- Create refresh_tokens table for JWT refresh token management
-- Stores hashed tokens (SHA-256) for security

CREATE TABLE IF NOT EXISTS refresh_tokens (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash VARCHAR(64) NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    revoked_at TIMESTAMPTZ,

    -- Device/session tracking (optional but recommended)
    user_agent TEXT,
    ip_address INET
);

-- Unique constraint on token_hash prevents duplicate tokens
CREATE UNIQUE INDEX idx_refresh_tokens_hash ON refresh_tokens(token_hash);

-- Index for finding active tokens by user (for logout-all-devices)
CREATE INDEX idx_refresh_tokens_user_active
    ON refresh_tokens(user_id)
    WHERE revoked_at IS NULL;

-- Index for cleanup job to find expired tokens
CREATE INDEX idx_refresh_tokens_expires
    ON refresh_tokens(expires_at)
    WHERE revoked_at IS NULL;

COMMENT ON TABLE refresh_tokens IS 'Stores hashed refresh tokens for JWT authentication';
COMMENT ON COLUMN refresh_tokens.token_hash IS 'SHA-256 hash of the refresh token';
COMMENT ON COLUMN refresh_tokens.revoked_at IS 'NULL if active, timestamp if revoked';
```

**Rollback**:
```sql
DROP TABLE IF EXISTS refresh_tokens;
```

### 5.3 Migration: Create Budgets Table

**File**: `migrations/YYYYMMDDHHMMSS_create_budgets.sql`

```sql
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

-- Primary query pattern: Get budget for specific user/month/year
-- The unique constraint already creates this index, but we document it
COMMENT ON CONSTRAINT uq_budgets_owner_month_year ON budgets IS
    'Ensures one budget per user per month. Also serves as the primary lookup index.';

-- Index for listing all budgets for a user ordered by date
CREATE INDEX idx_budgets_owner_date ON budgets(owner_id, year DESC, month DESC);

COMMENT ON TABLE budgets IS 'Monthly budget plans. One per user per month.';
COMMENT ON COLUMN budgets.month IS '0-indexed month (0=Jan, 11=Dec) to match JavaScript Date.getMonth()';
COMMENT ON COLUMN budgets.savings_rate IS 'Percentage of income to save (0-100)';
```

**Rollback**:
```sql
DROP TABLE IF EXISTS budgets;
```

### 5.4 Migration: Create Accounts Table

**File**: `migrations/YYYYMMDDHHMMSS_create_accounts.sql`

```sql
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

COMMENT ON TABLE accounts IS 'Financial accounts belonging to users';
COMMENT ON COLUMN accounts.account_type IS 'Account type: checking, savings, or credit';
COMMENT ON COLUMN accounts.balance IS 'Current balance. Negative values allowed for credit accounts.';
COMMENT ON COLUMN accounts.color_hex IS 'Hex color code for UI display (e.g., #64748b)';
```

**Rollback**:
```sql
DROP TABLE IF EXISTS accounts;
```

### 5.5 Migration: Create Categories Table

**File**: `migrations/YYYYMMDDHHMMSS_create_categories.sql`

```sql
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

COMMENT ON TABLE categories IS 'Spending categories within a budget';
COMMENT ON COLUMN categories.allocated_amount IS 'Budgeted amount for this category';
```

**Rollback**:
```sql
DROP TABLE IF EXISTS categories;
```

### 5.6 Migration: Create Transactions Table

**File**: `migrations/YYYYMMDDHHMMSS_create_transactions.sql`

```sql
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
    CONSTRAINT chk_transactions_type CHECK (transaction_type IN ('expense', 'income', 'transfer')),
    CONSTRAINT chk_transactions_description_length CHECK (
        description IS NULL OR LENGTH(description) <= 200
    )
);

-- Primary query: Get transactions for a category (for spent amount calculation)
CREATE INDEX idx_transactions_category ON transactions(category_id);

-- Query pattern: Get transactions for an account
CREATE INDEX idx_transactions_account ON transactions(account_id)
    WHERE account_id IS NOT NULL;

-- Query pattern: Get recent transactions, sorted by date
-- Using BRIN for large tables with date-ordered inserts
CREATE INDEX idx_transactions_date ON transactions(transaction_date DESC);

-- Query pattern: Filter by type within a category (for expense sums)
CREATE INDEX idx_transactions_category_type ON transactions(category_id, transaction_type);

-- Composite index for common query: transactions in date range for a category
CREATE INDEX idx_transactions_category_date
    ON transactions(category_id, transaction_date DESC);

COMMENT ON TABLE transactions IS 'Financial transactions linked to budget categories';
COMMENT ON COLUMN transactions.transaction_type IS 'expense (subtract from budget), income (add to budget), transfer (between accounts)';
COMMENT ON COLUMN transactions.transaction_date IS 'When the transaction occurred';
COMMENT ON COLUMN transactions.account_id IS 'Optional link to account. NULL if account was deleted.';
```

**Rollback**:
```sql
DROP TABLE IF EXISTS transactions;
```

### 5.7 Migration: Create Audit Logs Table (Optional)

**File**: `migrations/YYYYMMDDHHMMSS_create_audit_logs.sql`

```sql
-- Create audit_logs table (optional)
-- Tracks changes to entities for debugging and compliance

CREATE TABLE IF NOT EXISTS audit_logs (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,

    -- Who made the change (NULL if user was deleted or system action)
    user_id UUID REFERENCES users(id) ON DELETE SET NULL,

    -- What was changed
    entity_type VARCHAR(20) NOT NULL,
    entity_id UUID NOT NULL,
    action VARCHAR(10) NOT NULL,

    -- Change details as JSONB for flexible schema
    old_values JSONB,
    new_values JSONB,

    -- Metadata
    ip_address INET,
    user_agent TEXT,

    -- Timestamp (no updated_at - audit logs are immutable)
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- Constraints
    CONSTRAINT chk_audit_action CHECK (action IN ('CREATE', 'UPDATE', 'DELETE'))
);

-- Query pattern: Get audit history for a specific entity
CREATE INDEX idx_audit_entity ON audit_logs(entity_type, entity_id, created_at DESC);

-- Query pattern: Get all actions by a user
CREATE INDEX idx_audit_user ON audit_logs(user_id, created_at DESC)
    WHERE user_id IS NOT NULL;

-- Query pattern: Recent audit logs (for admin dashboard)
-- BRIN is efficient for append-only timestamp-ordered data
CREATE INDEX idx_audit_created_brin ON audit_logs USING BRIN(created_at);

-- GIN index for searching within JSONB (if needed)
-- CREATE INDEX idx_audit_old_values ON audit_logs USING GIN(old_values);
-- CREATE INDEX idx_audit_new_values ON audit_logs USING GIN(new_values);

COMMENT ON TABLE audit_logs IS 'Immutable audit trail of entity changes';
COMMENT ON COLUMN audit_logs.entity_type IS 'Table name: users, budgets, categories, accounts, transactions';
COMMENT ON COLUMN audit_logs.old_values IS 'Previous values (NULL for CREATE)';
COMMENT ON COLUMN audit_logs.new_values IS 'New values (NULL for DELETE)';
```

**Rollback**:
```sql
DROP TABLE IF EXISTS audit_logs;
```

### 5.8 Migration: Create Updated_At Trigger Function

**File**: `migrations/YYYYMMDDHHMMSS_create_updated_at_trigger.sql`

```sql
-- Create a reusable trigger function for updating the updated_at column
-- This function is called by triggers on each table

CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

COMMENT ON FUNCTION update_updated_at_column() IS
    'Trigger function that sets updated_at to current timestamp on UPDATE';

-- Apply triggers to all tables with updated_at column
-- (Run after all tables are created)

CREATE TRIGGER trg_users_updated_at
    BEFORE UPDATE ON users
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER trg_budgets_updated_at
    BEFORE UPDATE ON budgets
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER trg_categories_updated_at
    BEFORE UPDATE ON categories
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER trg_accounts_updated_at
    BEFORE UPDATE ON accounts
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER trg_transactions_updated_at
    BEFORE UPDATE ON transactions
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at_column();

-- Note: refresh_tokens and audit_logs don't need updated_at triggers
-- refresh_tokens: tokens are created and revoked, not updated
-- audit_logs: immutable by design
```

**Rollback**:
```sql
DROP TRIGGER IF EXISTS trg_transactions_updated_at ON transactions;
DROP TRIGGER IF EXISTS trg_accounts_updated_at ON accounts;
DROP TRIGGER IF EXISTS trg_categories_updated_at ON categories;
DROP TRIGGER IF EXISTS trg_budgets_updated_at ON budgets;
DROP TRIGGER IF EXISTS trg_users_updated_at ON users;
DROP FUNCTION IF EXISTS update_updated_at_column();
```

---

## 6. Index Strategy

### 6.1 Index Summary by Table

| Table | Index Name | Columns | Type | Purpose |
|-------|-----------|---------|------|---------|
| users | (implicit from UNIQUE) | email | B-tree | Auth lookups |
| refresh_tokens | idx_refresh_tokens_hash | token_hash | B-tree UNIQUE | Token validation |
| refresh_tokens | idx_refresh_tokens_user_active | user_id | Partial B-tree | Logout all devices |
| refresh_tokens | idx_refresh_tokens_expires | expires_at | Partial B-tree | Cleanup job |
| budgets | uq_budgets_owner_month_year | owner_id, month, year | B-tree UNIQUE | Primary lookup |
| budgets | idx_budgets_owner_date | owner_id, year, month | B-tree | List user budgets |
| accounts | idx_accounts_owner | owner_id | B-tree | List user accounts |
| accounts | idx_accounts_owner_type | owner_id, account_type | B-tree | Summary by type |
| categories | idx_categories_budget | budget_id | B-tree | List budget categories |
| transactions | idx_transactions_category | category_id | B-tree | List category transactions |
| transactions | idx_transactions_account | account_id | Partial B-tree | List account transactions |
| transactions | idx_transactions_date | transaction_date | B-tree | Recent transactions |
| transactions | idx_transactions_category_type | category_id, transaction_type | B-tree | Expense sums |
| transactions | idx_transactions_category_date | category_id, transaction_date | B-tree | Date-filtered queries |
| audit_logs | idx_audit_entity | entity_type, entity_id, created_at | B-tree | Entity history |
| audit_logs | idx_audit_user | user_id, created_at | Partial B-tree | User activity |
| audit_logs | idx_audit_created_brin | created_at | BRIN | Time-range scans |

### 6.2 Index Design Rationale

**Partial Indexes**: Used where queries filter on specific conditions:
- `idx_refresh_tokens_user_active`: Only non-revoked tokens
- `idx_transactions_account`: Only transactions with accounts

**Composite Index Column Order**: Most selective column first for equality predicates:
- `idx_budgets_owner_date`: owner_id (high selectivity), then date columns

**BRIN for Audit Logs**: Append-only table with timestamp ordering makes BRIN highly efficient (much smaller than B-tree for time-range queries).

---

## 7. Rollback Procedures

### 7.1 Individual Table Rollbacks

Each migration includes its rollback at the end of section 5. Summary:

| Migration | Rollback Command |
|-----------|------------------|
| Refresh Tokens | `DROP TABLE IF EXISTS refresh_tokens;` |
| Budgets | `DROP TABLE IF EXISTS budgets;` |
| Accounts | `DROP TABLE IF EXISTS accounts;` |
| Categories | `DROP TABLE IF EXISTS categories;` |
| Transactions | `DROP TABLE IF EXISTS transactions;` |
| Audit Logs | `DROP TABLE IF EXISTS audit_logs;` |
| Triggers | Drop triggers then function (see 5.8) |

### 7.2 Full Rollback Sequence

To rollback all migrations (in reverse dependency order):

```sql
-- Step 1: Drop triggers
DROP TRIGGER IF EXISTS trg_transactions_updated_at ON transactions;
DROP TRIGGER IF EXISTS trg_accounts_updated_at ON accounts;
DROP TRIGGER IF EXISTS trg_categories_updated_at ON categories;
DROP TRIGGER IF EXISTS trg_budgets_updated_at ON budgets;
DROP TRIGGER IF EXISTS trg_users_updated_at ON users;
DROP FUNCTION IF EXISTS update_updated_at_column();

-- Step 2: Drop tables (reverse dependency order)
DROP TABLE IF EXISTS audit_logs;
DROP TABLE IF EXISTS transactions;
DROP TABLE IF EXISTS categories;
DROP TABLE IF EXISTS accounts;
DROP TABLE IF EXISTS budgets;
DROP TABLE IF EXISTS refresh_tokens;

-- Note: users table is NOT dropped (it existed before this migration plan)
```

### 7.3 SQLx Migration Files

For SQLx, create both up and down migrations:

```
migrations/
  20260111000001_create_refresh_tokens.sql
  20260111000002_create_budgets.sql
  20260111000003_create_accounts.sql
  20260111000004_create_categories.sql
  20260111000005_create_transactions.sql
  20260111000006_create_audit_logs.sql
  20260111000007_create_updated_at_triggers.sql
```

SQLx doesn't have built-in down migrations, but you can:
1. Keep rollback SQL in comments at the end of each migration
2. Create a separate `rollback/` directory with rollback scripts
3. Use `sqlx migrate revert` with reversible migrations (if configured)

---

## 8. Query Pattern Optimization

### 8.1 Common Query Patterns and Their Indexes

#### Get User's Budget for Specific Month/Year
```sql
SELECT * FROM budgets
WHERE owner_id = $1 AND month = $2 AND year = $3;
-- Uses: uq_budgets_owner_month_year (unique index)
-- Performance: Index-only scan, O(log n)
```

#### Get Categories with Spent Amounts
```sql
SELECT
    c.*,
    COALESCE(SUM(t.amount) FILTER (WHERE t.transaction_type = 'expense'), 0) as spent_amount
FROM categories c
LEFT JOIN transactions t ON c.id = t.category_id
WHERE c.budget_id = $1
GROUP BY c.id;
-- Uses: idx_categories_budget, idx_transactions_category
-- Consider: Materialized view if this is very frequent
```

#### Get Recent Transactions for Account
```sql
SELECT * FROM transactions
WHERE account_id = $1
ORDER BY transaction_date DESC
LIMIT 50;
-- Uses: idx_transactions_account (partial)
-- Performance: Index scan with limit push-down
```

#### Account Summary by Type
```sql
SELECT
    account_type,
    SUM(balance) as total_balance,
    COUNT(*) as account_count
FROM accounts
WHERE owner_id = $1
GROUP BY account_type;
-- Uses: idx_accounts_owner_type
-- Performance: Index-only scan possible
```

#### Validate Refresh Token
```sql
SELECT * FROM refresh_tokens
WHERE token_hash = $1
  AND expires_at > NOW()
  AND revoked_at IS NULL;
-- Uses: idx_refresh_tokens_hash
-- Performance: Index lookup, O(1)
```

### 8.2 Pagination Recommendations

**For small datasets (< 1000 records)**: Use OFFSET/LIMIT
```sql
SELECT * FROM transactions
WHERE category_id = $1
ORDER BY transaction_date DESC
LIMIT 20 OFFSET 40;
```

**For large datasets**: Use keyset pagination
```sql
-- First page
SELECT * FROM transactions
WHERE category_id = $1
ORDER BY transaction_date DESC, id DESC
LIMIT 20;

-- Next page (using last row's values)
SELECT * FROM transactions
WHERE category_id = $1
  AND (transaction_date, id) < ($last_date, $last_id)
ORDER BY transaction_date DESC, id DESC
LIMIT 20;
```

---

## 9. Testing Strategy

### 9.1 Migration Tests

```sql
-- Test 1: Verify all tables exist
SELECT table_name FROM information_schema.tables
WHERE table_schema = 'public'
AND table_name IN ('users', 'refresh_tokens', 'budgets', 'categories', 'accounts', 'transactions', 'audit_logs');

-- Test 2: Verify foreign key constraints
SELECT
    tc.table_name,
    kcu.column_name,
    ccu.table_name AS foreign_table_name,
    ccu.column_name AS foreign_column_name
FROM information_schema.table_constraints AS tc
JOIN information_schema.key_column_usage AS kcu
    ON tc.constraint_name = kcu.constraint_name
JOIN information_schema.constraint_column_usage AS ccu
    ON ccu.constraint_name = tc.constraint_name
WHERE tc.constraint_type = 'FOREIGN KEY';

-- Test 3: Verify indexes exist
SELECT indexname, tablename FROM pg_indexes
WHERE schemaname = 'public';
```

### 9.2 Constraint Tests

```sql
-- Test: Budget month constraint (should fail)
INSERT INTO budgets (owner_id, month, year)
VALUES ('00000000-0000-0000-0000-000000000001', 12, 2024);
-- Expected: ERROR: new row violates check constraint "chk_budgets_month"

-- Test: Account type constraint (should fail)
INSERT INTO accounts (owner_id, name, account_type, color_hex)
VALUES ('00000000-0000-0000-0000-000000000001', 'Test', 'invalid', '#FFFFFF');
-- Expected: ERROR: new row violates check constraint "chk_accounts_type"

-- Test: Unique budget per month/year (should fail on second insert)
INSERT INTO budgets (owner_id, month, year) VALUES ($user_id, 0, 2024);
INSERT INTO budgets (owner_id, month, year) VALUES ($user_id, 0, 2024);
-- Expected: ERROR: duplicate key value violates unique constraint "uq_budgets_owner_month_year"
```

### 9.3 Cascade Delete Tests

```sql
-- Test: Deleting user cascades to all related data
BEGIN;
    -- Setup
    INSERT INTO users (id, email, password_hash) VALUES ($user_id, 'test@test.com', 'hash');
    INSERT INTO budgets (id, owner_id, month, year) VALUES ($budget_id, $user_id, 0, 2024);
    INSERT INTO categories (id, budget_id, name) VALUES ($category_id, $budget_id, 'Food');
    INSERT INTO accounts (id, owner_id, name, account_type, color_hex) VALUES ($account_id, $user_id, 'Checking', 'checking', '#000000');
    INSERT INTO transactions (category_id, account_id, amount, transaction_date) VALUES ($category_id, $account_id, 100, NOW());

    -- Delete user
    DELETE FROM users WHERE id = $user_id;

    -- Verify cascade
    SELECT COUNT(*) FROM budgets WHERE owner_id = $user_id; -- Should be 0
    SELECT COUNT(*) FROM accounts WHERE owner_id = $user_id; -- Should be 0
    SELECT COUNT(*) FROM categories WHERE budget_id = $budget_id; -- Should be 0
    SELECT COUNT(*) FROM transactions WHERE category_id = $category_id; -- Should be 0
ROLLBACK;
```

### 9.4 Performance Baseline Tests

```sql
-- After populating with test data, verify query plans use indexes
EXPLAIN (ANALYZE, BUFFERS)
SELECT * FROM budgets WHERE owner_id = $1 AND month = 0 AND year = 2024;
-- Expected: Index Scan using uq_budgets_owner_month_year

EXPLAIN (ANALYZE, BUFFERS)
SELECT * FROM transactions WHERE category_id = $1 ORDER BY transaction_date DESC LIMIT 20;
-- Expected: Index Scan using idx_transactions_category_date
```

---

## Appendix A: Complete Schema Diagram (ASCII)

```
+-------------------+       +--------------------+
|      users        |       |  refresh_tokens    |
+-------------------+       +--------------------+
| id (PK, UUID)     |<------| user_id (FK)       |
| email             |       | id (PK, UUID)      |
| password_hash     |       | token_hash         |
| full_name         |       | expires_at         |
| created_at        |       | revoked_at         |
| updated_at        |       | created_at         |
+-------------------+       +--------------------+
        |
        | 1:N
        v
+-------------------+       +--------------------+
|     budgets       |       |     accounts       |
+-------------------+       +--------------------+
| id (PK, UUID)     |       | id (PK, UUID)      |
| owner_id (FK)     |       | owner_id (FK) ---->|--- users
| month             |       | name               |
| year              |       | account_type       |
| total_income      |       | balance            |
| savings_rate      |       | color_hex          |
| created_at        |       | created_at         |
| updated_at        |       | updated_at         |
+-------------------+       +--------------------+
        |                           |
        | 1:N                       |
        v                           |
+-------------------+               |
|   categories      |               |
+-------------------+               |
| id (PK, UUID)     |               |
| budget_id (FK)    |               |
| name              |               |
| allocated_amount  |               |
| color_hex         |               |
| created_at        |               |
| updated_at        |               |
+-------------------+               |
        |                           |
        | 1:N                       | (optional)
        v                           v
+---------------------------------------+
|            transactions               |
+---------------------------------------+
| id (PK, UUID)                         |
| category_id (FK) -------> categories  |
| account_id (FK, nullable) -> accounts |
| amount                                |
| transaction_type                      |
| transaction_date                      |
| description                           |
| created_at                            |
| updated_at                            |
+---------------------------------------+

+-------------------+
|   audit_logs      |
+-------------------+
| id (PK, BIGINT)   |
| user_id (FK)      |--- users (nullable)
| entity_type       |
| entity_id         |
| action            |
| old_values (JSONB)|
| new_values (JSONB)|
| created_at        |
+-------------------+
```

---

## Appendix B: Migration File Naming Convention

SQLx requires timestamp-prefixed migration files:

```
Format: {timestamp}_{description}.sql
Example: 20260111143022_create_budgets.sql
```

Generate timestamp: `date +%Y%m%d%H%M%S`

Recommended migration file names:
1. `20260111000001_create_refresh_tokens.sql`
2. `20260111000002_create_budgets.sql`
3. `20260111000003_create_accounts.sql`
4. `20260111000004_create_categories.sql`
5. `20260111000005_create_transactions.sql`
6. `20260111000006_create_audit_logs.sql`
7. `20260111000007_create_updated_at_triggers.sql`

---

## Appendix C: Rust Model Stubs

These models will need to be added to `src/models.rs`:

```rust
// Budget model
#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Budget {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub month: i16,
    pub year: i16,
    pub total_income: BigDecimal, // or rust_decimal::Decimal
    pub savings_rate: BigDecimal,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// Category model
#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Category {
    pub id: Uuid,
    pub budget_id: Uuid,
    pub name: String,
    pub allocated_amount: BigDecimal,
    pub color_hex: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// Account model
#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Account {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub name: String,
    pub account_type: String, // or custom enum
    pub balance: BigDecimal,
    pub color_hex: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// Transaction model
#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Transaction {
    pub id: Uuid,
    pub category_id: Uuid,
    pub account_id: Option<Uuid>,
    pub amount: BigDecimal,
    pub transaction_type: String, // or custom enum
    pub transaction_date: DateTime<Utc>,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// RefreshToken model
#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct RefreshToken {
    pub id: Uuid,
    pub user_id: Uuid,
    pub token_hash: String,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub user_agent: Option<String>,
    pub ip_address: Option<IpAddr>,
}
```

---

## Summary Checklist

Before implementing:
- [ ] Review this document with stakeholders
- [ ] Confirm UUID vs VARCHAR decision
- [ ] Confirm transaction_date as TIMESTAMPTZ vs BIGINT
- [ ] Decide on audit_logs inclusion (optional)
- [ ] Set up test database for migration testing

During implementation:
- [ ] Run each migration on test database first
- [ ] Verify constraint enforcement
- [ ] Test cascade deletes
- [ ] Benchmark query performance with EXPLAIN ANALYZE
- [ ] Update Rust models in `src/models.rs`

After implementation:
- [ ] Document any deviations from this plan
- [ ] Update CLAUDE.md with new table information
- [ ] Create seed data scripts for development
