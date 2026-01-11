# Transaction Management Feature - Implementation Plan

## Overview

This document provides a comprehensive implementation plan for the Transaction Management feature (Feature 5) in the BudgetFlow backend. This is the **most critical feature** due to the atomic account balance updates required when creating, updating, or deleting transactions.

**Project Location:** `/Users/madizhanbyrtayev/Desktop/projects/next/be-rust`

---

## 1. Database Migration

### 1.1 Migration File

Create migration: `migrations/YYYYMMDDHHMMSS_create_transactions_table.sql`

```sql
-- Create transactions table
CREATE TABLE IF NOT EXISTS transactions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    category_id UUID NOT NULL REFERENCES categories(id) ON DELETE CASCADE,
    account_id UUID REFERENCES accounts(id) ON DELETE SET NULL,
    amount NUMERIC(12,2) NOT NULL CHECK (amount > 0),
    date TIMESTAMPTZ NOT NULL,
    description VARCHAR(200),
    type VARCHAR(10) NOT NULL DEFAULT 'expense' CHECK (type IN ('expense', 'income', 'transfer')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Indexes for query performance
CREATE INDEX idx_transactions_category ON transactions(category_id);
CREATE INDEX idx_transactions_account ON transactions(account_id);
CREATE INDEX idx_transactions_date ON transactions(date DESC);
CREATE INDEX idx_transactions_type ON transactions(type);

-- Composite index for common filter patterns
CREATE INDEX idx_transactions_date_range ON transactions(date, category_id, account_id);

-- Trigger for updated_at
CREATE OR REPLACE FUNCTION update_transactions_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER transactions_updated_at_trigger
    BEFORE UPDATE ON transactions
    FOR EACH ROW
    EXECUTE FUNCTION update_transactions_updated_at();
```

### 1.2 Prerequisites

The following tables must exist before this migration:
- `categories` table (with `id UUID PRIMARY KEY`, linked to `budgets`)
- `accounts` table (with `id UUID PRIMARY KEY`, `balance NUMERIC(12,2)`, `owner_id UUID`)

---

## 2. File Structure

```
src/
├── main.rs                 # Add transaction routes
├── lib.rs                  # Export transactions module
├── models.rs               # Existing models
├── errors.rs               # Add NotFound error variant
├── auth.rs                 # Existing auth (extract middleware)
└── transactions/
    ├── mod.rs              # Module exports
    ├── models.rs           # Transaction domain models and DTOs
    ├── handlers.rs         # HTTP handlers
    ├── service.rs          # Business logic with atomic operations
    └── repository.rs       # Database operations
```

---

## 3. Error Handling Updates

### 3.1 Enhanced AppError (`src/errors.rs`)

```rust
use actix_web::{HttpResponse, ResponseError};
use serde::Serialize;
use std::fmt;

#[derive(Debug)]
pub enum AppError {
    ValidationError(String),
    Unauthorized(String),
    Forbidden(String),           // NEW: For authorization failures
    NotFound(String),            // NEW: For missing resources
    Conflict(String),
    InternalError(String),
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
    message: String,
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::ValidationError(msg) => write!(f, "Validation error: {}", msg),
            AppError::Unauthorized(msg) => write!(f, "Unauthorized: {}", msg),
            AppError::Forbidden(msg) => write!(f, "Forbidden: {}", msg),
            AppError::NotFound(msg) => write!(f, "Not found: {}", msg),
            AppError::Conflict(msg) => write!(f, "Conflict: {}", msg),
            AppError::InternalError(msg) => write!(f, "Internal error: {}", msg),
        }
    }
}

impl ResponseError for AppError {
    fn error_response(&self) -> HttpResponse {
        let (status, error_type, message) = match self {
            AppError::ValidationError(msg) => (
                actix_web::http::StatusCode::BAD_REQUEST,
                "VALIDATION_ERROR",
                msg.clone(),
            ),
            AppError::Unauthorized(msg) => (
                actix_web::http::StatusCode::UNAUTHORIZED,
                "UNAUTHORIZED",
                msg.clone(),
            ),
            AppError::Forbidden(msg) => (
                actix_web::http::StatusCode::FORBIDDEN,
                "FORBIDDEN",
                msg.clone(),
            ),
            AppError::NotFound(msg) => (
                actix_web::http::StatusCode::NOT_FOUND,
                "NOT_FOUND",
                msg.clone(),
            ),
            AppError::Conflict(msg) => (
                actix_web::http::StatusCode::CONFLICT,
                "CONFLICT",
                msg.clone(),
            ),
            AppError::InternalError(msg) => (
                actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                msg.clone(),
            ),
        };

        HttpResponse::build(status).json(ErrorResponse {
            error: error_type.to_string(),
            message,
        })
    }
}

// Convenience conversions
impl From<sqlx::Error> for AppError {
    fn from(err: sqlx::Error) -> Self {
        match err {
            sqlx::Error::RowNotFound => AppError::NotFound("Resource not found".to_string()),
            _ => AppError::InternalError(err.to_string()),
        }
    }
}
```

---

## 4. Transaction Models (`src/transactions/models.rs`)

```rust
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use validator::Validate;

/// Transaction type enum for type safety
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "VARCHAR", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum TransactionType {
    Expense,
    Income,
    Transfer,
}

impl TransactionType {
    /// Returns the balance multiplier for this transaction type
    /// - Expense: -1 (decreases balance)
    /// - Income: +1 (increases balance)
    /// - Transfer: 0 (handled separately for source/destination)
    pub fn balance_multiplier(&self) -> Decimal {
        match self {
            TransactionType::Expense => Decimal::new(-1, 0),
            TransactionType::Income => Decimal::new(1, 0),
            TransactionType::Transfer => Decimal::ZERO,
        }
    }
}

/// Database model for transactions
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Transaction {
    pub id: Uuid,
    pub category_id: Uuid,
    pub account_id: Option<Uuid>,
    pub amount: Decimal,
    pub date: DateTime<Utc>,
    pub description: Option<String>,
    #[sqlx(rename = "type")]
    pub transaction_type: String,  // SQLx reads as String, we convert
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Transaction {
    pub fn get_type(&self) -> TransactionType {
        match self.transaction_type.as_str() {
            "income" => TransactionType::Income,
            "transfer" => TransactionType::Transfer,
            _ => TransactionType::Expense,
        }
    }
}

/// DTO for creating a transaction
#[derive(Debug, Deserialize, Validate)]
pub struct CreateTransactionDto {
    pub category_id: Uuid,
    pub account_id: Option<Uuid>,

    #[validate(custom(function = "validate_positive_amount"))]
    pub amount: Decimal,

    pub date: DateTime<Utc>,

    #[validate(length(max = 200, message = "Description cannot exceed 200 characters"))]
    pub description: Option<String>,

    #[serde(default = "default_transaction_type")]
    pub transaction_type: TransactionType,
}

fn default_transaction_type() -> TransactionType {
    TransactionType::Expense
}

fn validate_positive_amount(amount: &Decimal) -> Result<(), validator::ValidationError> {
    if *amount <= Decimal::ZERO {
        return Err(validator::ValidationError::new("amount_must_be_positive"));
    }
    Ok(())
}

/// DTO for updating a transaction (all fields optional)
#[derive(Debug, Deserialize, Validate)]
pub struct UpdateTransactionDto {
    pub category_id: Option<Uuid>,
    pub account_id: Option<Option<Uuid>>,  // None = don't update, Some(None) = set to NULL

    #[validate(custom(function = "validate_optional_positive_amount"))]
    pub amount: Option<Decimal>,

    pub date: Option<DateTime<Utc>>,

    #[validate(length(max = 200, message = "Description cannot exceed 200 characters"))]
    pub description: Option<String>,

    pub transaction_type: Option<TransactionType>,
}

fn validate_optional_positive_amount(amount: &Option<Decimal>) -> Result<(), validator::ValidationError> {
    if let Some(amt) = amount {
        if *amt <= Decimal::ZERO {
            return Err(validator::ValidationError::new("amount_must_be_positive"));
        }
    }
    Ok(())
}

/// Response DTO for transaction
#[derive(Debug, Serialize)]
pub struct TransactionResponseDto {
    pub id: Uuid,
    pub category_id: Uuid,
    pub account_id: Option<Uuid>,
    pub amount: Decimal,
    pub date: DateTime<Utc>,
    pub description: Option<String>,
    pub transaction_type: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<Transaction> for TransactionResponseDto {
    fn from(t: Transaction) -> Self {
        Self {
            id: t.id,
            category_id: t.category_id,
            account_id: t.account_id,
            amount: t.amount,
            date: t.date,
            description: t.description,
            transaction_type: t.transaction_type,
            created_at: t.created_at,
            updated_at: t.updated_at,
        }
    }
}

/// Query parameters for listing transactions
#[derive(Debug, Deserialize)]
pub struct TransactionFilters {
    pub start_date: Option<DateTime<Utc>>,
    pub end_date: Option<DateTime<Utc>>,
    pub category_id: Option<Uuid>,
    pub account_id: Option<Uuid>,
    pub transaction_type: Option<TransactionType>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// Request body for fetching transactions by multiple categories
#[derive(Debug, Deserialize)]
pub struct CategoriesQueryDto {
    pub category_ids: Vec<Uuid>,
}

/// Paginated response wrapper
#[derive(Debug, Serialize)]
pub struct PaginatedResponse<T> {
    pub data: Vec<T>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}
```

