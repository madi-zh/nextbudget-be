use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use super::models::{
    Budget, CreateBudgetDto, ListBudgetsQuery, UpdateBudgetDto, UpdateIncomeDto,
    UpdateSavingsRateDto,
};
use crate::currency::service::CurrencyService;
use crate::errors::AppError;

/// Service layer for budget business logic.
pub struct BudgetService;

impl BudgetService {
    /// List all budgets for a user with optional year filtering and pagination.
    pub async fn list_budgets(
        pool: &PgPool,
        owner_id: Uuid,
        query: &ListBudgetsQuery,
    ) -> Result<Vec<Budget>, AppError> {
        let budgets = if let Some(year) = query.year {
            sqlx::query_as::<_, Budget>(
                r#"
                SELECT id, owner_id, month, year, total_income, savings_rate, currency, created_at, updated_at
                FROM budgets
                WHERE owner_id = $1 AND year = $2
                ORDER BY year DESC, month DESC
                LIMIT $3 OFFSET $4
                "#,
            )
            .bind(owner_id)
            .bind(year)
            .bind(query.limit)
            .bind(query.offset)
            .fetch_all(pool)
            .await
        } else {
            sqlx::query_as::<_, Budget>(
                r#"
                SELECT id, owner_id, month, year, total_income, savings_rate, currency, created_at, updated_at
                FROM budgets
                WHERE owner_id = $1
                ORDER BY year DESC, month DESC
                LIMIT $2 OFFSET $3
                "#,
            )
            .bind(owner_id)
            .bind(query.limit)
            .bind(query.offset)
            .fetch_all(pool)
            .await
        };

        budgets.map_err(|e| AppError::InternalError(e.to_string()))
    }

