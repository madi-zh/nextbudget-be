use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use super::models::{Category, CategoryWithSpent, CreateCategoryDto, UpdateCategoryDto};
use crate::errors::AppError;

/// Service layer for category business logic.
pub struct CategoryService;

impl CategoryService {
    /// Verify user owns the budget - CRITICAL for authorization
    pub async fn verify_budget_ownership(
        pool: &PgPool,
        budget_id: Uuid,
        user_id: Uuid,
    ) -> Result<bool, AppError> {
        let result = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM budgets WHERE id = $1 AND owner_id = $2",
        )
        .bind(budget_id)
        .bind(user_id)
        .fetch_one(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?;

        Ok(result > 0)
    }

    /// Get category by ID with ownership check through budget
    pub async fn get_by_id(
        pool: &PgPool,
        category_id: Uuid,
        user_id: Uuid,
    ) -> Result<CategoryWithSpent, AppError> {
        // Query with LEFT JOIN to transactions - returns 0 if no transactions exist
        sqlx::query_as::<_, CategoryWithSpent>(
            r#"
            SELECT
                c.id, c.budget_id, c.name, c.allocated_amount,
                c.color_hex, c.created_at, c.updated_at,
                COALESCE(SUM(CASE WHEN t.transaction_type = 'expense' THEN t.amount ELSE 0 END), 0) as spent_amount
            FROM categories c
            INNER JOIN budgets b ON c.budget_id = b.id AND b.owner_id = $2
            LEFT JOIN transactions t ON c.id = t.category_id
            WHERE c.id = $1
            GROUP BY c.id, c.budget_id, c.name, c.allocated_amount,
                     c.color_hex, c.created_at, c.updated_at
            "#,
        )
        .bind(category_id)
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?
        .ok_or_else(|| AppError::NotFound("Category not found".to_string()))
    }

    /// Get all categories for a specific budget (with ownership check)
    pub async fn get_by_budget_id(
        pool: &PgPool,
        budget_id: Uuid,
        user_id: Uuid,
    ) -> Result<Vec<CategoryWithSpent>, AppError> {
        // First verify ownership
        if !Self::verify_budget_ownership(pool, budget_id, user_id).await? {
            return Err(AppError::NotFound("Budget not found".to_string()));
        }

        sqlx::query_as::<_, CategoryWithSpent>(
            r#"
            SELECT
                c.id, c.budget_id, c.name, c.allocated_amount,
                c.color_hex, c.created_at, c.updated_at,
                COALESCE(SUM(CASE WHEN t.transaction_type = 'expense' THEN t.amount ELSE 0 END), 0) as spent_amount
            FROM categories c
            LEFT JOIN transactions t ON c.id = t.category_id
            WHERE c.budget_id = $1
            GROUP BY c.id, c.budget_id, c.name, c.allocated_amount,
                     c.color_hex, c.created_at, c.updated_at
            ORDER BY c.name ASC
            "#,
        )
        .bind(budget_id)
        .fetch_all(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))
    }

    /// Get all categories for user (across all budgets)
    pub async fn get_all_for_user(
        pool: &PgPool,
        user_id: Uuid,
    ) -> Result<Vec<CategoryWithSpent>, AppError> {
        sqlx::query_as::<_, CategoryWithSpent>(
            r#"
            SELECT
                c.id, c.budget_id, c.name, c.allocated_amount,
                c.color_hex, c.created_at, c.updated_at,
                COALESCE(SUM(CASE WHEN t.transaction_type = 'expense' THEN t.amount ELSE 0 END), 0) as spent_amount
            FROM categories c
            INNER JOIN budgets b ON c.budget_id = b.id AND b.owner_id = $1
            LEFT JOIN transactions t ON c.id = t.category_id
            GROUP BY c.id, c.budget_id, c.name, c.allocated_amount,
                     c.color_hex, c.created_at, c.updated_at
            ORDER BY c.name ASC
            "#,
        )
        .bind(user_id)
        .fetch_all(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))
    }

    /// Create a new category
    pub async fn create(
        pool: &PgPool,
        dto: &CreateCategoryDto,
        user_id: Uuid,
    ) -> Result<Category, AppError> {
        // Verify budget ownership first
        if !Self::verify_budget_ownership(pool, dto.budget_id, user_id).await? {
            return Err(AppError::NotFound("Budget not found".to_string()));
        }

        // Trim and sanitize name
        let name = dto.name.trim().to_string();
        if name.is_empty() {
            return Err(AppError::ValidationError(
                "Name cannot be empty".to_string(),
            ));
        }

        let allocated_amount = dto.allocated_amount.unwrap_or(Decimal::ZERO);

        let category = sqlx::query_as::<_, Category>(
            r#"
            INSERT INTO categories (budget_id, name, allocated_amount, color_hex)
            VALUES ($1, $2, $3, $4)
            RETURNING id, budget_id, name, allocated_amount, color_hex, created_at, updated_at
            "#,
        )
        .bind(dto.budget_id)
        .bind(&name)
        .bind(allocated_amount)
        .bind(&dto.color_hex)
        .fetch_one(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?;

        Ok(category)
    }

    /// Update an existing category
    pub async fn update(
        pool: &PgPool,
        category_id: Uuid,
        dto: &UpdateCategoryDto,
        user_id: Uuid,
    ) -> Result<Category, AppError> {
        // First verify the category exists and user has access
        let existing = Self::get_by_id(pool, category_id, user_id).await?;

        // Build update values
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
            None => existing.name,
        };

        let new_allocated_amount = dto.allocated_amount.unwrap_or(existing.allocated_amount);
        let new_color_hex = dto.color_hex.as_ref().unwrap_or(&existing.color_hex);

        sqlx::query_as::<_, Category>(
            r#"
            UPDATE categories
            SET name = $2, allocated_amount = $3, color_hex = $4, updated_at = NOW()
            WHERE id = $1
            RETURNING id, budget_id, name, allocated_amount, color_hex, created_at, updated_at
            "#,
        )
        .bind(category_id)
        .bind(&new_name)
        .bind(new_allocated_amount)
        .bind(new_color_hex)
        .fetch_one(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))
    }

    /// Delete a category (cascades to transactions)
    pub async fn delete(pool: &PgPool, category_id: Uuid, user_id: Uuid) -> Result<(), AppError> {
        // Verify ownership first
        let _ = Self::get_by_id(pool, category_id, user_id).await?;

        sqlx::query("DELETE FROM categories WHERE id = $1")
            .bind(category_id)
            .execute(pool)
            .await
            .map_err(|e| AppError::InternalError(e.to_string()))?;

        Ok(())
    }
}