### 4.1 Add rust_decimal to Cargo.toml

```toml
[dependencies]
# ... existing dependencies
rust_decimal = { version = "1.33", features = ["serde", "db-postgres"] }
```

Also update sqlx features:
```toml
sqlx = { version = "0.8.3", features = [
    "postgres",
    "runtime-tokio",
    "tls-native-tls",
    "uuid",
    "chrono",
    "macros",
    "rust_decimal"  # ADD THIS
] }
```

---

## 5. Transaction Service - Atomic Operations (`src/transactions/service.rs`)

This is the **most critical** file - handles all atomic balance updates.

```rust
use chrono::Utc;
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use crate::errors::AppError;
use super::models::{
    CreateTransactionDto, Transaction, TransactionFilters,
    TransactionType, UpdateTransactionDto,
};

pub struct TransactionService;

impl TransactionService {
    /// Create a transaction with atomic balance update
    ///
    /// CRITICAL: This operation MUST be atomic. If the transaction insert
    /// succeeds but balance update fails, we have data inconsistency.
    pub async fn create_transaction(
        pool: &PgPool,
        user_id: Uuid,
        dto: CreateTransactionDto,
    ) -> Result<Transaction, AppError> {
        // Start a database transaction
        let mut tx = pool.begin().await?;

        // 1. Verify user owns the category's budget
        let category_valid = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS(
                SELECT 1 FROM categories c
                JOIN budgets b ON c.budget_id = b.id
                WHERE c.id = $1 AND b.owner_id = $2
            )
            "#
        )
        .bind(dto.category_id)
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await?;

        if !category_valid {
            return Err(AppError::Forbidden(
                "Category not found or access denied".to_string()
            ));
        }

        // 2. If account_id provided, verify user owns it and lock the row
        if let Some(account_id) = dto.account_id {
            let account_valid = sqlx::query_scalar::<_, bool>(
                r#"
                SELECT EXISTS(
                    SELECT 1 FROM accounts
                    WHERE id = $1 AND owner_id = $2
                )
                "#
            )
            .bind(account_id)
            .bind(user_id)
            .fetch_one(&mut *tx)
            .await?;

            if !account_valid {
                return Err(AppError::Forbidden(
                    "Account not found or access denied".to_string()
                ));
            }
        }

        // 3. Insert the transaction
        let transaction_type_str = match dto.transaction_type {
            TransactionType::Expense => "expense",
            TransactionType::Income => "income",
            TransactionType::Transfer => "transfer",
        };

        let transaction = sqlx::query_as::<_, Transaction>(
            r#"
            INSERT INTO transactions
                (category_id, account_id, amount, date, description, type)
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING id, category_id, account_id, amount, date, description,
                      type as transaction_type, created_at, updated_at
            "#
        )
        .bind(dto.category_id)
        .bind(dto.account_id)
        .bind(dto.amount)
        .bind(dto.date)
        .bind(&dto.description)
        .bind(transaction_type_str)
        .fetch_one(&mut *tx)
        .await?;

        // 4. Update account balance if account_id is present
        if let Some(account_id) = dto.account_id {
            Self::update_account_balance(
                &mut tx,
                account_id,
                dto.amount,
                dto.transaction_type,
                BalanceOperation::Apply,
            ).await?;
        }

        // 5. Commit the transaction
        tx.commit().await?;

        Ok(transaction)
    }

    /// Delete a transaction with atomic balance restoration
    ///
    /// CRITICAL: Must restore account balance before deleting.
    /// Uses SELECT FOR UPDATE to prevent concurrent modifications.
    pub async fn delete_transaction(
        pool: &PgPool,
        user_id: Uuid,
        transaction_id: Uuid,
    ) -> Result<(), AppError> {
        let mut tx = pool.begin().await?;

        // 1. Fetch and lock the transaction row
        let transaction = sqlx::query_as::<_, Transaction>(
            r#"
            SELECT t.id, t.category_id, t.account_id, t.amount, t.date,
                   t.description, t.type as transaction_type, t.created_at, t.updated_at
            FROM transactions t
            JOIN categories c ON t.category_id = c.id
            JOIN budgets b ON c.budget_id = b.id
            WHERE t.id = $1 AND b.owner_id = $2
            FOR UPDATE OF t
            "#
        )
        .bind(transaction_id)
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| AppError::NotFound("Transaction not found".to_string()))?;

        // 2. Restore account balance if account exists
        if let Some(account_id) = transaction.account_id {
            // Lock the account row
            let account_exists = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM accounts WHERE id = $1 FOR UPDATE)"
            )
            .bind(account_id)
            .fetch_one(&mut *tx)
            .await?;

            if account_exists {
                Self::update_account_balance(
                    &mut tx,
                    account_id,
                    transaction.amount,
                    transaction.get_type(),
                    BalanceOperation::Reverse,
                ).await?;
            }
        }

        // 3. Delete the transaction
        sqlx::query("DELETE FROM transactions WHERE id = $1")
            .bind(transaction_id)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;

        Ok(())
    }

    /// Update a transaction with atomic balance adjustments
    ///
    /// COMPLEX SCENARIOS:
    /// 1. Amount change only: adjust difference
    /// 2. Account change: reverse old account, apply to new account
    /// 3. Type change: reverse old effect, apply new effect
    /// 4. Combination of above
    pub async fn update_transaction(
        pool: &PgPool,
        user_id: Uuid,
        transaction_id: Uuid,
        dto: UpdateTransactionDto,
    ) -> Result<Transaction, AppError> {
        let mut tx = pool.begin().await?;

        // 1. Fetch and lock the existing transaction
        let old_transaction = sqlx::query_as::<_, Transaction>(
            r#"
            SELECT t.id, t.category_id, t.account_id, t.amount, t.date,
                   t.description, t.type as transaction_type, t.created_at, t.updated_at
            FROM transactions t
            JOIN categories c ON t.category_id = c.id
            JOIN budgets b ON c.budget_id = b.id
            WHERE t.id = $1 AND b.owner_id = $2
            FOR UPDATE OF t
            "#
        )
        .bind(transaction_id)
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| AppError::NotFound("Transaction not found".to_string()))?;

        // 2. Validate new category if changing
        if let Some(new_category_id) = dto.category_id {
            let category_valid = sqlx::query_scalar::<_, bool>(
                r#"
                SELECT EXISTS(
                    SELECT 1 FROM categories c
                    JOIN budgets b ON c.budget_id = b.id
                    WHERE c.id = $1 AND b.owner_id = $2
                )
                "#
            )
            .bind(new_category_id)
            .bind(user_id)
            .fetch_one(&mut *tx)
            .await?;

            if !category_valid {
                return Err(AppError::Forbidden(
                    "New category not found or access denied".to_string()
                ));
            }
        }

        // 3. Validate new account if changing
        let new_account_id = match &dto.account_id {
            Some(Some(id)) => {
                let account_valid = sqlx::query_scalar::<_, bool>(
                    "SELECT EXISTS(SELECT 1 FROM accounts WHERE id = $1 AND owner_id = $2)"
                )
                .bind(id)
                .bind(user_id)
                .fetch_one(&mut *tx)
                .await?;

                if !account_valid {
                    return Err(AppError::Forbidden(
                        "New account not found or access denied".to_string()
                    ));
                }
                Some(*id)
            },
            Some(None) => None,  // Explicitly set to NULL
            None => old_transaction.account_id,  // Keep existing
        };

        // Determine final values
        let new_amount = dto.amount.unwrap_or(old_transaction.amount);
        let new_type = dto.transaction_type.unwrap_or(old_transaction.get_type());

        // 4. CRITICAL: Handle balance adjustments
        Self::handle_balance_update_for_modification(
            &mut tx,
            &old_transaction,
            new_account_id,
            new_amount,
            new_type,
        ).await?;

        // 5. Build and execute update query
        let new_type_str = match new_type {
            TransactionType::Expense => "expense",
            TransactionType::Income => "income",
            TransactionType::Transfer => "transfer",
        };

        let updated = sqlx::query_as::<_, Transaction>(
            r#"
            UPDATE transactions SET
                category_id = COALESCE($1, category_id),
                account_id = $2,
                amount = $3,
                date = COALESCE($4, date),
                description = COALESCE($5, description),
                type = $6,
                updated_at = NOW()
            WHERE id = $7
            RETURNING id, category_id, account_id, amount, date, description,
                      type as transaction_type, created_at, updated_at
            "#
        )
        .bind(dto.category_id)
        .bind(new_account_id)
        .bind(new_amount)
        .bind(dto.date)
        .bind(&dto.description)
        .bind(new_type_str)
        .bind(transaction_id)
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(updated)
    }

    /// Handle the complex balance update scenarios during modification
    async fn handle_balance_update_for_modification(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        old: &Transaction,
        new_account_id: Option<Uuid>,
        new_amount: Decimal,
        new_type: TransactionType,
    ) -> Result<(), AppError> {
        let old_type = old.get_type();
        let old_account_id = old.account_id;
        let old_amount = old.amount;

        // Scenario 1: Same account, amount/type may have changed
        if old_account_id == new_account_id {
            if let Some(account_id) = old_account_id {
                // Lock the account
                sqlx::query("SELECT 1 FROM accounts WHERE id = $1 FOR UPDATE")
                    .bind(account_id)
                    .execute(&mut **tx)
                    .await?;

                // Calculate the net change
                let old_effect = Self::calculate_balance_effect(old_amount, old_type);
                let new_effect = Self::calculate_balance_effect(new_amount, new_type);
                let net_change = new_effect - old_effect;

                if net_change != Decimal::ZERO {
                    sqlx::query(
                        "UPDATE accounts SET balance = balance + $1, updated_at = NOW() WHERE id = $2"
                    )
                    .bind(net_change)
                    .bind(account_id)
                    .execute(&mut **tx)
                    .await?;
                }
            }
        }
        // Scenario 2: Account changed (including to/from NULL)
        else {
            // Reverse effect on old account
            if let Some(old_acc) = old_account_id {
                sqlx::query("SELECT 1 FROM accounts WHERE id = $1 FOR UPDATE")
                    .bind(old_acc)
                    .execute(&mut **tx)
                    .await?;

                Self::update_account_balance(
                    tx,
                    old_acc,
                    old_amount,
                    old_type,
                    BalanceOperation::Reverse,
                ).await?;
            }

            // Apply effect to new account
            if let Some(new_acc) = new_account_id {
                sqlx::query("SELECT 1 FROM accounts WHERE id = $1 FOR UPDATE")
                    .bind(new_acc)
                    .execute(&mut **tx)
                    .await?;

                Self::update_account_balance(
                    tx,
                    new_acc,
                    new_amount,
                    new_type,
                    BalanceOperation::Apply,
                ).await?;
            }
        }

        Ok(())
    }

    /// Calculate the effect on account balance
    fn calculate_balance_effect(amount: Decimal, transaction_type: TransactionType) -> Decimal {
        match transaction_type {
            TransactionType::Expense => -amount,
            TransactionType::Income => amount,
            TransactionType::Transfer => Decimal::ZERO,
        }
    }

    /// Update account balance atomically
    async fn update_account_balance(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        account_id: Uuid,
        amount: Decimal,
        transaction_type: TransactionType,
        operation: BalanceOperation,
    ) -> Result<(), AppError> {
        let effect = Self::calculate_balance_effect(amount, transaction_type);

        let adjustment = match operation {
            BalanceOperation::Apply => effect,
            BalanceOperation::Reverse => -effect,
        };

        if adjustment != Decimal::ZERO {
            sqlx::query(
                "UPDATE accounts SET balance = balance + $1, updated_at = NOW() WHERE id = $2"
            )
            .bind(adjustment)
            .bind(account_id)
            .execute(&mut **tx)
            .await?;
        }

        Ok(())
    }

    /// Get a single transaction by ID
    pub async fn get_transaction(
        pool: &PgPool,
        user_id: Uuid,
        transaction_id: Uuid,
    ) -> Result<Transaction, AppError> {
        sqlx::query_as::<_, Transaction>(
            r#"
            SELECT t.id, t.category_id, t.account_id, t.amount, t.date,
                   t.description, t.type as transaction_type, t.created_at, t.updated_at
            FROM transactions t
            JOIN categories c ON t.category_id = c.id
            JOIN budgets b ON c.budget_id = b.id
            WHERE t.id = $1 AND b.owner_id = $2
            "#
        )
        .bind(transaction_id)
        .bind(user_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound("Transaction not found".to_string()))
    }

    /// List transactions with filters
    pub async fn list_transactions(
        pool: &PgPool,
        user_id: Uuid,
        filters: TransactionFilters,
    ) -> Result<(Vec<Transaction>, i64), AppError> {
        let limit = filters.limit.unwrap_or(50).min(100);
        let offset = filters.offset.unwrap_or(0);

        // Build dynamic query
        let mut query = String::from(
            r#"
            SELECT t.id, t.category_id, t.account_id, t.amount, t.date,
                   t.description, t.type as transaction_type, t.created_at, t.updated_at
            FROM transactions t
            JOIN categories c ON t.category_id = c.id
            JOIN budgets b ON c.budget_id = b.id
            WHERE b.owner_id = $1
            "#
        );

        let mut count_query = String::from(
            r#"
            SELECT COUNT(*)
            FROM transactions t
            JOIN categories c ON t.category_id = c.id
            JOIN budgets b ON c.budget_id = b.id
            WHERE b.owner_id = $1
            "#
        );

        let mut param_idx = 2;
        let mut conditions = Vec::new();

        if filters.start_date.is_some() {
            conditions.push(format!("t.date >= ${}", param_idx));
            param_idx += 1;
        }
        if filters.end_date.is_some() {
            conditions.push(format!("t.date <= ${}", param_idx));
            param_idx += 1;
        }
        if filters.category_id.is_some() {
            conditions.push(format!("t.category_id = ${}", param_idx));
            param_idx += 1;
        }
        if filters.account_id.is_some() {
            conditions.push(format!("t.account_id = ${}", param_idx));
            param_idx += 1;
        }
        if filters.transaction_type.is_some() {
            conditions.push(format!("t.type = ${}", param_idx));
        }

        for cond in &conditions {
            query.push_str(" AND ");
            query.push_str(cond);
            count_query.push_str(" AND ");
            count_query.push_str(cond);
        }

        query.push_str(" ORDER BY t.date DESC, t.created_at DESC");
        query.push_str(&format!(" LIMIT {} OFFSET {}", limit, offset));

        // Execute with dynamic binding (simplified - actual impl would use sqlx::QueryBuilder)
        // For now, using a more straightforward approach with optional bindings:

        let transactions = Self::execute_list_query(pool, user_id, &filters, limit, offset).await?;
        let total = Self::execute_count_query(pool, user_id, &filters).await?;

        Ok((transactions, total))
    }

    /// Execute list query with filters
    async fn execute_list_query(
        pool: &PgPool,
        user_id: Uuid,
        filters: &TransactionFilters,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Transaction>, AppError> {
        // Using a simpler approach with COALESCE for optional filters
        let type_str = filters.transaction_type.map(|t| match t {
            TransactionType::Expense => "expense",
            TransactionType::Income => "income",
            TransactionType::Transfer => "transfer",
        });

        sqlx::query_as::<_, Transaction>(
            r#"
            SELECT t.id, t.category_id, t.account_id, t.amount, t.date,
                   t.description, t.type as transaction_type, t.created_at, t.updated_at
            FROM transactions t
            JOIN categories c ON t.category_id = c.id
            JOIN budgets b ON c.budget_id = b.id
            WHERE b.owner_id = $1
              AND ($2::timestamptz IS NULL OR t.date >= $2)
              AND ($3::timestamptz IS NULL OR t.date <= $3)
              AND ($4::uuid IS NULL OR t.category_id = $4)
              AND ($5::uuid IS NULL OR t.account_id = $5)
              AND ($6::text IS NULL OR t.type = $6)
            ORDER BY t.date DESC, t.created_at DESC
            LIMIT $7 OFFSET $8
            "#
        )
        .bind(user_id)
        .bind(filters.start_date)
        .bind(filters.end_date)
        .bind(filters.category_id)
        .bind(filters.account_id)
        .bind(type_str)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    /// Execute count query with filters
    async fn execute_count_query(
        pool: &PgPool,
        user_id: Uuid,
        filters: &TransactionFilters,
    ) -> Result<i64, AppError> {
        let type_str = filters.transaction_type.map(|t| match t {
            TransactionType::Expense => "expense",
            TransactionType::Income => "income",
            TransactionType::Transfer => "transfer",
        });

        sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)
            FROM transactions t
            JOIN categories c ON t.category_id = c.id
            JOIN budgets b ON c.budget_id = b.id
            WHERE b.owner_id = $1
              AND ($2::timestamptz IS NULL OR t.date >= $2)
              AND ($3::timestamptz IS NULL OR t.date <= $3)
              AND ($4::uuid IS NULL OR t.category_id = $4)
              AND ($5::uuid IS NULL OR t.account_id = $5)
              AND ($6::text IS NULL OR t.type = $6)
            "#
        )
        .bind(user_id)
        .bind(filters.start_date)
        .bind(filters.end_date)
        .bind(filters.category_id)
        .bind(filters.account_id)
        .bind(type_str)
        .fetch_one(pool)
        .await
        .map_err(Into::into)
    }

    /// Get transactions by category
    pub async fn get_by_category(
        pool: &PgPool,
        user_id: Uuid,
        category_id: Uuid,
    ) -> Result<Vec<Transaction>, AppError> {
        // Verify user owns the category
        let category_valid = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS(
                SELECT 1 FROM categories c
                JOIN budgets b ON c.budget_id = b.id
                WHERE c.id = $1 AND b.owner_id = $2
            )
            "#
        )
        .bind(category_id)
        .bind(user_id)
        .fetch_one(pool)
        .await?;

        if !category_valid {
            return Err(AppError::Forbidden(
                "Category not found or access denied".to_string()
            ));
        }

        sqlx::query_as::<_, Transaction>(
            r#"
            SELECT id, category_id, account_id, amount, date, description,
                   type as transaction_type, created_at, updated_at
            FROM transactions
            WHERE category_id = $1
            ORDER BY date DESC, created_at DESC
            "#
        )
        .bind(category_id)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }

    /// Get transactions by multiple categories
    pub async fn get_by_categories(
        pool: &PgPool,
        user_id: Uuid,
        category_ids: Vec<Uuid>,
    ) -> Result<Vec<Transaction>, AppError> {
        if category_ids.is_empty() {
            return Ok(Vec::new());
        }

        // Verify user owns all categories
        let valid_count = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(DISTINCT c.id)
            FROM categories c
            JOIN budgets b ON c.budget_id = b.id
            WHERE c.id = ANY($1) AND b.owner_id = $2
            "#
        )
        .bind(&category_ids)
        .bind(user_id)
        .fetch_one(pool)
        .await?;

        if valid_count != category_ids.len() as i64 {
            return Err(AppError::Forbidden(
                "One or more categories not found or access denied".to_string()
            ));
        }

        sqlx::query_as::<_, Transaction>(
            r#"
            SELECT id, category_id, account_id, amount, date, description,
                   type as transaction_type, created_at, updated_at
            FROM transactions
            WHERE category_id = ANY($1)
            ORDER BY date DESC, created_at DESC
            "#
        )
        .bind(&category_ids)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
    }
}

/// Indicates whether to apply or reverse a balance effect
#[derive(Debug, Clone, Copy)]
enum BalanceOperation {
    Apply,
    Reverse,
}
```

