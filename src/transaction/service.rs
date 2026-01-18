use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use super::models::{
    CreateTransactionDto, Transaction, TransactionFilters, TransactionType, UpdateTransactionDto,
};
use crate::errors::AppError;

/// Service layer for transaction business logic.
/// CRITICAL: All balance updates must be atomic to prevent data inconsistency.
pub struct TransactionService;

/// Indicates whether to apply or reverse a balance effect
#[derive(Debug, Clone, Copy)]
enum BalanceOperation {
    Apply,
    Reverse,
}

impl TransactionService {
    /// Create a transaction with atomic balance update.
    /// CRITICAL: This operation MUST be atomic.
    pub async fn create_transaction(
        pool: &PgPool,
        user_id: Uuid,
        dto: CreateTransactionDto,
    ) -> Result<Transaction, AppError> {
        // Start a database transaction
        let mut tx = pool
            .begin()
            .await
            .map_err(|e| AppError::InternalError(e.to_string()))?;

        // 1. Verify user owns the category's budget
        let category_valid = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS(
                SELECT 1 FROM categories c
                JOIN budgets b ON c.budget_id = b.id
                WHERE c.id = $1 AND b.owner_id = $2
            )
            "#,
        )
        .bind(dto.category_id)
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?;

        if !category_valid {
            return Err(AppError::NotFound(
                "Category not found or access denied".to_string(),
            ));
        }

        // 2. If account_id provided, verify user owns it
        if let Some(account_id) = dto.account_id {
            let account_valid = sqlx::query_scalar::<_, bool>(
                r#"
                SELECT EXISTS(
                    SELECT 1 FROM accounts
                    WHERE id = $1 AND owner_id = $2
                )
                "#,
            )
            .bind(account_id)
            .bind(user_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| AppError::InternalError(e.to_string()))?;

            if !account_valid {
                return Err(AppError::NotFound(
                    "Account not found or access denied".to_string(),
                ));
            }
        }

        // 3. Insert the transaction
        let transaction_type_str = dto.transaction_type.as_str();

        let transaction = sqlx::query_as::<_, Transaction>(
            r#"
            INSERT INTO transactions
                (category_id, account_id, amount, transaction_date, description, transaction_type)
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING id, category_id, account_id, amount, transaction_date, description,
                      transaction_type, created_at, updated_at
            "#,
        )
        .bind(dto.category_id)
        .bind(dto.account_id)
        .bind(dto.amount)
        .bind(dto.transaction_date)
        .bind(&dto.description)
        .bind(transaction_type_str)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?;

        // 4. Update account balance if account_id is present
        if let Some(account_id) = dto.account_id {
            Self::update_account_balance(
                &mut tx,
                account_id,
                dto.amount,
                dto.transaction_type,
                BalanceOperation::Apply,
            )
            .await?;
        }

        // 5. Commit the transaction
        tx.commit()
            .await
            .map_err(|e| AppError::InternalError(e.to_string()))?;