    /// Get a budget by ID, ensuring the requesting user owns it.
    pub async fn get_budget_by_id(
        pool: &PgPool,
        budget_id: Uuid,
        owner_id: Uuid,
    ) -> Result<Budget, AppError> {
        sqlx::query_as::<_, Budget>(
            r#"
            SELECT id, owner_id, month, year, total_income, savings_rate, currency, created_at, updated_at
            FROM budgets
            WHERE id = $1 AND owner_id = $2
            "#,
        )
        .bind(budget_id)
        .bind(owner_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?
        .ok_or_else(|| AppError::NotFound("Budget not found".to_string()))
    }

    /// Get a budget by month and year for a specific user.
    pub async fn get_budget_by_month_year(
        pool: &PgPool,
        owner_id: Uuid,
        month: i16,
        year: i16,
    ) -> Result<Budget, AppError> {
        sqlx::query_as::<_, Budget>(
            r#"
            SELECT id, owner_id, month, year, total_income, savings_rate, currency, created_at, updated_at
            FROM budgets
            WHERE owner_id = $1 AND month = $2 AND year = $3
            "#,
        )
        .bind(owner_id)
        .bind(month)
        .bind(year)
        .fetch_optional(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?
        .ok_or_else(|| AppError::NotFound(format!("Budget not found for {}/{}", month + 1, year)))
    }

    /// Create a new budget.
    pub async fn create_budget(
        pool: &PgPool,
        owner_id: Uuid,
        dto: &CreateBudgetDto,
    ) -> Result<Budget, AppError> {
        // Check for existing budget with same month/year
        let exists = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM budgets WHERE owner_id = $1 AND month = $2 AND year = $3",
        )
        .bind(owner_id)
        .bind(dto.month)
        .bind(dto.year)
        .fetch_one(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?;

        if exists > 0 {
            return Err(AppError::Conflict(format!(
                "Budget already exists for {}/{}",
                dto.month + 1,
                dto.year
            )));
        }

        let total_income = dto.total_income.unwrap_or(Decimal::ZERO);
        let savings_rate = dto.savings_rate.unwrap_or(Decimal::ZERO);

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

        sqlx::query_as::<_, Budget>(
            r#"
            INSERT INTO budgets (owner_id, month, year, total_income, savings_rate, currency)
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING id, owner_id, month, year, total_income, savings_rate, currency, created_at, updated_at
            "#,
        )
        .bind(owner_id)
        .bind(dto.month)
        .bind(dto.year)
        .bind(total_income)
        .bind(savings_rate)
        .bind(&currency)
        .fetch_one(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))
    }

    /// Update a budget (partial update - PATCH semantics).
    pub async fn update_budget(
        pool: &PgPool,
        budget_id: Uuid,
        owner_id: Uuid,
        dto: &UpdateBudgetDto,
    ) -> Result<Budget, AppError> {
        // First verify ownership and get current budget
        let current = Self::get_budget_by_id(pool, budget_id, owner_id).await?;

        // Determine new values (use existing if not provided)
        let new_month = dto.month.unwrap_or(current.month);
        let new_year = dto.year.unwrap_or(current.year);
        let new_income = dto.total_income.unwrap_or(current.total_income);
        let new_savings_rate = dto.savings_rate.unwrap_or(current.savings_rate);

        // If month/year is changing, check for conflicts
        if new_month != current.month || new_year != current.year {
            let exists = sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM budgets WHERE owner_id = $1 AND month = $2 AND year = $3 AND id != $4",
            )
            .bind(owner_id)
            .bind(new_month)
            .bind(new_year)
            .bind(budget_id)
            .fetch_one(pool)
            .await
            .map_err(|e| AppError::InternalError(e.to_string()))?;

            if exists > 0 {
                return Err(AppError::Conflict(format!(
                    "Budget already exists for {}/{}",
                    new_month + 1,
                    new_year
                )));
            }
        }

        sqlx::query_as::<_, Budget>(
            r#"
            UPDATE budgets
            SET month = $1, year = $2, total_income = $3, savings_rate = $4, updated_at = NOW()
            WHERE id = $5 AND owner_id = $6
            RETURNING id, owner_id, month, year, total_income, savings_rate, currency, created_at, updated_at
            "#,
        )
        .bind(new_month)
        .bind(new_year)
        .bind(new_income)
        .bind(new_savings_rate)
        .bind(budget_id)
        .bind(owner_id)
        .fetch_one(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))
    }

    /// Update only the income field (optimized single query with ownership check).
    pub async fn update_income(
        pool: &PgPool,
        budget_id: Uuid,
        owner_id: Uuid,
        dto: &UpdateIncomeDto,
    ) -> Result<Budget, AppError> {
        sqlx::query_as::<_, Budget>(
            r#"
            UPDATE budgets
            SET total_income = $1, updated_at = NOW()
            WHERE id = $2 AND owner_id = $3
            RETURNING id, owner_id, month, year, total_income, savings_rate, currency, created_at, updated_at
            "#,
        )
        .bind(dto.total_income)
        .bind(budget_id)
        .bind(owner_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?
        .ok_or_else(|| AppError::NotFound("Budget not found".to_string()))
    }

    /// Update only the savings rate field (optimized single query with ownership check).
    pub async fn update_savings_rate(
        pool: &PgPool,
        budget_id: Uuid,
        owner_id: Uuid,
        dto: &UpdateSavingsRateDto,
    ) -> Result<Budget, AppError> {
        sqlx::query_as::<_, Budget>(
            r#"
            UPDATE budgets
            SET savings_rate = $1, updated_at = NOW()
            WHERE id = $2 AND owner_id = $3
            RETURNING id, owner_id, month, year, total_income, savings_rate, currency, created_at, updated_at
            "#,
        )
        .bind(dto.savings_rate)
        .bind(budget_id)
        .bind(owner_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?
        .ok_or_else(|| AppError::NotFound("Budget not found".to_string()))
    }

    /// Delete a budget.
    pub async fn delete_budget(
        pool: &PgPool,
        budget_id: Uuid,
        owner_id: Uuid,
    ) -> Result<(), AppError> {
        let result = sqlx::query("DELETE FROM budgets WHERE id = $1 AND owner_id = $2")
            .bind(budget_id)
            .bind(owner_id)
            .execute(pool)
            .await
            .map_err(|e| AppError::InternalError(e.to_string()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("Budget not found".to_string()));
        }

        Ok(())
    }
}