---

## 6. HTTP Handlers (`src/transactions/handlers.rs`)

```rust
use actix_web::{delete, get, patch, post, web, HttpRequest, HttpResponse};
use sqlx::PgPool;
use validator::Validate;

use crate::auth::decode_token;
use crate::errors::AppError;
use super::models::{
    CategoriesQueryDto, CreateTransactionDto, PaginatedResponse,
    TransactionFilters, TransactionResponseDto, UpdateTransactionDto,
};
use super::service::TransactionService;

/// Extract user ID from JWT token
fn extract_user_id(req: &HttpRequest, jwt_secret: &str) -> Result<uuid::Uuid, AppError> {
    let token = req
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .ok_or_else(|| AppError::Unauthorized("Missing Authorization header".to_string()))?;

    let claims = decode_token(token, jwt_secret)?;
    Ok(claims.sub)
}

/// GET /transactions
/// List transactions with optional filters
#[get("/transactions")]
pub async fn list_transactions(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    jwt_secret: web::Data<String>,
    query: web::Query<TransactionFilters>,
) -> Result<HttpResponse, AppError> {
    let user_id = extract_user_id(&req, jwt_secret.get_ref())?;

    let (transactions, total) = TransactionService::list_transactions(
        pool.get_ref(),
        user_id,
        query.into_inner(),
    ).await?;

    let response: Vec<TransactionResponseDto> = transactions
        .into_iter()
        .map(Into::into)
        .collect();

    Ok(HttpResponse::Ok().json(PaginatedResponse {
        data: response,
        total,
        limit: 50, // Default limit
        offset: 0,
    }))
}

/// GET /transactions/{id}
/// Get a specific transaction by ID
#[get("/transactions/{id}")]
pub async fn get_transaction(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    jwt_secret: web::Data<String>,
    path: web::Path<uuid::Uuid>,
) -> Result<HttpResponse, AppError> {
    let user_id = extract_user_id(&req, jwt_secret.get_ref())?;
    let transaction_id = path.into_inner();

    let transaction = TransactionService::get_transaction(
        pool.get_ref(),
        user_id,
        transaction_id,
    ).await?;

    Ok(HttpResponse::Ok().json(TransactionResponseDto::from(transaction)))
}

/// GET /transactions/category/{category_id}
/// Get all transactions for a category
#[get("/transactions/category/{category_id}")]
pub async fn get_by_category(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    jwt_secret: web::Data<String>,
    path: web::Path<uuid::Uuid>,
) -> Result<HttpResponse, AppError> {
    let user_id = extract_user_id(&req, jwt_secret.get_ref())?;
    let category_id = path.into_inner();

    let transactions = TransactionService::get_by_category(
        pool.get_ref(),
        user_id,
        category_id,
    ).await?;

    let response: Vec<TransactionResponseDto> = transactions
        .into_iter()
        .map(Into::into)
        .collect();

    Ok(HttpResponse::Ok().json(response))
}

/// POST /transactions/categories
/// Get transactions for multiple categories
#[post("/transactions/categories")]
pub async fn get_by_categories(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    jwt_secret: web::Data<String>,
    body: web::Json<CategoriesQueryDto>,
) -> Result<HttpResponse, AppError> {
    let user_id = extract_user_id(&req, jwt_secret.get_ref())?;

    let transactions = TransactionService::get_by_categories(
        pool.get_ref(),
        user_id,
        body.into_inner().category_ids,
    ).await?;

    let response: Vec<TransactionResponseDto> = transactions
        .into_iter()
        .map(Into::into)
        .collect();

    Ok(HttpResponse::Ok().json(response))
}

/// POST /transactions
/// Create a new transaction (atomically updates account balance)
#[post("/transactions")]
pub async fn create_transaction(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    jwt_secret: web::Data<String>,
    body: web::Json<CreateTransactionDto>,
) -> Result<HttpResponse, AppError> {
    let user_id = extract_user_id(&req, jwt_secret.get_ref())?;

    // Validate input
    body.validate()
        .map_err(|e| AppError::ValidationError(e.to_string()))?;

    let transaction = TransactionService::create_transaction(
        pool.get_ref(),
        user_id,
        body.into_inner(),
    ).await?;

    Ok(HttpResponse::Created().json(TransactionResponseDto::from(transaction)))
}

/// PATCH /transactions/{id}
/// Update a transaction (handles balance adjustments atomically)
#[patch("/transactions/{id}")]
pub async fn update_transaction(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    jwt_secret: web::Data<String>,
    path: web::Path<uuid::Uuid>,
    body: web::Json<UpdateTransactionDto>,
) -> Result<HttpResponse, AppError> {
    let user_id = extract_user_id(&req, jwt_secret.get_ref())?;
    let transaction_id = path.into_inner();

    // Validate input
    body.validate()
        .map_err(|e| AppError::ValidationError(e.to_string()))?;

    let transaction = TransactionService::update_transaction(
        pool.get_ref(),
        user_id,
        transaction_id,
        body.into_inner(),
    ).await?;

    Ok(HttpResponse::Ok().json(TransactionResponseDto::from(transaction)))
}

/// DELETE /transactions/{id}
/// Delete a transaction (atomically restores account balance)
#[delete("/transactions/{id}")]
pub async fn delete_transaction(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    jwt_secret: web::Data<String>,
    path: web::Path<uuid::Uuid>,
) -> Result<HttpResponse, AppError> {
    let user_id = extract_user_id(&req, jwt_secret.get_ref())?;
    let transaction_id = path.into_inner();

    TransactionService::delete_transaction(
        pool.get_ref(),
        user_id,
        transaction_id,
    ).await?;

    Ok(HttpResponse::NoContent().finish())
}
```

