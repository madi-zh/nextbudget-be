use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use super::models::{
    Budget, BudgetMember, CreateBudgetDto, CreateSharedBudgetDto, ListBudgetsQuery,
    MemberResponse, UpdateBudgetDto, UpdateIncomeDto, UpdateSavingsRateDto,
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
                SELECT id, owner_id, month, year, total_income, savings_rate, currency, created_at, updated_at, is_shared, name
                FROM budgets
                WHERE owner_id = $1 AND year = $2 AND is_shared = false
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
                SELECT id, owner_id, month, year, total_income, savings_rate, currency, created_at, updated_at, is_shared, name
                FROM budgets
                WHERE owner_id = $1 AND is_shared = false
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
            SELECT id, owner_id, month, year, total_income, savings_rate, currency, created_at, updated_at, is_shared, name
            FROM budgets
            WHERE id = $1 AND owner_id = $2 AND is_shared = false
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
            SELECT id, owner_id, month, year, total_income, savings_rate, currency, created_at, updated_at, is_shared, name
            FROM budgets
            WHERE owner_id = $1 AND month = $2 AND year = $3 AND is_shared = false
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
            "SELECT COUNT(*) FROM budgets WHERE owner_id = $1 AND month = $2 AND year = $3 AND is_shared = false",
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
                        "Currency '{code}' is not valid or not active"
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
            INSERT INTO budgets (owner_id, month, year, total_income, savings_rate, currency, is_shared, name)
            VALUES ($1, $2, $3, $4, $5, $6, false, '')
            RETURNING id, owner_id, month, year, total_income, savings_rate, currency, created_at, updated_at, is_shared, name
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
            RETURNING id, owner_id, month, year, total_income, savings_rate, currency, created_at, updated_at, is_shared, name
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
            WHERE id = $2 AND owner_id = $3 AND is_shared = false
            RETURNING id, owner_id, month, year, total_income, savings_rate, currency, created_at, updated_at, is_shared, name
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
            WHERE id = $2 AND owner_id = $3 AND is_shared = false
            RETURNING id, owner_id, month, year, total_income, savings_rate, currency, created_at, updated_at, is_shared, name
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
        let result = sqlx::query("DELETE FROM budgets WHERE id = $1 AND owner_id = $2 AND is_shared = false")
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

    // ─── Shared Budget Methods ───────────────────────────────────────────

    /// Verify that a user has access to a budget (is owner OR member)
    pub async fn verify_budget_access(
        pool: &PgPool,
        budget_id: Uuid,
        user_id: Uuid,
    ) -> Result<bool, AppError> {
        let result = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS(
                SELECT 1 FROM budgets b
                LEFT JOIN budget_members bm ON b.id = bm.budget_id AND bm.user_id = $2
                WHERE b.id = $1 AND (b.owner_id = $2 OR bm.user_id IS NOT NULL)
            )
            "#,
        )
        .bind(budget_id)
        .bind(user_id)
        .fetch_one(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?;

        Ok(result)
    }

    /// List shared budgets the user belongs to (as owner or member)
    pub async fn list_shared_budgets(
        pool: &PgPool,
        user_id: Uuid,
        query: &ListBudgetsQuery,
    ) -> Result<Vec<Budget>, AppError> {
        let budgets = if let Some(year) = query.year {
            sqlx::query_as::<_, Budget>(
                r#"
                SELECT DISTINCT b.id, b.owner_id, b.month, b.year, b.total_income, b.savings_rate,
                       b.currency, b.created_at, b.updated_at, b.is_shared, b.name
                FROM budgets b
                LEFT JOIN budget_members bm ON b.id = bm.budget_id
                WHERE b.is_shared = true AND (b.owner_id = $1 OR bm.user_id = $1) AND b.year = $2
                ORDER BY b.year DESC, b.month DESC
                LIMIT $3 OFFSET $4
                "#,
            )
            .bind(user_id)
            .bind(year)
            .bind(query.limit)
            .bind(query.offset)
            .fetch_all(pool)
            .await
        } else {
            sqlx::query_as::<_, Budget>(
                r#"
                SELECT DISTINCT b.id, b.owner_id, b.month, b.year, b.total_income, b.savings_rate,
                       b.currency, b.created_at, b.updated_at, b.is_shared, b.name
                FROM budgets b
                LEFT JOIN budget_members bm ON b.id = bm.budget_id
                WHERE b.is_shared = true AND (b.owner_id = $1 OR bm.user_id = $1)
                ORDER BY b.year DESC, b.month DESC
                LIMIT $2 OFFSET $3
                "#,
            )
            .bind(user_id)
            .bind(query.limit)
            .bind(query.offset)
            .fetch_all(pool)
            .await
        };

        budgets.map_err(|e| AppError::InternalError(e.to_string()))
    }

    /// Get a single shared budget with access check
    pub async fn get_shared_budget(
        pool: &PgPool,
        budget_id: Uuid,
        user_id: Uuid,
    ) -> Result<Budget, AppError> {
        sqlx::query_as::<_, Budget>(
            r#"
            SELECT DISTINCT b.id, b.owner_id, b.month, b.year, b.total_income, b.savings_rate,
                   b.currency, b.created_at, b.updated_at, b.is_shared, b.name
            FROM budgets b
            LEFT JOIN budget_members bm ON b.id = bm.budget_id AND bm.user_id = $2
            WHERE b.id = $1 AND b.is_shared = true AND (b.owner_id = $2 OR bm.user_id IS NOT NULL)
            "#,
        )
        .bind(budget_id)
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?
        .ok_or_else(|| AppError::NotFound("Shared budget not found".to_string()))
    }

    /// Create a new shared budget
    pub async fn create_shared_budget(
        pool: &PgPool,
        owner_id: Uuid,
        dto: &CreateSharedBudgetDto,
    ) -> Result<Budget, AppError> {
        let total_income = dto.total_income.unwrap_or(Decimal::ZERO);
        let savings_rate = dto.savings_rate.unwrap_or(Decimal::ZERO);

        let currency = match &dto.currency {
            Some(code) => {
                let is_valid = CurrencyService::validate_currency(pool, code).await?;
                if !is_valid {
                    return Err(AppError::ValidationError(format!(
                        "Currency '{code}' is not valid or not active"
                    )));
                }
                code.to_uppercase()
            }
            None => {
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
            INSERT INTO budgets (owner_id, month, year, total_income, savings_rate, currency, is_shared, name)
            VALUES ($1, $2, $3, $4, $5, $6, true, $7)
            RETURNING id, owner_id, month, year, total_income, savings_rate, currency, created_at, updated_at, is_shared, name
            "#,
        )
        .bind(owner_id)
        .bind(dto.month)
        .bind(dto.year)
        .bind(total_income)
        .bind(savings_rate)
        .bind(&currency)
        .bind(&dto.name)
        .fetch_one(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))
    }

    /// Update a shared budget (any member can update)
    pub async fn update_shared_budget(
        pool: &PgPool,
        budget_id: Uuid,
        user_id: Uuid,
        dto: &UpdateBudgetDto,
    ) -> Result<Budget, AppError> {
        // Verify access
        let budget = Self::get_shared_budget(pool, budget_id, user_id).await?;

        let new_month = dto.month.unwrap_or(budget.month);
        let new_year = dto.year.unwrap_or(budget.year);
        let new_income = dto.total_income.unwrap_or(budget.total_income);
        let new_savings_rate = dto.savings_rate.unwrap_or(budget.savings_rate);

        sqlx::query_as::<_, Budget>(
            r#"
            UPDATE budgets
            SET month = $1, year = $2, total_income = $3, savings_rate = $4, updated_at = NOW()
            WHERE id = $5 AND is_shared = true
            RETURNING id, owner_id, month, year, total_income, savings_rate, currency, created_at, updated_at, is_shared, name
            "#,
        )
        .bind(new_month)
        .bind(new_year)
        .bind(new_income)
        .bind(new_savings_rate)
        .bind(budget_id)
        .fetch_one(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))
    }

    /// Delete a shared budget (owner only)
    pub async fn delete_shared_budget(
        pool: &PgPool,
        budget_id: Uuid,
        user_id: Uuid,
    ) -> Result<(), AppError> {
        let result = sqlx::query("DELETE FROM budgets WHERE id = $1 AND owner_id = $2 AND is_shared = true")
            .bind(budget_id)
            .bind(user_id)
            .execute(pool)
            .await
            .map_err(|e| AppError::InternalError(e.to_string()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("Shared budget not found or you are not the owner".to_string()));
        }

        Ok(())
    }

    /// Update income on a shared budget (any member)
    pub async fn update_shared_income(
        pool: &PgPool,
        budget_id: Uuid,
        user_id: Uuid,
        dto: &UpdateIncomeDto,
    ) -> Result<Budget, AppError> {
        // Verify access
        if !Self::verify_budget_access(pool, budget_id, user_id).await? {
            return Err(AppError::NotFound("Shared budget not found".to_string()));
        }

        sqlx::query_as::<_, Budget>(
            r#"
            UPDATE budgets
            SET total_income = $1, updated_at = NOW()
            WHERE id = $2 AND is_shared = true
            RETURNING id, owner_id, month, year, total_income, savings_rate, currency, created_at, updated_at, is_shared, name
            "#,
        )
        .bind(dto.total_income)
        .bind(budget_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?
        .ok_or_else(|| AppError::NotFound("Shared budget not found".to_string()))
    }

    /// Update savings rate on a shared budget (any member)
    pub async fn update_shared_savings_rate(
        pool: &PgPool,
        budget_id: Uuid,
        user_id: Uuid,
        dto: &UpdateSavingsRateDto,
    ) -> Result<Budget, AppError> {
        if !Self::verify_budget_access(pool, budget_id, user_id).await? {
            return Err(AppError::NotFound("Shared budget not found".to_string()));
        }

        sqlx::query_as::<_, Budget>(
            r#"
            UPDATE budgets
            SET savings_rate = $1, updated_at = NOW()
            WHERE id = $2 AND is_shared = true
            RETURNING id, owner_id, month, year, total_income, savings_rate, currency, created_at, updated_at, is_shared, name
            "#,
        )
        .bind(dto.savings_rate)
        .bind(budget_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?
        .ok_or_else(|| AppError::NotFound("Shared budget not found".to_string()))
    }

    /// Add a member to a shared budget (owner only)
    pub async fn add_member(
        pool: &PgPool,
        budget_id: Uuid,
        owner_id: Uuid,
        email: &str,
    ) -> Result<MemberResponse, AppError> {
        // Verify the requester is the owner
        let is_owner = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM budgets WHERE id = $1 AND owner_id = $2 AND is_shared = true)",
        )
        .bind(budget_id)
        .bind(owner_id)
        .fetch_one(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?;

        if !is_owner {
            return Err(AppError::Forbidden("Only the budget owner can add members".to_string()));
        }

        // Lookup user by email
        let user = sqlx::query_as::<_, (Uuid, String, Option<String>)>(
            "SELECT id, email, full_name FROM users WHERE email = $1",
        )
        .bind(email)
        .fetch_optional(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?
        .ok_or_else(|| AppError::NotFound(format!("User with email '{email}' not found")))?;

        let (user_id, user_email, full_name) = user;

        // Don't allow adding the owner as a member
        if user_id == owner_id {
            return Err(AppError::ValidationError("Cannot add the owner as a member".to_string()));
        }

        // Insert member
        let member = sqlx::query_as::<_, BudgetMember>(
            r#"
            INSERT INTO budget_members (budget_id, user_id)
            VALUES ($1, $2)
            ON CONFLICT (budget_id, user_id) DO NOTHING
            RETURNING id, budget_id, user_id, created_at
            "#,
        )
        .bind(budget_id)
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?
        .ok_or_else(|| AppError::Conflict("User is already a member of this budget".to_string()))?;

        Ok(MemberResponse {
            id: user_id,
            email: user_email,
            full_name,
            joined_at: member.created_at,
        })
    }

    /// Remove a member from a shared budget (owner only)
    pub async fn remove_member(
        pool: &PgPool,
        budget_id: Uuid,
        owner_id: Uuid,
        member_user_id: Uuid,
    ) -> Result<(), AppError> {
        // Verify the requester is the owner
        let is_owner = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM budgets WHERE id = $1 AND owner_id = $2 AND is_shared = true)",
        )
        .bind(budget_id)
        .bind(owner_id)
        .fetch_one(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?;

        if !is_owner {
            return Err(AppError::Forbidden("Only the budget owner can remove members".to_string()));
        }

        let result = sqlx::query("DELETE FROM budget_members WHERE budget_id = $1 AND user_id = $2")
            .bind(budget_id)
            .bind(member_user_id)
            .execute(pool)
            .await
            .map_err(|e| AppError::InternalError(e.to_string()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("Member not found".to_string()));
        }

        Ok(())
    }

    /// List members of a shared budget (any member can view)
    pub async fn list_members(
        pool: &PgPool,
        budget_id: Uuid,
        user_id: Uuid,
    ) -> Result<Vec<MemberResponse>, AppError> {
        // Verify access
        if !Self::verify_budget_access(pool, budget_id, user_id).await? {
            return Err(AppError::Forbidden("Access denied".to_string()));
        }

        let members = sqlx::query_as::<_, (Uuid, String, Option<String>, DateTime<Utc>)>(
            r#"
            SELECT u.id, u.email, u.full_name, bm.created_at
            FROM budget_members bm
            JOIN users u ON bm.user_id = u.id
            WHERE bm.budget_id = $1
            ORDER BY bm.created_at ASC
            "#,
        )
        .bind(budget_id)
        .fetch_all(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?;

        Ok(members
            .into_iter()
            .map(|(id, email, full_name, joined_at)| MemberResponse {
                id,
                email,
                full_name,
                joined_at,
            })
            .collect())
    }
}
