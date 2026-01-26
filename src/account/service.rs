use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use super::models::{
    Account, AccountsSummary, CreateAccountDto, CurrencySummary, CurrencySummaryRow, SummaryRow,
    UpdateAccountDto, UpdateBalanceDto,
};
use crate::currency::service::CurrencyService;
use crate::errors::AppError;

/// Service layer for account business logic.
pub struct AccountService;

impl AccountService {
    /// List all accounts for a user.
    pub async fn list_accounts(pool: &PgPool, owner_id: Uuid) -> Result<Vec<Account>, AppError> {
        sqlx::query_as::<_, Account>(
            r#"
            SELECT id, owner_id, name, account_type, balance, color_hex, currency, created_at, updated_at
            FROM accounts
            WHERE owner_id = $1
            ORDER BY created_at DESC
            "#,
        )
        .bind(owner_id)
        .fetch_all(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))
    }

    /// Get an account by ID, ensuring the requesting user owns it.
    pub async fn get_account_by_id(
        pool: &PgPool,
        account_id: Uuid,
        owner_id: Uuid,
    ) -> Result<Account, AppError> {
        sqlx::query_as::<_, Account>(
            r#"
            SELECT id, owner_id, name, account_type, balance, color_hex, currency, created_at, updated_at
            FROM accounts
            WHERE id = $1 AND owner_id = $2
            "#,
        )
        .bind(account_id)
        .bind(owner_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?
        .ok_or_else(|| AppError::NotFound("Account not found".to_string()))
    }

    /// Get accounts by type for a user.
    pub async fn get_accounts_by_type(
        pool: &PgPool,
        owner_id: Uuid,
        account_type: &str,
    ) -> Result<Vec<Account>, AppError> {
        // Validate type
        let valid_types = ["checking", "savings", "credit"];
        if !valid_types.contains(&account_type) {
            return Err(AppError::ValidationError(format!(
                "Invalid account type '{}'. Must be one of: {}",
                account_type,
                valid_types.join(", ")
            )));
        }

        sqlx::query_as::<_, Account>(
            r#"
            SELECT id, owner_id, name, account_type, balance, color_hex, currency, created_at, updated_at
            FROM accounts
            WHERE owner_id = $1 AND account_type = $2
            ORDER BY created_at DESC
            "#,
        )
        .bind(owner_id)
        .bind(account_type)
        .fetch_all(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))
    }

    /// Get accounts with financial summary for a user.
    pub async fn get_accounts_summary(
        pool: &PgPool,
        owner_id: Uuid,
    ) -> Result<(Vec<Account>, AccountsSummary, Vec<CurrencySummary>), AppError> {
        // Fetch all accounts
        let accounts = Self::list_accounts(pool, owner_id).await?;

        // Compute overall summary with a single aggregation query
        let summary_row = sqlx::query_as::<_, SummaryRow>(
            r#"
            SELECT
                COALESCE(SUM(CASE WHEN account_type = 'savings' THEN balance ELSE 0 END), 0) as total_savings,
                COALESCE(SUM(CASE WHEN account_type IN ('checking', 'credit') THEN balance ELSE 0 END), 0) as total_spending,
                COUNT(*) as accounts_count
            FROM accounts
            WHERE owner_id = $1
            "#,
        )
        .bind(owner_id)
        .fetch_one(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?;

        let total_savings = summary_row.total_savings.unwrap_or(Decimal::ZERO);
        let total_spending = summary_row.total_spending.unwrap_or(Decimal::ZERO);
        let net_worth = total_savings + total_spending;

        let summary = AccountsSummary {
            total_savings,
            total_spending,
            net_worth,
            accounts_count: summary_row.accounts_count.unwrap_or(0),
        };

        // Compute per-currency summaries
        let currency_rows = sqlx::query_as::<_, CurrencySummaryRow>(
            r#"
            SELECT
                currency,
                COALESCE(SUM(CASE WHEN account_type = 'savings' THEN balance ELSE 0 END), 0) as total_savings,
                COALESCE(SUM(CASE WHEN account_type IN ('checking', 'credit') THEN balance ELSE 0 END), 0) as total_spending,
                COUNT(*) as accounts_count
            FROM accounts
            WHERE owner_id = $1
            GROUP BY currency
            ORDER BY currency ASC
            "#,
        )
        .bind(owner_id)
        .fetch_all(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?;

        let summaries: Vec<CurrencySummary> = currency_rows
            .into_iter()
            .map(|row| {
                let savings = row.total_savings.unwrap_or(Decimal::ZERO);
                let spending = row.total_spending.unwrap_or(Decimal::ZERO);
                CurrencySummary {
                    currency: row.currency,
                    total_savings: savings,
                    total_spending: spending,
                    net_worth: savings + spending,
                    accounts_count: row.accounts_count.unwrap_or(0),
                }
            })
            .collect();

        Ok((accounts, summary, summaries))
    }

    /// Create a new account.
    pub async fn create_account(
        pool: &PgPool,
        owner_id: Uuid,
        dto: &CreateAccountDto,
    ) -> Result<Account, AppError> {
        let name = dto.name.trim().to_string();
        if name.is_empty() {
            return Err(AppError::ValidationError(
                "Name cannot be empty".to_string(),
            ));
        }

        let balance = dto.balance.unwrap_or(Decimal::ZERO);
        let account_type = dto.account_type.as_str();

        // Determine currency: use provided currency or fall back to user's default_currency
        let currency = match &dto.currency {
            Some(code) => {
                // Validate that the currency exists and is active
                let is_valid = CurrencyService::validate_currency(pool, code).await?;
                if !is_valid {
                    return Err(AppError::ValidationError(format!(
                        "Currency '{}' is not valid or not active",
                        code
                    )));
                }
                code.to_uppercase()
            }
            None => {
                // Fetch user's default_currency from the users table
                let default_currency: String =
                    sqlx::query_scalar("SELECT default_currency FROM users WHERE id = $1")
                        .bind(owner_id)
                        .fetch_one(pool)
                        .await
                        .map_err(|e| AppError::InternalError(e.to_string()))?;
                default_currency
            }
        };

        sqlx::query_as::<_, Account>(
            r#"
            INSERT INTO accounts (owner_id, name, account_type, balance, color_hex, currency)
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING id, owner_id, name, account_type, balance, color_hex, currency, created_at, updated_at
            "#,
        )
        .bind(owner_id)
        .bind(&name)
        .bind(account_type)
        .bind(balance)
        .bind(&dto.color_hex)
        .bind(&currency)
        .fetch_one(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))
    }

    /// Update an account (partial update - PATCH semantics).
    pub async fn update_account(
        pool: &PgPool,
        account_id: Uuid,
        owner_id: Uuid,
        dto: &UpdateAccountDto,
    ) -> Result<Account, AppError> {
        // Verify ownership and get current account
        let current = Self::get_account_by_id(pool, account_id, owner_id).await?;

        // Determine new values
        let new_name = match &dto.name {
            Some(n) => {
                let trimmed = n.trim().to_string();
                if trimmed.is_empty() {
                    return Err(AppError::ValidationError(
                        "Name cannot be empty".to_string(),
                    ));
                }
                trimmed
            }
            None => current.name,
        };

        let new_type = dto
            .account_type
            .as_ref()
            .map(|t| t.as_str())
            .unwrap_or(&current.account_type);

        let new_color = dto.color_hex.as_ref().unwrap_or(&current.color_hex);

        sqlx::query_as::<_, Account>(
            r#"
            UPDATE accounts SET
                name = $3,
                account_type = $4,
                color_hex = $5,
                updated_at = NOW()
            WHERE id = $1 AND owner_id = $2
            RETURNING id, owner_id, name, account_type, balance, color_hex, currency, created_at, updated_at
            "#,
        )
        .bind(account_id)
        .bind(owner_id)
        .bind(&new_name)
        .bind(new_type)
        .bind(new_color)
        .fetch_one(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))
    }

    /// Update only the balance field.
    pub async fn update_balance(
        pool: &PgPool,
        account_id: Uuid,
        owner_id: Uuid,
        dto: &UpdateBalanceDto,
    ) -> Result<Account, AppError> {
        sqlx::query_as::<_, Account>(
            r#"
            UPDATE accounts
            SET balance = $3, updated_at = NOW()
            WHERE id = $1 AND owner_id = $2
            RETURNING id, owner_id, name, account_type, balance, color_hex, currency, created_at, updated_at
            "#,
        )
        .bind(account_id)
        .bind(owner_id)
        .bind(dto.balance)
        .fetch_optional(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?
        .ok_or_else(|| AppError::NotFound("Account not found".to_string()))
    }

    /// Delete an account.
    pub async fn delete_account(
        pool: &PgPool,
        account_id: Uuid,
        owner_id: Uuid,
    ) -> Result<(), AppError> {
        let result = sqlx::query("DELETE FROM accounts WHERE id = $1 AND owner_id = $2")
            .bind(account_id)
            .bind(owner_id)
            .execute(pool)
            .await
            .map_err(|e| AppError::InternalError(e.to_string()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("Account not found".to_string()));
        }

        Ok(())
    }
}