---

## 7. Module Registration

### 7.1 `src/transactions/mod.rs`

```rust
pub mod handlers;
pub mod models;
pub mod service;

pub use handlers::*;
```

### 7.2 Update `src/lib.rs`

```rust
pub mod auth;
pub mod errors;
pub mod models;
pub mod transactions;
```

### 7.3 Update `src/main.rs`

```rust
mod auth;
mod errors;
mod models;
mod transactions;

use actix_web::{get, web, App, HttpResponse, HttpServer, Responder};
use dotenv::dotenv;
use sqlx::postgres::PgPoolOptions;
use std::env;

#[get("/health")]
async fn health_check() -> impl Responder {
    HttpResponse::Ok().json(serde_json::json!({"status": "healthy"}))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();
    env_logger::init();

    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let jwt_secret = env::var("JWT_SECRET").expect("JWT_SECRET must be set");

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("Failed to create pool");

    println!("Starting server at http://0.0.0.0:8080");

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .app_data(web::Data::new(jwt_secret.clone()))
            .service(health_check)
            // Auth routes
            .service(auth::register)
            .service(auth::login)
            .service(auth::me)
            // Transaction routes
            .service(transactions::list_transactions)
            .service(transactions::get_transaction)
            .service(transactions::get_by_category)
            .service(transactions::get_by_categories)
            .service(transactions::create_transaction)
            .service(transactions::update_transaction)
            .service(transactions::delete_transaction)
    })
    .bind(("0.0.0.0", 8080))?
    .run()
    .await
}
```