        Ok(transaction)
    }

    /// Delete a transaction with atomic balance restoration.
    /// CRITICAL: Must restore account balance before deleting.
    pub async fn delete_transaction(
        pool: &PgPool,
        user_id: Uuid,
        transaction_id: Uuid,
    ) -> Result<(), AppError> {
        let mut tx = pool
            .begin()
            .await
            .map_err(|e| AppError::InternalError(e.to_string()))?;

        // 1. Fetch and lock the transaction row
        let transaction = sqlx::query_as::<_, Transaction>(
            r#"
            SELECT t.id, t.category_id, t.account_id, t.amount, t.transaction_date,
                   t.description, t.transaction_type, t.created_at, t.updated_at
            FROM transactions t
            JOIN categories c ON t.category_id = c.id
            JOIN budgets b ON c.budget_id = b.id
            WHERE t.id = $1 AND b.owner_id = $2
            FOR UPDATE OF t
            "#,
        )
        .bind(transaction_id)
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?
        .ok_or_else(|| AppError::NotFound("Transaction not found".to_string()))?;

        // 2. Restore account balance if account exists
        if let Some(account_id) = transaction.account_id {
            // Lock and check if account still exists
            let account_exists = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM accounts WHERE id = $1 FOR UPDATE)",
            )
            .bind(account_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| AppError::InternalError(e.to_string()))?;

            if account_exists {
                Self::update_account_balance(
                    &mut tx,
                    account_id,
                    transaction.amount,
                    transaction.get_type(),
                    BalanceOperation::Reverse,
                )
                .await?;
            }
        }

        // 3. Delete the transaction
        sqlx::query("DELETE FROM transactions WHERE id = $1")
            .bind(transaction_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::InternalError(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| AppError::InternalError(e.to_string()))?;

        Ok(())
    }

    /// Update a transaction with atomic balance adjustments.
    /// COMPLEX SCENARIOS:
    /// 1. Amount change only: adjust difference
    /// 2. Account change: reverse old account, apply to new account
    /// 3. Type change: reverse old effect, apply new effect
    pub async fn update_transaction(
        pool: &PgPool,
        user_id: Uuid,
        transaction_id: Uuid,
        dto: UpdateTransactionDto,
    ) -> Result<Transaction, AppError> {
        let mut tx = pool
            .begin()
            .await
            .map_err(|e| AppError::InternalError(e.to_string()))?;

        // 1. Fetch and lock the existing transaction
        let old_transaction = sqlx::query_as::<_, Transaction>(
            r#"
            SELECT t.id, t.category_id, t.account_id, t.amount, t.transaction_date,
                   t.description, t.transaction_type, t.created_at, t.updated_at
            FROM transactions t
            JOIN categories c ON t.category_id = c.id
            JOIN budgets b ON c.budget_id = b.id
            WHERE t.id = $1 AND b.owner_id = $2
            FOR UPDATE OF t
            "#,
        )
        .bind(transaction_id)
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?
        .ok_or_else(|| AppError::NotFound("Transaction not found".to_string()))?;

        // 2. Validate new category if changing
        let new_category_id = dto.category_id.unwrap_or(old_transaction.category_id);
        if dto.category_id.is_some() {
            let category_valid = sqlx::query_scalar::<_, bool>(
                r#"
                SELECT EXISTS(
                    SELECT 1 FROM categories c
                    JOIN budgets b ON c.budget_id = b.id
                    WHERE c.id = $1 AND b.owner_id = $2
                )
                "#,
            )
            .bind(new_category_id)
            .bind(user_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| AppError::InternalError(e.to_string()))?;

            if !category_valid {
                return Err(AppError::NotFound(
                    "New category not found or access denied".to_string(),
                ));
            }
        }

        // 3. Validate and determine new account
        let new_account_id = match &dto.account_id {
            Some(Some(id)) => {
                let account_valid = sqlx::query_scalar::<_, bool>(
                    "SELECT EXISTS(SELECT 1 FROM accounts WHERE id = $1 AND owner_id = $2)",
                )
                .bind(id)
                .bind(user_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(|e| AppError::InternalError(e.to_string()))?;

                if !account_valid {
                    return Err(AppError::NotFound(
                        "New account not found or access denied".to_string(),
                    ));
                }
                Some(*id)
            }
            Some(None) => None,                 // Explicitly set to NULL
            None => old_transaction.account_id, // Keep existing
        };

        // Determine final values
        let new_amount = dto.amount.unwrap_or(old_transaction.amount);
        let new_type = dto.transaction_type.unwrap_or(old_transaction.get_type());
        let new_date = dto
            .transaction_date
            .unwrap_or(old_transaction.transaction_date);
        let new_description = dto
            .description
            .or_else(|| old_transaction.description.clone());

        // 4. CRITICAL: Handle balance adjustments
        Self::handle_balance_update_for_modification(
            &mut tx,
            &old_transaction,
            new_account_id,
            new_amount,
            new_type,
        )
        .await?;

        // 5. Build and execute update query
        let new_type_str = new_type.as_str();

        let updated = sqlx::query_as::<_, Transaction>(
            r#"
            UPDATE transactions SET
                category_id = $2,
                account_id = $3,
                amount = $4,
                transaction_date = $5,
                description = $6,
                transaction_type = $7,
                updated_at = NOW()
            WHERE id = $1
            RETURNING id, category_id, account_id, amount, transaction_date, description,
                      transaction_type, created_at, updated_at
            "#,
        )
        .bind(transaction_id)
        .bind(new_category_id)
        .bind(new_account_id)
        .bind(new_amount)
        .bind(new_date)
        .bind(&new_description)
        .bind(new_type_str)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?;

        tx.commit()
            .await
            .map_err(|e| AppError::InternalError(e.to_string()))?;

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
                    .await
                    .map_err(|e| AppError::InternalError(e.to_string()))?;

                // Calculate the net change
                let old_effect = Self::calculate_balance_effect(old_amount, old_type);
                let new_effect = Self::calculate_balance_effect(new_amount, new_type);
                let net_change = new_effect - old_effect;

                if net_change != Decimal::ZERO {
                    sqlx::query(
                        "UPDATE accounts SET balance = balance + $1, updated_at = NOW() WHERE id = $2",
                    )
                    .bind(net_change)
                    .bind(account_id)
                    .execute(&mut **tx)
                    .await
                    .map_err(|e| AppError::InternalError(e.to_string()))?;
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
                    .await
                    .map_err(|e| AppError::InternalError(e.to_string()))?;

                Self::update_account_balance(
                    tx,
                    old_acc,
                    old_amount,
                    old_type,
                    BalanceOperation::Reverse,
                )
                .await?;
            }

            // Apply effect to new account
            if let Some(new_acc) = new_account_id {
                sqlx::query("SELECT 1 FROM accounts WHERE id = $1 FOR UPDATE")
                    .bind(new_acc)
                    .execute(&mut **tx)
                    .await
                    .map_err(|e| AppError::InternalError(e.to_string()))?;

                Self::update_account_balance(
                    tx,
                    new_acc,
                    new_amount,
                    new_type,
                    BalanceOperation::Apply,
                )
                .await?;
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
                "UPDATE accounts SET balance = balance + $1, updated_at = NOW() WHERE id = $2",
            )
            .bind(adjustment)
            .bind(account_id)
            .execute(&mut **tx)
            .await
            .map_err(|e| AppError::InternalError(e.to_string()))?;
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
            SELECT t.id, t.category_id, t.account_id, t.amount, t.transaction_date,
                   t.description, t.transaction_type, t.created_at, t.updated_at
            FROM transactions t
            JOIN categories c ON t.category_id = c.id
            JOIN budgets b ON c.budget_id = b.id
            WHERE t.id = $1 AND b.owner_id = $2
            "#,
        )
        .bind(transaction_id)
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?
        .ok_or_else(|| AppError::NotFound("Transaction not found".to_string()))
    }

    /// List transactions with filters
    pub async fn list_transactions(
        pool: &PgPool,
        user_id: Uuid,
        filters: &TransactionFilters,
    ) -> Result<(Vec<Transaction>, i64), AppError> {
        let limit = filters.limit.min(100);
        let offset = filters.offset;

        // Execute list query with filters
        let transactions = sqlx::query_as::<_, Transaction>(
            r#"
            SELECT t.id, t.category_id, t.account_id, t.amount, t.transaction_date,
                   t.description, t.transaction_type, t.created_at, t.updated_at
            FROM transactions t
            JOIN categories c ON t.category_id = c.id
            JOIN budgets b ON c.budget_id = b.id
            WHERE b.owner_id = $1
              AND ($2::timestamptz IS NULL OR t.transaction_date >= $2)
              AND ($3::timestamptz IS NULL OR t.transaction_date <= $3)
              AND ($4::uuid IS NULL OR t.category_id = $4)
              AND ($5::uuid IS NULL OR t.account_id = $5)
              AND ($6::text IS NULL OR t.transaction_type = $6)
            ORDER BY t.transaction_date DESC, t.created_at DESC
            LIMIT $7 OFFSET $8
            "#,
        )
        .bind(user_id)
        .bind(filters.start_date)
        .bind(filters.end_date)
        .bind(filters.category_id)
        .bind(filters.account_id)
        .bind(&filters.transaction_type)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?;

        // Execute count query
        let total = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)
            FROM transactions t
            JOIN categories c ON t.category_id = c.id
            JOIN budgets b ON c.budget_id = b.id
            WHERE b.owner_id = $1
              AND ($2::timestamptz IS NULL OR t.transaction_date >= $2)
              AND ($3::timestamptz IS NULL OR t.transaction_date <= $3)
              AND ($4::uuid IS NULL OR t.category_id = $4)
              AND ($5::uuid IS NULL OR t.account_id = $5)
              AND ($6::text IS NULL OR t.transaction_type = $6)
            "#,
        )
        .bind(user_id)
        .bind(filters.start_date)
        .bind(filters.end_date)
        .bind(filters.category_id)
        .bind(filters.account_id)
        .bind(&filters.transaction_type)
        .fetch_one(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?;

        Ok((transactions, total))
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
            "#,
        )
        .bind(category_id)
        .bind(user_id)
        .fetch_one(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?;

        if !category_valid {
            return Err(AppError::NotFound(
                "Category not found or access denied".to_string(),
            ));
        }

        sqlx::query_as::<_, Transaction>(
            r#"
            SELECT id, category_id, account_id, amount, transaction_date, description,
                   transaction_type, created_at, updated_at
            FROM transactions
            WHERE category_id = $1
            ORDER BY transaction_date DESC, created_at DESC
            "#,
        )
        .bind(category_id)
        .fetch_all(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))
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
            "#,
        )
        .bind(&category_ids)
        .bind(user_id)
        .fetch_one(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?;

        if valid_count != category_ids.len() as i64 {
            return Err(AppError::NotFound(
                "One or more categories not found or access denied".to_string(),
            ));
        }

        sqlx::query_as::<_, Transaction>(
            r#"
            SELECT id, category_id, account_id, amount, transaction_date, description,
                   transaction_type, created_at, updated_at
            FROM transactions
            WHERE category_id = ANY($1)
            ORDER BY transaction_date DESC, created_at DESC
            "#,
        )
        .bind(&category_ids)
        .fetch_all(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))
    }
}
