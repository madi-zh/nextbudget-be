use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use super::models::{
    CategorySummaryRow, CreateTransactionDto, SummaryFilters, Transaction, TransactionDetailRow,
    TransactionFilters, TransactionFiltersDetailed, TransactionType, UpdateTransactionDto,
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
    /// For transfers: decreases source account balance, increases destination account balance.
    pub async fn create_transaction(
        pool: &PgPool,
        user_id: Uuid,
        dto: CreateTransactionDto,
    ) -> Result<Transaction, AppError> {
        // Validate transfer constraints
        dto.validate_transfer()
            .map_err(|e| AppError::ValidationError(e.to_string()))?;

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
                    "Source account not found or access denied".to_string(),
                ));
            }
        }

        // 3. If destination_account_id provided (for transfers), verify user owns it
        if let Some(dest_account_id) = dto.destination_account_id {
            let account_valid = sqlx::query_scalar::<_, bool>(
                r#"
                SELECT EXISTS(
                    SELECT 1 FROM accounts
                    WHERE id = $1 AND owner_id = $2
                )
                "#,
            )
            .bind(dest_account_id)
            .bind(user_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| AppError::InternalError(e.to_string()))?;

            if !account_valid {
                return Err(AppError::NotFound(
                    "Destination account not found or access denied".to_string(),
                ));
            }
        }

        // 4. Insert the transaction
        let transaction_type_str = dto.transaction_type.as_str();

        let transaction = sqlx::query_as::<_, Transaction>(
            r#"
            INSERT INTO transactions
                (category_id, account_id, destination_account_id, amount, transaction_date, description, transaction_type)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING id, category_id, account_id, destination_account_id, amount, transaction_date, description,
                      transaction_type, created_at, updated_at
            "#,
        )
        .bind(dto.category_id)
        .bind(dto.account_id)
        .bind(dto.destination_account_id)
        .bind(dto.amount)
        .bind(dto.transaction_date)
        .bind(&dto.description)
        .bind(transaction_type_str)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?;

        // 5. Update account balances
        Self::apply_transaction_balance_effects(
            &mut tx,
            dto.account_id,
            dto.destination_account_id,
            dto.amount,
            dto.transaction_type,
            BalanceOperation::Apply,
        )
        .await?;

        // 6. Commit the transaction
        tx.commit()
            .await
            .map_err(|e| AppError::InternalError(e.to_string()))?;

        Ok(transaction)
    }

    /// Delete a transaction with atomic balance restoration.
    /// CRITICAL: Must restore account balance before deleting.
    /// For transfers: restores both source and destination account balances.
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
            SELECT t.id, t.category_id, t.account_id, t.destination_account_id, t.amount, t.transaction_date,
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

        // 2. Restore account balances (reverse the effects)
        Self::apply_transaction_balance_effects_with_existence_check(
            &mut tx,
            transaction.account_id,
            transaction.destination_account_id,
            transaction.amount,
            transaction.get_type(),
            BalanceOperation::Reverse,
        )
        .await?;

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
    /// 4. Transfer destination change: reverse old destination, apply to new destination
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
            SELECT t.id, t.category_id, t.account_id, t.destination_account_id, t.amount, t.transaction_date,
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

        // 3. Validate and determine new account (source)
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
                        "New source account not found or access denied".to_string(),
                    ));
                }
                Some(*id)
            }
            Some(None) => None,                 // Explicitly set to NULL
            None => old_transaction.account_id, // Keep existing
        };

        // 4. Validate and determine new destination account
        let new_destination_account_id = match &dto.destination_account_id {
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
                        "New destination account not found or access denied".to_string(),
                    ));
                }
                Some(*id)
            }
            Some(None) => None, // Explicitly set to NULL
            None => old_transaction.destination_account_id, // Keep existing
        };

        // Determine final values
        let new_amount = dto.amount.unwrap_or(old_transaction.amount);
        let new_type = dto.transaction_type.unwrap_or(old_transaction.get_type());
        let new_date = dto
            .transaction_date
            .unwrap_or(old_transaction.transaction_date);

        // 5. Validate transfer constraints (before consuming dto.description)
        dto.validate_transfer(new_type, new_account_id, new_destination_account_id)
            .map_err(|e| AppError::ValidationError(e.to_string()))?;

        let new_description = dto
            .description
            .or_else(|| old_transaction.description.clone());

        // 6. CRITICAL: Handle balance adjustments
        Self::handle_balance_update_for_modification_with_destination(
            &mut tx,
            &old_transaction,
            new_account_id,
            new_destination_account_id,
            new_amount,
            new_type,
        )
        .await?;

        // 7. Build and execute update query
        let new_type_str = new_type.as_str();

        let updated = sqlx::query_as::<_, Transaction>(
            r#"
            UPDATE transactions SET
                category_id = $2,
                account_id = $3,
                destination_account_id = $4,
                amount = $5,
                transaction_date = $6,
                description = $7,
                transaction_type = $8,
                updated_at = NOW()
            WHERE id = $1
            RETURNING id, category_id, account_id, destination_account_id, amount, transaction_date, description,
                      transaction_type, created_at, updated_at
            "#,
        )
        .bind(transaction_id)
        .bind(new_category_id)
        .bind(new_account_id)
        .bind(new_destination_account_id)
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

    /// Apply balance effects for a transaction (create/delete)
    /// For transfers: source account decreases, destination account increases
    async fn apply_transaction_balance_effects(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        source_account_id: Option<Uuid>,
        destination_account_id: Option<Uuid>,
        amount: Decimal,
        transaction_type: TransactionType,
        operation: BalanceOperation,
    ) -> Result<(), AppError> {
        match transaction_type {
            TransactionType::Expense => {
                // Expense: only affects source account (decreases balance)
                if let Some(account_id) = source_account_id {
                    Self::update_single_account_balance(tx, account_id, -amount, operation).await?;
                }
            }
            TransactionType::Income => {
                // Income: only affects source account (increases balance)
                if let Some(account_id) = source_account_id {
                    Self::update_single_account_balance(tx, account_id, amount, operation).await?;
                }
            }
            TransactionType::Transfer => {
                // Transfer: source decreases, destination increases
                if let Some(src) = source_account_id {
                    Self::update_single_account_balance(tx, src, -amount, operation).await?;
                }
                if let Some(dst) = destination_account_id {
                    Self::update_single_account_balance(tx, dst, amount, operation).await?;
                }
            }
        }
        Ok(())
    }

    /// Apply balance effects with existence check (for delete operations)
    async fn apply_transaction_balance_effects_with_existence_check(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        source_account_id: Option<Uuid>,
        destination_account_id: Option<Uuid>,
        amount: Decimal,
        transaction_type: TransactionType,
        operation: BalanceOperation,
    ) -> Result<(), AppError> {
        match transaction_type {
            TransactionType::Expense => {
                if let Some(account_id) = source_account_id {
                    if Self::account_exists(tx, account_id).await? {
                        Self::update_single_account_balance(tx, account_id, -amount, operation)
                            .await?;
                    }
                }
            }
            TransactionType::Income => {
                if let Some(account_id) = source_account_id {
                    if Self::account_exists(tx, account_id).await? {
                        Self::update_single_account_balance(tx, account_id, amount, operation)
                            .await?;
                    }
                }
            }
            TransactionType::Transfer => {
                if let Some(src) = source_account_id {
                    if Self::account_exists(tx, src).await? {
                        Self::update_single_account_balance(tx, src, -amount, operation).await?;
                    }
                }
                if let Some(dst) = destination_account_id {
                    if Self::account_exists(tx, dst).await? {
                        Self::update_single_account_balance(tx, dst, amount, operation).await?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Check if an account exists (with lock)
    async fn account_exists(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        account_id: Uuid,
    ) -> Result<bool, AppError> {
        sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM accounts WHERE id = $1 FOR UPDATE)",
        )
        .bind(account_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))
    }

    /// Handle the complex balance update scenarios during modification with destination account support
    async fn handle_balance_update_for_modification_with_destination(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        old: &Transaction,
        new_account_id: Option<Uuid>,
        new_destination_account_id: Option<Uuid>,
        new_amount: Decimal,
        new_type: TransactionType,
    ) -> Result<(), AppError> {
        let old_type = old.get_type();

        // Strategy: Reverse all old effects, then apply all new effects
        // This handles all complex scenarios cleanly

        // Reverse old effects
        Self::apply_transaction_balance_effects_with_existence_check(
            tx,
            old.account_id,
            old.destination_account_id,
            old.amount,
            old_type,
            BalanceOperation::Reverse,
        )
        .await?;

        // Apply new effects
        Self::apply_transaction_balance_effects(
            tx,
            new_account_id,
            new_destination_account_id,
            new_amount,
            new_type,
            BalanceOperation::Apply,
        )
        .await?;

        Ok(())
    }

    /// Update a single account balance atomically
    async fn update_single_account_balance(
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        account_id: Uuid,
        effect: Decimal,
        operation: BalanceOperation,
    ) -> Result<(), AppError> {
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
            SELECT t.id, t.category_id, t.account_id, t.destination_account_id, t.amount, t.transaction_date,
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
            SELECT t.id, t.category_id, t.account_id, t.destination_account_id, t.amount, t.transaction_date,
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
            SELECT id, category_id, account_id, destination_account_id, amount, transaction_date, description,
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
            SELECT id, category_id, account_id, destination_account_id, amount, transaction_date, description,
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

    /// List transactions with detailed account/category info
    pub async fn list_transactions_detailed(
        pool: &PgPool,
        user_id: Uuid,
        filters: &TransactionFiltersDetailed,
    ) -> Result<(Vec<TransactionDetailRow>, i64), AppError> {
        let limit = filters.limit.min(100);
        let offset = filters.offset;

        // Execute list query with JOINs for detailed info (including destination account)
        let transactions = sqlx::query_as::<_, TransactionDetailRow>(
            r#"
            SELECT
                t.id, t.amount, t.transaction_type, t.transaction_date,
                t.description, t.created_at, t.updated_at,
                c.id as category_id, c.name as category_name, c.color_hex as category_color_hex,
                a.id as account_id, a.name as account_name, a.account_type,
                a.color_hex as account_color_hex, a.currency as account_currency,
                da.id as dest_account_id, da.name as dest_account_name, da.account_type as dest_account_type,
                da.color_hex as dest_account_color_hex, da.currency as dest_account_currency
            FROM transactions t
            JOIN categories c ON t.category_id = c.id
            JOIN budgets b ON c.budget_id = b.id
            LEFT JOIN accounts a ON t.account_id = a.id
            LEFT JOIN accounts da ON t.destination_account_id = da.id
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

    /// Get transactions by account ID
    pub async fn get_by_account(
        pool: &PgPool,
        user_id: Uuid,
        account_id: Uuid,
        filters: &TransactionFilters,
    ) -> Result<(Vec<Transaction>, i64), AppError> {
        // Verify user owns the account
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
        .fetch_one(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?;

        if !account_valid {
            return Err(AppError::NotFound(
                "Account not found or access denied".to_string(),
            ));
        }

        let limit = filters.limit.min(100);
        let offset = filters.offset;

        // Execute list query (include transactions where this account is source OR destination)
        let transactions = sqlx::query_as::<_, Transaction>(
            r#"
            SELECT t.id, t.category_id, t.account_id, t.destination_account_id, t.amount, t.transaction_date,
                   t.description, t.transaction_type, t.created_at, t.updated_at
            FROM transactions t
            WHERE (t.account_id = $1 OR t.destination_account_id = $1)
              AND ($2::timestamptz IS NULL OR t.transaction_date >= $2)
              AND ($3::timestamptz IS NULL OR t.transaction_date <= $3)
              AND ($4::uuid IS NULL OR t.category_id = $4)
              AND ($5::text IS NULL OR t.transaction_type = $5)
            ORDER BY t.transaction_date DESC, t.created_at DESC
            LIMIT $6 OFFSET $7
            "#,
        )
        .bind(account_id)
        .bind(filters.start_date)
        .bind(filters.end_date)
        .bind(filters.category_id)
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
            WHERE (t.account_id = $1 OR t.destination_account_id = $1)
              AND ($2::timestamptz IS NULL OR t.transaction_date >= $2)
              AND ($3::timestamptz IS NULL OR t.transaction_date <= $3)
              AND ($4::uuid IS NULL OR t.category_id = $4)
              AND ($5::text IS NULL OR t.transaction_type = $5)
            "#,
        )
        .bind(account_id)
        .bind(filters.start_date)
        .bind(filters.end_date)
        .bind(filters.category_id)
        .bind(&filters.transaction_type)
        .fetch_one(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?;

        Ok((transactions, total))
    }

    /// Get transaction summary with totals and category breakdown
    pub async fn get_summary(
        pool: &PgPool,
        user_id: Uuid,
        filters: &SummaryFilters,
    ) -> Result<(Decimal, Decimal, i64, Vec<CategorySummaryRow>), AppError> {
        // Get total income
        let total_income = sqlx::query_scalar::<_, Option<Decimal>>(
            r#"
            SELECT COALESCE(SUM(t.amount), 0)
            FROM transactions t
            JOIN categories c ON t.category_id = c.id
            JOIN budgets b ON c.budget_id = b.id
            WHERE b.owner_id = $1
              AND t.transaction_type = 'income'
              AND ($2::timestamptz IS NULL OR t.transaction_date >= $2)
              AND ($3::timestamptz IS NULL OR t.transaction_date <= $3)
              AND ($4::uuid IS NULL OR t.account_id = $4)
            "#,
        )
        .bind(user_id)
        .bind(filters.start_date)
        .bind(filters.end_date)
        .bind(filters.account_id)
        .fetch_one(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?
        .unwrap_or(Decimal::ZERO);

        // Get total expenses
        let total_expenses = sqlx::query_scalar::<_, Option<Decimal>>(
            r#"
            SELECT COALESCE(SUM(t.amount), 0)
            FROM transactions t
            JOIN categories c ON t.category_id = c.id
            JOIN budgets b ON c.budget_id = b.id
            WHERE b.owner_id = $1
              AND t.transaction_type = 'expense'
              AND ($2::timestamptz IS NULL OR t.transaction_date >= $2)
              AND ($3::timestamptz IS NULL OR t.transaction_date <= $3)
              AND ($4::uuid IS NULL OR t.account_id = $4)
            "#,
        )
        .bind(user_id)
        .bind(filters.start_date)
        .bind(filters.end_date)
        .bind(filters.account_id)
        .fetch_one(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?
        .unwrap_or(Decimal::ZERO);

        // Get total transaction count
        let transaction_count = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT COUNT(*)
            FROM transactions t
            JOIN categories c ON t.category_id = c.id
            JOIN budgets b ON c.budget_id = b.id
            WHERE b.owner_id = $1
              AND ($2::timestamptz IS NULL OR t.transaction_date >= $2)
              AND ($3::timestamptz IS NULL OR t.transaction_date <= $3)
              AND ($4::uuid IS NULL OR t.account_id = $4)
            "#,
        )
        .bind(user_id)
        .bind(filters.start_date)
        .bind(filters.end_date)
        .bind(filters.account_id)
        .fetch_one(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?;

        // Get category breakdown (expenses only, as that's most useful for spending analysis)
        let by_category = sqlx::query_as::<_, CategorySummaryRow>(
            r#"
            SELECT
                c.id as category_id,
                c.name as category_name,
                c.color_hex as category_color_hex,
                COALESCE(SUM(t.amount), 0) as total_amount,
                COUNT(t.id) as transaction_count
            FROM categories c
            JOIN budgets b ON c.budget_id = b.id
            LEFT JOIN transactions t ON t.category_id = c.id
                AND t.transaction_type = 'expense'
                AND ($2::timestamptz IS NULL OR t.transaction_date >= $2)
                AND ($3::timestamptz IS NULL OR t.transaction_date <= $3)
                AND ($4::uuid IS NULL OR t.account_id = $4)
            WHERE b.owner_id = $1
            GROUP BY c.id, c.name, c.color_hex
            HAVING COUNT(t.id) > 0
            ORDER BY total_amount DESC
            "#,
        )
        .bind(user_id)
        .bind(filters.start_date)
        .bind(filters.end_date)
        .bind(filters.account_id)
        .fetch_all(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?;

        Ok((total_income, total_expenses, transaction_count, by_category))
    }
}