---

## 8. Concurrent Update Handling (Row Locking Strategy)

### 8.1 Why Row Locking is Critical

Without proper locking, concurrent operations can cause:
- **Lost Updates**: Two updates read the same balance, both modify, last one wins
- **Phantom Reads**: Balance calculated between transaction insert and account update
- **Dirty Reads**: Reading uncommitted balance changes

### 8.2 Locking Strategy

```sql
-- Always lock in consistent order to prevent deadlocks:
-- 1. First lock transactions (if updating existing)
-- 2. Then lock accounts (sorted by ID to prevent deadlock)

-- Example: Updating a transaction with account change
BEGIN;

-- Lock the transaction row
SELECT * FROM transactions WHERE id = $1 FOR UPDATE;

-- Lock BOTH account rows (old and new) in ID order to prevent deadlock
SELECT * FROM accounts
WHERE id IN ($old_account_id, $new_account_id)
ORDER BY id
FOR UPDATE;

-- Perform updates
UPDATE accounts SET balance = balance + $restore_amount WHERE id = $old_account_id;
UPDATE accounts SET balance = balance - $apply_amount WHERE id = $new_account_id;
UPDATE transactions SET ... WHERE id = $1;

COMMIT;
```

### 8.3 Deadlock Prevention Rules

1. **Always acquire locks in the same order**: Sort account IDs before locking
2. **Keep transactions short**: Minimize time between BEGIN and COMMIT
3. **Use appropriate isolation level**: READ COMMITTED (default) is sufficient with FOR UPDATE

---

## 9. Test Cases (`tests/transaction_tests.rs`)

```rust
use serde_json::{json, Value};
use rust_decimal::Decimal;

mod common;
use common::TestApp;

// ============================================
// BALANCE INTEGRITY TESTS (CRITICAL)
// ============================================

#[actix_rt::test]
async fn test_create_expense_decreases_balance() {
    let app = TestApp::new().await;
    let (token, _) = app.create_test_user().await;

    // Create account with initial balance 1000
    let account = app.create_account(&token, json!({
        "name": "Checking",
        "type": "checking",
        "balance": 1000.00,
        "color_hex": "#3B82F6"
    })).await;

    let budget = app.create_budget(&token).await;
    let category = app.create_category(&token, &budget["id"]).await;

    // Create expense transaction of 250
    app.post_auth("/transactions", &token, &json!({
        "category_id": category["id"],
        "account_id": account["id"],
        "amount": 250.00,
        "date": "2024-01-15T12:00:00Z",
        "transaction_type": "expense"
    })).await;

    // Verify balance decreased to 750
    let updated_account = app.get_auth(&format!("/accounts/{}", account["id"]), &token).await;
    assert_eq!(updated_account["balance"], 750.00);
}

#[actix_rt::test]
async fn test_create_income_increases_balance() {
    let app = TestApp::new().await;
    let (token, _) = app.create_test_user().await;

    let account = app.create_account(&token, json!({
        "name": "Checking",
        "type": "checking",
        "balance": 500.00,
        "color_hex": "#3B82F6"
    })).await;

    let budget = app.create_budget(&token).await;
    let category = app.create_category(&token, &budget["id"]).await;

    // Create income transaction of 300
    app.post_auth("/transactions", &token, &json!({
        "category_id": category["id"],
        "account_id": account["id"],
        "amount": 300.00,
        "date": "2024-01-15T12:00:00Z",
        "transaction_type": "income"
    })).await;

    // Verify balance increased to 800
    let updated_account = app.get_auth(&format!("/accounts/{}", account["id"]), &token).await;
    assert_eq!(updated_account["balance"], 800.00);
}

#[actix_rt::test]
async fn test_delete_expense_restores_balance() {
    let app = TestApp::new().await;
    let (token, _) = app.create_test_user().await;

    let account = app.create_account(&token, json!({
        "name": "Checking",
        "type": "checking",
        "balance": 1000.00,
        "color_hex": "#3B82F6"
    })).await;

    let budget = app.create_budget(&token).await;
    let category = app.create_category(&token, &budget["id"]).await;

    // Create expense (balance: 1000 - 400 = 600)
    let transaction = app.post_auth("/transactions", &token, &json!({
        "category_id": category["id"],
        "account_id": account["id"],
        "amount": 400.00,
        "date": "2024-01-15T12:00:00Z",
        "transaction_type": "expense"
    })).await;

    // Delete transaction (balance should restore to 1000)
    app.delete_auth(&format!("/transactions/{}", transaction["id"]), &token).await;

    let updated_account = app.get_auth(&format!("/accounts/{}", account["id"]), &token).await;
    assert_eq!(updated_account["balance"], 1000.00);
}

#[actix_rt::test]
async fn test_delete_income_restores_balance() {
    let app = TestApp::new().await;
    let (token, _) = app.create_test_user().await;

    let account = app.create_account(&token, json!({
        "name": "Checking",
        "type": "checking",
        "balance": 500.00,
        "color_hex": "#3B82F6"
    })).await;

    let budget = app.create_budget(&token).await;
    let category = app.create_category(&token, &budget["id"]).await;

    // Create income (balance: 500 + 200 = 700)
    let transaction = app.post_auth("/transactions", &token, &json!({
        "category_id": category["id"],
        "account_id": account["id"],
        "amount": 200.00,
        "date": "2024-01-15T12:00:00Z",
        "transaction_type": "income"
    })).await;

    // Delete transaction (balance should restore to 500)
    app.delete_auth(&format!("/transactions/{}", transaction["id"]), &token).await;

    let updated_account = app.get_auth(&format!("/accounts/{}", account["id"]), &token).await;
    assert_eq!(updated_account["balance"], 500.00);
}

// ============================================
// UPDATE BALANCE CHANGE TESTS
// ============================================

#[actix_rt::test]
async fn test_update_amount_adjusts_balance_difference() {
    let app = TestApp::new().await;
    let (token, _) = app.create_test_user().await;

    let account = app.create_account(&token, json!({
        "name": "Checking",
        "type": "checking",
        "balance": 1000.00,
        "color_hex": "#3B82F6"
    })).await;

    let budget = app.create_budget(&token).await;
    let category = app.create_category(&token, &budget["id"]).await;

    // Create expense of 200 (balance: 800)
    let transaction = app.post_auth("/transactions", &token, &json!({
        "category_id": category["id"],
        "account_id": account["id"],
        "amount": 200.00,
        "date": "2024-01-15T12:00:00Z",
        "transaction_type": "expense"
    })).await;

    // Update to 300 (should deduct additional 100, balance: 700)
    app.patch_auth(&format!("/transactions/{}", transaction["id"]), &token, &json!({
        "amount": 300.00
    })).await;

    let updated_account = app.get_auth(&format!("/accounts/{}", account["id"]), &token).await;
    assert_eq!(updated_account["balance"], 700.00);
}

#[actix_rt::test]
async fn test_update_type_reverses_and_applies() {
    let app = TestApp::new().await;
    let (token, _) = app.create_test_user().await;

    let account = app.create_account(&token, json!({
        "name": "Checking",
        "type": "checking",
        "balance": 1000.00,
        "color_hex": "#3B82F6"
    })).await;

    let budget = app.create_budget(&token).await;
    let category = app.create_category(&token, &budget["id"]).await;

    // Create expense of 100 (balance: 900)
    let transaction = app.post_auth("/transactions", &token, &json!({
        "category_id": category["id"],
        "account_id": account["id"],
        "amount": 100.00,
        "date": "2024-01-15T12:00:00Z",
        "transaction_type": "expense"
    })).await;

    // Change to income (reverse -100, apply +100 = +200 net, balance: 1100)
    app.patch_auth(&format!("/transactions/{}", transaction["id"]), &token, &json!({
        "transaction_type": "income"
    })).await;

    let updated_account = app.get_auth(&format!("/accounts/{}", account["id"]), &token).await;
    assert_eq!(updated_account["balance"], 1100.00);
}

#[actix_rt::test]
async fn test_update_account_transfers_balance() {
    let app = TestApp::new().await;
    let (token, _) = app.create_test_user().await;

    let account1 = app.create_account(&token, json!({
        "name": "Checking",
        "type": "checking",
        "balance": 1000.00,
        "color_hex": "#3B82F6"
    })).await;

    let account2 = app.create_account(&token, json!({
        "name": "Savings",
        "type": "savings",
        "balance": 500.00,
        "color_hex": "#10B981"
    })).await;

    let budget = app.create_budget(&token).await;
    let category = app.create_category(&token, &budget["id"]).await;

    // Create expense on account1 (balance1: 800)
    let transaction = app.post_auth("/transactions", &token, &json!({
        "category_id": category["id"],
        "account_id": account1["id"],
        "amount": 200.00,
        "date": "2024-01-15T12:00:00Z",
        "transaction_type": "expense"
    })).await;

    // Move to account2 (restore account1 to 1000, apply to account2: 300)
    app.patch_auth(&format!("/transactions/{}", transaction["id"]), &token, &json!({
        "account_id": account2["id"]
    })).await;

    let updated_account1 = app.get_auth(&format!("/accounts/{}", account1["id"]), &token).await;
    let updated_account2 = app.get_auth(&format!("/accounts/{}", account2["id"]), &token).await;

    assert_eq!(updated_account1["balance"], 1000.00);
    assert_eq!(updated_account2["balance"], 300.00);
}

#[actix_rt::test]
async fn test_update_remove_account() {
    let app = TestApp::new().await;
    let (token, _) = app.create_test_user().await;

    let account = app.create_account(&token, json!({
        "name": "Checking",
        "type": "checking",
        "balance": 1000.00,
        "color_hex": "#3B82F6"
    })).await;

    let budget = app.create_budget(&token).await;
    let category = app.create_category(&token, &budget["id"]).await;

    // Create expense (balance: 700)
    let transaction = app.post_auth("/transactions", &token, &json!({
        "category_id": category["id"],
        "account_id": account["id"],
        "amount": 300.00,
        "date": "2024-01-15T12:00:00Z",
        "transaction_type": "expense"
    })).await;

    // Remove account from transaction (restore balance to 1000)
    app.patch_auth(&format!("/transactions/{}", transaction["id"]), &token, &json!({
        "account_id": null
    })).await;

    let updated_account = app.get_auth(&format!("/accounts/{}", account["id"]), &token).await;
    assert_eq!(updated_account["balance"], 1000.00);
}

// ============================================
// TRANSACTION WITHOUT ACCOUNT TESTS
// ============================================

#[actix_rt::test]
async fn test_create_transaction_without_account() {
    let app = TestApp::new().await;
    let (token, _) = app.create_test_user().await;

    let budget = app.create_budget(&token).await;
    let category = app.create_category(&token, &budget["id"]).await;

    // Create expense without account
    let response = app.post_auth("/transactions", &token, &json!({
        "category_id": category["id"],
        "amount": 100.00,
        "date": "2024-01-15T12:00:00Z",
        "transaction_type": "expense"
    })).await;

    assert_eq!(response.status(), 201);
    let transaction: Value = response.json().await;
    assert!(transaction["account_id"].is_null());
}

// ============================================
// AUTHORIZATION TESTS
// ============================================

#[actix_rt::test]
async fn test_cannot_create_transaction_for_other_users_category() {
    let app = TestApp::new().await;
    let (token1, _) = app.create_test_user_with_email("user1@test.com").await;
    let (token2, _) = app.create_test_user_with_email("user2@test.com").await;

    // User1 creates a budget and category
    let budget = app.create_budget(&token1).await;
    let category = app.create_category(&token1, &budget["id"]).await;

    // User2 tries to create transaction in User1's category
    let response = app.post_auth("/transactions", &token2, &json!({
        "category_id": category["id"],
        "amount": 100.00,
        "date": "2024-01-15T12:00:00Z",
        "transaction_type": "expense"
    })).await;

    assert_eq!(response.status(), 403);
}

#[actix_rt::test]
async fn test_cannot_create_transaction_with_other_users_account() {
    let app = TestApp::new().await;
    let (token1, _) = app.create_test_user_with_email("user1@test.com").await;
    let (token2, _) = app.create_test_user_with_email("user2@test.com").await;

    // User1 creates an account
    let account1 = app.create_account(&token1, json!({
        "name": "User1 Account",
        "type": "checking",
        "balance": 1000.00,
        "color_hex": "#3B82F6"
    })).await;

    // User2 creates their own budget/category
    let budget2 = app.create_budget(&token2).await;
    let category2 = app.create_category(&token2, &budget2["id"]).await;

    // User2 tries to use User1's account
    let response = app.post_auth("/transactions", &token2, &json!({
        "category_id": category2["id"],
        "account_id": account1["id"],
        "amount": 100.00,
        "date": "2024-01-15T12:00:00Z",
        "transaction_type": "expense"
    })).await;

    assert_eq!(response.status(), 403);
}

#[actix_rt::test]
async fn test_cannot_access_other_users_transaction() {
    let app = TestApp::new().await;
    let (token1, _) = app.create_test_user_with_email("user1@test.com").await;
    let (token2, _) = app.create_test_user_with_email("user2@test.com").await;

    // User1 creates a transaction
    let budget = app.create_budget(&token1).await;
    let category = app.create_category(&token1, &budget["id"]).await;
    let transaction = app.post_auth("/transactions", &token1, &json!({
        "category_id": category["id"],
        "amount": 100.00,
        "date": "2024-01-15T12:00:00Z"
    })).await;
    let tx_body: Value = transaction.json().await;

    // User2 tries to access User1's transaction
    let response = app.get_auth(&format!("/transactions/{}", tx_body["id"]), &token2).await;
    assert_eq!(response.status(), 404);
}

// ============================================
// CONCURRENT UPDATE TESTS
// ============================================

#[actix_rt::test]
async fn test_concurrent_balance_updates_are_serialized() {
    let app = TestApp::new().await;
    let (token, _) = app.create_test_user().await;

    let account = app.create_account(&token, json!({
        "name": "Checking",
        "type": "checking",
        "balance": 1000.00,
        "color_hex": "#3B82F6"
    })).await;

    let budget = app.create_budget(&token).await;
    let category = app.create_category(&token, &budget["id"]).await;

    // Spawn 10 concurrent expense transactions of 50 each
    let mut handles = vec![];
    for _ in 0..10 {
        let app_clone = app.clone();
        let token_clone = token.clone();
        let category_id = category["id"].as_str().unwrap().to_string();
        let account_id = account["id"].as_str().unwrap().to_string();

        handles.push(tokio::spawn(async move {
            app_clone.post_auth("/transactions", &token_clone, &json!({
                "category_id": category_id,
                "account_id": account_id,
                "amount": 50.00,
                "date": "2024-01-15T12:00:00Z",
                "transaction_type": "expense"
            })).await
        }));
    }

    // Wait for all to complete
    for handle in handles {
        handle.await.unwrap();
    }

    // Final balance should be exactly 1000 - (10 * 50) = 500
    let updated_account = app.get_auth(&format!("/accounts/{}", account["id"]), &token).await;
    assert_eq!(updated_account["balance"], 500.00);
}

// ============================================
// VALIDATION TESTS
// ============================================

#[actix_rt::test]
async fn test_reject_negative_amount() {
    let app = TestApp::new().await;
    let (token, _) = app.create_test_user().await;

    let budget = app.create_budget(&token).await;
    let category = app.create_category(&token, &budget["id"]).await;

    let response = app.post_auth("/transactions", &token, &json!({
        "category_id": category["id"],
        "amount": -100.00,
        "date": "2024-01-15T12:00:00Z"
    })).await;

    assert_eq!(response.status(), 400);
}

#[actix_rt::test]
async fn test_reject_zero_amount() {
    let app = TestApp::new().await;
    let (token, _) = app.create_test_user().await;

    let budget = app.create_budget(&token).await;
    let category = app.create_category(&token, &budget["id"]).await;

    let response = app.post_auth("/transactions", &token, &json!({
        "category_id": category["id"],
        "amount": 0,
        "date": "2024-01-15T12:00:00Z"
    })).await;

    assert_eq!(response.status(), 400);
}

#[actix_rt::test]
async fn test_reject_description_too_long() {
    let app = TestApp::new().await;
    let (token, _) = app.create_test_user().await;

    let budget = app.create_budget(&token).await;
    let category = app.create_category(&token, &budget["id"]).await;

    let long_description = "x".repeat(201);

    let response = app.post_auth("/transactions", &token, &json!({
        "category_id": category["id"],
        "amount": 100.00,
        "date": "2024-01-15T12:00:00Z",
        "description": long_description
    })).await;

    assert_eq!(response.status(), 400);
}

// ============================================
// FILTER TESTS
// ============================================

#[actix_rt::test]
async fn test_filter_by_date_range() {
    let app = TestApp::new().await;
    let (token, _) = app.create_test_user().await;

    let budget = app.create_budget(&token).await;
    let category = app.create_category(&token, &budget["id"]).await;

    // Create transactions on different dates
    app.post_auth("/transactions", &token, &json!({
        "category_id": category["id"],
        "amount": 100.00,
        "date": "2024-01-10T12:00:00Z"
    })).await;

    app.post_auth("/transactions", &token, &json!({
        "category_id": category["id"],
        "amount": 200.00,
        "date": "2024-01-15T12:00:00Z"
    })).await;

    app.post_auth("/transactions", &token, &json!({
        "category_id": category["id"],
        "amount": 300.00,
        "date": "2024-01-20T12:00:00Z"
    })).await;

    // Filter for middle date only
    let response = app.get_auth(
        "/transactions?start_date=2024-01-14T00:00:00Z&end_date=2024-01-16T23:59:59Z",
        &token
    ).await;

    let body: Value = response.json().await;
    assert_eq!(body["data"].as_array().unwrap().len(), 1);
    assert_eq!(body["data"][0]["amount"], 200.00);
}

#[actix_rt::test]
async fn test_get_by_multiple_categories() {
    let app = TestApp::new().await;
    let (token, _) = app.create_test_user().await;

    let budget = app.create_budget(&token).await;
    let category1 = app.create_category(&token, &budget["id"]).await;
    let category2 = app.create_category(&token, &budget["id"]).await;
    let category3 = app.create_category(&token, &budget["id"]).await;

    // Create transactions in different categories
    app.post_auth("/transactions", &token, &json!({
        "category_id": category1["id"],
        "amount": 100.00,
        "date": "2024-01-15T12:00:00Z"
    })).await;

    app.post_auth("/transactions", &token, &json!({
        "category_id": category2["id"],
        "amount": 200.00,
        "date": "2024-01-15T12:00:00Z"
    })).await;

    app.post_auth("/transactions", &token, &json!({
        "category_id": category3["id"],
        "amount": 300.00,
        "date": "2024-01-15T12:00:00Z"
    })).await;

    // Get transactions for categories 1 and 2 only
    let response = app.post_auth("/transactions/categories", &token, &json!({
        "category_ids": [category1["id"], category2["id"]]
    })).await;

    let transactions: Vec<Value> = response.json().await;
    assert_eq!(transactions.len(), 2);
}
```

---

## 10. Dependencies to Add (`Cargo.toml`)

```toml
[dependencies]
actix-web = "4.9.0"
serde = { version = "1.0.217", features = ["derive"] }
serde_json = "1.0.138"
tokio = { version = "1.43.0", features = ["full"] }
sqlx = { version = "0.8.3", features = [
    "postgres",
    "runtime-tokio",
    "tls-native-tls",
    "uuid",
    "chrono",
    "macros",
    "rust_decimal"    # ADD THIS
] }
dotenv = "0.15.0"
env_logger = "0.11.6"
argon2 = "0.5.3"
jsonwebtoken = "9.3.0"
validator = { version = "0.20.0", features = ["derive"] }
uuid = { version = "1.12.1", features = ["v4", "serde"] }
chrono = { version = "0.4.39", features = ["serde"] }
rust_decimal = { version = "1.33", features = ["serde"] }  # ADD THIS

[dev-dependencies]
actix-rt = "2.11.0"
bytes = "1.11.0"
once_cell = "1.21.3"
```

---

## 11. Implementation Order

### Phase 1: Foundation
1. Add `rust_decimal` dependency to Cargo.toml
2. Update `src/errors.rs` with `NotFound` and `Forbidden` variants
3. Create migration file for transactions table
4. Run `sqlx migrate run`

### Phase 2: Core Module Structure
1. Create `src/transactions/` directory
2. Create `src/transactions/models.rs` with all DTOs
3. Create `src/transactions/mod.rs`
4. Update `src/lib.rs` to export module

### Phase 3: Service Layer (Critical)
1. Implement `TransactionService::create_transaction` with atomic balance update
2. Implement `TransactionService::delete_transaction` with balance restoration
3. Implement `TransactionService::update_transaction` with complex balance logic
4. Implement read operations (get, list, filter)

### Phase 4: Handlers
1. Create `src/transactions/handlers.rs`
2. Implement all 7 endpoints
3. Register routes in `src/main.rs`

### Phase 5: Testing
1. Unit tests for balance calculation logic
2. Integration tests for all endpoints
3. Concurrency tests for balance integrity
4. Authorization tests

---

## 12. Critical Verification Checklist

Before considering this feature complete:

- [ ] **Atomicity**: All balance updates occur within database transactions
- [ ] **Row Locking**: `FOR UPDATE` used on transactions and accounts during modifications
- [ ] **Authorization**: Every operation verifies user owns the category's budget
- [ ] **Authorization**: Every operation verifies user owns the account (if specified)
- [ ] **Balance Math**: Expense decreases balance, income increases balance
- [ ] **Delete Restoration**: Deleting expense restores balance, deleting income deducts
- [ ] **Update Handling**: Account change reverses old, applies new
- [ ] **Update Handling**: Type change properly reverses and reapplies effect
- [ ] **Update Handling**: Amount change applies only the difference
- [ ] **Concurrent Safety**: Multiple simultaneous operations produce correct final balance
- [ ] **Validation**: Amount > 0 enforced at API and database levels
- [ ] **No Null Dereference**: account_id is optional, all paths handle None

---

## 13. API Quick Reference

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/transactions` | List with filters (query params: start_date, end_date, category_id, account_id, transaction_type, limit, offset) |
| GET | `/transactions/:id` | Get specific transaction |
| GET | `/transactions/category/:categoryId` | Get all transactions for a category |
| POST | `/transactions/categories` | Get transactions for multiple categories (body: { category_ids: UUID[] }) |
| POST | `/transactions` | Create transaction (atomically updates account balance) |
| PATCH | `/transactions/:id` | Update transaction (handles complex balance adjustments) |
| DELETE | `/transactions/:id` | Delete transaction (atomically restores account balance) |

---

## 14. Potential Edge Cases to Handle

1. **Deleted Account**: When an account is deleted (`ON DELETE SET NULL`), transactions should not try to update its balance
2. **Deleted Category**: Transactions cascade delete with category, so no orphans
3. **Concurrent Deletes**: Two requests to delete the same transaction - second should get 404
4. **Very Large Amounts**: NUMERIC(12,2) supports up to 9,999,999,999.99
5. **Timezone Handling**: All dates stored as TIMESTAMPTZ, client sends ISO 8601

---

This plan provides a complete, production-ready implementation of the Transaction Management feature with a focus on data integrity, atomic operations, and comprehensive testing.
