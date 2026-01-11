# Category Management Feature Plan

## Overview

This document provides a detailed implementation plan for Feature 4: Category Management in the BudgetFlow Rust backend. Categories are budget subdivisions that track spending by purpose (e.g., "Groceries", "Utilities").

---

## 1. File Structure

```
src/
├── main.rs                    # Add category routes
├── models.rs                  # Existing models (keep as-is)
├── errors.rs                  # Add NotFound variant
├── auth.rs                    # Existing (extract auth helpers)
├── categories/
│   ├── mod.rs                 # Module exports
│   ├── models.rs              # Category domain model and DTOs
│   ├── handlers.rs            # HTTP handlers (thin layer)
│   └── service.rs             # Business logic and queries
└── middleware/
    └── auth.rs                # Extracted auth middleware (optional refactor)

migrations/
└── YYYYMMDDHHMMSS_create_categories_table.sql
```

### Rationale
- **Separation of concerns**: Handlers stay thin, services contain logic
- **Testability**: Services can be unit tested without HTTP
- **Scalability**: Pattern replicates for transactions, accounts, etc.

---

## 2. Database Migration

### File: `migrations/YYYYMMDDHHMMSS_create_categories_table.sql`

```sql
-- Prerequisites: budgets table must exist
-- Run migrations for budgets first if not already done

CREATE TABLE categories (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    budget_id UUID NOT NULL REFERENCES budgets(id) ON DELETE CASCADE,
    name VARCHAR(50) NOT NULL,
    allocated_amount NUMERIC(12,2) NOT NULL DEFAULT 0 CHECK (allocated_amount >= 0),
    color_hex CHAR(7) NOT NULL DEFAULT '#64748b' CHECK (color_hex ~ '^#[0-9A-Fa-f]{6}$'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Index for efficient lookup by budget
CREATE INDEX idx_categories_budget_id ON categories(budget_id);

-- Trigger for updated_at (reuse if already exists)
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ language 'plpgsql';

CREATE TRIGGER update_categories_updated_at
    BEFORE UPDATE ON categories
    FOR EACH ROW
    EXECUTE FUNCTION update_updated_at_column();
```

---

## 3. Model and DTO Definitions

### File: `src/categories/models.rs`

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use validator::Validate;

/// Database model - maps directly to categories table
#[derive(Debug, Clone, FromRow)]
pub struct Category {
    pub id: Uuid,
    pub budget_id: Uuid,
    pub name: String,
    pub allocated_amount: sqlx::types::BigDecimal,
    pub color_hex: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Extended model with computed spent_amount from transactions
#[derive(Debug, Clone, FromRow)]
pub struct CategoryWithSpent {
    pub id: Uuid,
    pub budget_id: Uuid,
    pub name: String,
    pub allocated_amount: sqlx::types::BigDecimal,
    pub color_hex: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub spent_amount: sqlx::types::BigDecimal,
}

/// Response DTO - converts BigDecimal to f64 for JSON
#[derive(Debug, Serialize)]
pub struct CategoryResponseDto {
    pub id: Uuid,
    pub budget_id: Uuid,
    pub name: String,
    pub allocated_amount: f64,
    pub spent_amount: f64,
    pub remaining_amount: f64,
    pub color_hex: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl CategoryResponseDto {
    pub fn from_category_with_spent(cat: CategoryWithSpent) -> Self {
        use bigdecimal::ToPrimitive;

        let allocated = cat.allocated_amount.to_f64().unwrap_or(0.0);
        let spent = cat.spent_amount.to_f64().unwrap_or(0.0);

        Self {
            id: cat.id,
            budget_id: cat.budget_id,
            name: cat.name,
            allocated_amount: allocated,
            spent_amount: spent,
            remaining_amount: allocated - spent,
            color_hex: cat.color_hex,
            created_at: cat.created_at,
            updated_at: cat.updated_at,
        }
    }
}

/// Request DTO for creating a category
#[derive(Debug, Deserialize, Validate)]
pub struct CreateCategoryDto {
    pub budget_id: Uuid,

    #[validate(length(min = 1, max = 50, message = "Name must be 1-50 characters"))]
    pub name: String,

    #[validate(range(min = 0.0, message = "Allocated amount must be non-negative"))]
    #[serde(default)]
    pub allocated_amount: f64,

    #[validate(regex(
        path = "COLOR_HEX_REGEX",
        message = "Color must be in #RRGGBB format"
    ))]
    #[serde(default = "default_color")]
    pub color_hex: String,
}

fn default_color() -> String {
    "#64748b".to_string()
}

lazy_static::lazy_static! {
    static ref COLOR_HEX_REGEX: regex::Regex =
        regex::Regex::new(r"^#[0-9A-Fa-f]{6}$").unwrap();
}

/// Request DTO for updating a category (all fields optional)
#[derive(Debug, Deserialize, Validate)]
pub struct UpdateCategoryDto {
    #[validate(length(min = 1, max = 50, message = "Name must be 1-50 characters"))]
    pub name: Option<String>,

    #[validate(range(min = 0.0, message = "Allocated amount must be non-negative"))]
    pub allocated_amount: Option<f64>,

    #[validate(regex(
        path = "COLOR_HEX_REGEX",
        message = "Color must be in #RRGGBB format"
    ))]
    pub color_hex: Option<String>,
}

/// Path parameters
#[derive(Debug, Deserialize)]
pub struct CategoryPath {
    pub id: Uuid,
}

#[derive(Debug, Deserialize)]
pub struct BudgetPath {
    pub budget_id: Uuid,
}
```

### Cargo.toml additions

```toml
[dependencies]
bigdecimal = { version = "0.4", features = ["serde"] }
lazy_static = "1.4"
regex = "1.10"
```

---

## 4. Service Layer Implementation

### File: `src/categories/service.rs`

```rust
use sqlx::PgPool;
use uuid::Uuid;

use crate::errors::AppError;
use super::models::{CategoryWithSpent, CreateCategoryDto, UpdateCategoryDto};

pub struct CategoryService;

impl CategoryService {
    /// Verify user owns the budget - CRITICAL for authorization
    pub async fn verify_budget_ownership(
        pool: &PgPool,
        budget_id: Uuid,
        user_id: Uuid,
    ) -> Result<bool, AppError> {
        let result = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM budgets WHERE id = $1 AND owner_id = $2"
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
        sqlx::query_as::<_, CategoryWithSpent>(
            r#"
            SELECT
                c.id, c.budget_id, c.name, c.allocated_amount,
                c.color_hex, c.created_at, c.updated_at,
                COALESCE(SUM(t.amount) FILTER (WHERE t.type = 'expense'), 0) as spent_amount
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
                COALESCE(SUM(t.amount) FILTER (WHERE t.type = 'expense'), 0) as spent_amount
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
                COALESCE(SUM(t.amount) FILTER (WHERE t.type = 'expense'), 0) as spent_amount
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
        dto: CreateCategoryDto,
        user_id: Uuid,
    ) -> Result<CategoryWithSpent, AppError> {
        // Verify budget ownership first
        if !Self::verify_budget_ownership(pool, dto.budget_id, user_id).await? {
            return Err(AppError::NotFound("Budget not found".to_string()));
        }

        // Trim and sanitize name
        let name = dto.name.trim().to_string();
        if name.is_empty() {
            return Err(AppError::ValidationError("Name cannot be empty".to_string()));
        }

        let category = sqlx::query_as::<_, CategoryWithSpent>(
            r#"
            WITH inserted AS (
                INSERT INTO categories (budget_id, name, allocated_amount, color_hex)
                VALUES ($1, $2, $3, $4)
                RETURNING id, budget_id, name, allocated_amount, color_hex, created_at, updated_at
            )
            SELECT
                i.id, i.budget_id, i.name, i.allocated_amount,
                i.color_hex, i.created_at, i.updated_at,
                0::NUMERIC(12,2) as spent_amount
            FROM inserted i
            "#,
        )
        .bind(dto.budget_id)
        .bind(&name)
        .bind(sqlx::types::BigDecimal::try_from(dto.allocated_amount)
            .map_err(|_| AppError::ValidationError("Invalid allocated amount".to_string()))?)
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
        dto: UpdateCategoryDto,
        user_id: Uuid,
    ) -> Result<CategoryWithSpent, AppError> {
        // First verify the category exists and user has access
        let existing = Self::get_by_id(pool, category_id, user_id).await?;

        // Build dynamic update
        let name = dto.name
            .map(|n| n.trim().to_string())
            .unwrap_or(existing.name);

        let allocated_amount = match dto.allocated_amount {
            Some(amt) => sqlx::types::BigDecimal::try_from(amt)
                .map_err(|_| AppError::ValidationError("Invalid allocated amount".to_string()))?,
            None => existing.allocated_amount.clone(),
        };

        let color_hex = dto.color_hex.unwrap_or(existing.color_hex);

        if name.is_empty() {
            return Err(AppError::ValidationError("Name cannot be empty".to_string()));
        }

        sqlx::query_as::<_, CategoryWithSpent>(
            r#"
            WITH updated AS (
                UPDATE categories
                SET name = $2, allocated_amount = $3, color_hex = $4, updated_at = NOW()
                WHERE id = $1
                RETURNING id, budget_id, name, allocated_amount, color_hex, created_at, updated_at
            )
            SELECT
                u.id, u.budget_id, u.name, u.allocated_amount,
                u.color_hex, u.created_at, u.updated_at,
                COALESCE(SUM(t.amount) FILTER (WHERE t.type = 'expense'), 0) as spent_amount
            FROM updated u
            LEFT JOIN transactions t ON u.id = t.category_id
            GROUP BY u.id, u.budget_id, u.name, u.allocated_amount,
                     u.color_hex, u.created_at, u.updated_at
            "#,
        )
        .bind(category_id)
        .bind(&name)
        .bind(&allocated_amount)
        .bind(&color_hex)
        .fetch_one(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))
    }

    /// Delete a category (cascades to transactions)
    pub async fn delete(
        pool: &PgPool,
        category_id: Uuid,
        user_id: Uuid,
    ) -> Result<(), AppError> {
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
```

---

## 5. Handler Implementations

### File: `src/categories/handlers.rs`

```rust
use actix_web::{delete, get, patch, post, web, HttpRequest, HttpResponse};
use sqlx::PgPool;
use validator::Validate;

use crate::auth::decode_token;
use crate::errors::AppError;
use super::models::{
    BudgetPath, CategoryPath, CategoryResponseDto,
    CreateCategoryDto, UpdateCategoryDto,
};
use super::service::CategoryService;

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

/// GET /categories - List all categories for the authenticated user
#[get("/categories")]
pub async fn list_categories(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    jwt_secret: web::Data<String>,
) -> Result<HttpResponse, AppError> {
    let user_id = extract_user_id(&req, jwt_secret.get_ref())?;

    let categories = CategoryService::get_all_for_user(pool.get_ref(), user_id).await?;

    let response: Vec<CategoryResponseDto> = categories
        .into_iter()
        .map(CategoryResponseDto::from_category_with_spent)
        .collect();

    Ok(HttpResponse::Ok().json(response))
}

/// GET /categories/{id} - Get a specific category
#[get("/categories/{id}")]
pub async fn get_category(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    jwt_secret: web::Data<String>,
    path: web::Path<CategoryPath>,
) -> Result<HttpResponse, AppError> {
    let user_id = extract_user_id(&req, jwt_secret.get_ref())?;

    let category = CategoryService::get_by_id(pool.get_ref(), path.id, user_id).await?;

    Ok(HttpResponse::Ok().json(CategoryResponseDto::from_category_with_spent(category)))
}

/// GET /categories/budget/{budget_id} - Get all categories for a budget
#[get("/categories/budget/{budget_id}")]
pub async fn get_categories_by_budget(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    jwt_secret: web::Data<String>,
    path: web::Path<BudgetPath>,
) -> Result<HttpResponse, AppError> {
    let user_id = extract_user_id(&req, jwt_secret.get_ref())?;

    let categories = CategoryService::get_by_budget_id(
        pool.get_ref(),
        path.budget_id,
        user_id
    ).await?;

    let response: Vec<CategoryResponseDto> = categories
        .into_iter()
        .map(CategoryResponseDto::from_category_with_spent)
        .collect();

    Ok(HttpResponse::Ok().json(response))
}

/// POST /categories - Create a new category
#[post("/categories")]
pub async fn create_category(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    jwt_secret: web::Data<String>,
    body: web::Json<CreateCategoryDto>,
) -> Result<HttpResponse, AppError> {
    let user_id = extract_user_id(&req, jwt_secret.get_ref())?;

    // Validate input
    body.validate()
        .map_err(|e| AppError::ValidationError(e.to_string()))?;

    let category = CategoryService::create(
        pool.get_ref(),
        body.into_inner(),
        user_id
    ).await?;

    Ok(HttpResponse::Created().json(CategoryResponseDto::from_category_with_spent(category)))
}

/// PATCH /categories/{id} - Update a category
#[patch("/categories/{id}")]
pub async fn update_category(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    jwt_secret: web::Data<String>,
    path: web::Path<CategoryPath>,
    body: web::Json<UpdateCategoryDto>,
) -> Result<HttpResponse, AppError> {
    let user_id = extract_user_id(&req, jwt_secret.get_ref())?;

    // Validate input
    body.validate()
        .map_err(|e| AppError::ValidationError(e.to_string()))?;

    let category = CategoryService::update(
        pool.get_ref(),
        path.id,
        body.into_inner(),
        user_id,
    ).await?;

    Ok(HttpResponse::Ok().json(CategoryResponseDto::from_category_with_spent(category)))
}

/// DELETE /categories/{id} - Delete a category
#[delete("/categories/{id}")]
pub async fn delete_category(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    jwt_secret: web::Data<String>,
    path: web::Path<CategoryPath>,
) -> Result<HttpResponse, AppError> {
    let user_id = extract_user_id(&req, jwt_secret.get_ref())?;

    CategoryService::delete(pool.get_ref(), path.id, user_id).await?;

    Ok(HttpResponse::NoContent().finish())
}
```

### File: `src/categories/mod.rs`

```rust
pub mod handlers;
pub mod models;
pub mod service;

pub use handlers::*;
```

---

## 6. Error Type Update

### File: `src/errors.rs` (additions)

```rust
#[derive(Debug)]
pub enum AppError {
    ValidationError(String),
    Unauthorized(String),
    NotFound(String),      // <-- ADD THIS
    Conflict(String),
    InternalError(String),
}

// In ResponseError impl, add:
AppError::NotFound(msg) => (
    actix_web::http::StatusCode::NOT_FOUND,
    "NOT_FOUND",
    msg.clone(),
),
```

---

## 7. Main.rs Integration

### File: `src/main.rs` (additions)

```rust
mod categories;

// In HttpServer::new closure:
.service(categories::list_categories)
.service(categories::get_category)
.service(categories::get_categories_by_budget)
.service(categories::create_category)
.service(categories::update_category)
.service(categories::delete_category)
```

---

## 8. Budget Ownership Verification Flow

```
Request → Handler → Extract JWT → Get user_id
                                      ↓
                               CategoryService
                                      ↓
                    ┌─────────────────────────────────┐
                    │  verify_budget_ownership()       │
                    │  SELECT COUNT(*) FROM budgets    │
                    │  WHERE id = ? AND owner_id = ?   │
                    └─────────────────────────────────┘
                                      ↓
                              count > 0 ?
                             /          \
                         Yes              No
                          ↓                ↓
                   Continue          Return 404
                   operation         (hides existence)
```

### Security Notes:
- Return 404 instead of 403 to prevent resource enumeration
- Always check ownership before any operation
- Use parameterized queries to prevent SQL injection

---

## 9. Input Validation Details

### Name Validation
| Rule | Implementation |
|------|----------------|
| Min length | `#[validate(length(min = 1, ...))]` |
| Max length | `#[validate(length(..., max = 50))]` |
| Trim whitespace | `dto.name.trim()` in service |
| Reject empty after trim | Manual check in service |

### Color Hex Validation
| Rule | Implementation |
|------|----------------|
| Format | Regex `^#[0-9A-Fa-f]{6}$` |
| Default | `#64748b` (slate-500) |
| Case | Accept any, store as-is |

### Allocated Amount Validation
| Rule | Implementation |
|------|----------------|
| Non-negative | `#[validate(range(min = 0.0))]` |
| Precision | 2 decimal places (database) |
| Default | 0.00 |

---

## 10. Test Cases

### File: `src/categories/tests.rs`

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{test, App};
    use sqlx::PgPool;

    // ==================== UNIT TESTS ====================

    mod validation_tests {
        use super::*;

        #[test]
        fn test_name_validation_too_short() {
            let dto = CreateCategoryDto {
                budget_id: Uuid::new_v4(),
                name: "".to_string(),
                allocated_amount: 100.0,
                color_hex: "#64748b".to_string(),
            };
            assert!(dto.validate().is_err());
        }

        #[test]
        fn test_name_validation_too_long() {
            let dto = CreateCategoryDto {
                budget_id: Uuid::new_v4(),
                name: "a".repeat(51),
                allocated_amount: 100.0,
                color_hex: "#64748b".to_string(),
            };
            assert!(dto.validate().is_err());
        }

        #[test]
        fn test_name_validation_valid() {
            let dto = CreateCategoryDto {
                budget_id: Uuid::new_v4(),
                name: "Groceries".to_string(),
                allocated_amount: 100.0,
                color_hex: "#64748b".to_string(),
            };
            assert!(dto.validate().is_ok());
        }

        #[test]
        fn test_color_hex_invalid_format() {
            let dto = CreateCategoryDto {
                budget_id: Uuid::new_v4(),
                name: "Test".to_string(),
                allocated_amount: 100.0,
                color_hex: "64748b".to_string(), // Missing #
            };
            assert!(dto.validate().is_err());
        }

        #[test]
        fn test_color_hex_invalid_chars() {
            let dto = CreateCategoryDto {
                budget_id: Uuid::new_v4(),
                name: "Test".to_string(),
                allocated_amount: 100.0,
                color_hex: "#GGGGGG".to_string(),
            };
            assert!(dto.validate().is_err());
        }

        #[test]
        fn test_color_hex_valid() {
            let dto = CreateCategoryDto {
                budget_id: Uuid::new_v4(),
                name: "Test".to_string(),
                allocated_amount: 100.0,
                color_hex: "#FF5733".to_string(),
            };
            assert!(dto.validate().is_ok());
        }

        #[test]
        fn test_allocated_amount_negative() {
            let dto = CreateCategoryDto {
                budget_id: Uuid::new_v4(),
                name: "Test".to_string(),
                allocated_amount: -50.0,
                color_hex: "#64748b".to_string(),
            };
            assert!(dto.validate().is_err());
        }

        #[test]
        fn test_allocated_amount_zero_valid() {
            let dto = CreateCategoryDto {
                budget_id: Uuid::new_v4(),
                name: "Test".to_string(),
                allocated_amount: 0.0,
                color_hex: "#64748b".to_string(),
            };
            assert!(dto.validate().is_ok());
        }
    }

    mod response_dto_tests {
        use super::*;
        use bigdecimal::FromPrimitive;

        #[test]
        fn test_remaining_amount_calculation() {
            let category = CategoryWithSpent {
                id: Uuid::new_v4(),
                budget_id: Uuid::new_v4(),
                name: "Test".to_string(),
                allocated_amount: BigDecimal::from_f64(500.0).unwrap(),
                spent_amount: BigDecimal::from_f64(200.0).unwrap(),
                color_hex: "#64748b".to_string(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };

            let dto = CategoryResponseDto::from_category_with_spent(category);

            assert_eq!(dto.allocated_amount, 500.0);
            assert_eq!(dto.spent_amount, 200.0);
            assert_eq!(dto.remaining_amount, 300.0);
        }

        #[test]
        fn test_overspent_category() {
            let category = CategoryWithSpent {
                id: Uuid::new_v4(),
                budget_id: Uuid::new_v4(),
                name: "Overspent".to_string(),
                allocated_amount: BigDecimal::from_f64(100.0).unwrap(),
                spent_amount: BigDecimal::from_f64(150.0).unwrap(),
                color_hex: "#64748b".to_string(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };

            let dto = CategoryResponseDto::from_category_with_spent(category);

            assert_eq!(dto.remaining_amount, -50.0);
        }
    }

    // ==================== INTEGRATION TESTS ====================

    mod integration_tests {
        use super::*;

        async fn setup_test_db() -> PgPool {
            // Use test database URL
            let database_url = std::env::var("TEST_DATABASE_URL")
                .unwrap_or_else(|_| "postgres://test:test@localhost/budgetflow_test".to_string());

            PgPool::connect(&database_url).await.unwrap()
        }

        async fn create_test_user(pool: &PgPool) -> Uuid {
            let user_id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO users (id, email, password_hash, full_name) VALUES ($1, $2, $3, $4)"
            )
            .bind(user_id)
            .bind(format!("test-{}@example.com", user_id))
            .bind("hash")
            .bind("Test User")
            .execute(pool)
            .await
            .unwrap();
            user_id
        }

        async fn create_test_budget(pool: &PgPool, owner_id: Uuid) -> Uuid {
            let budget_id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO budgets (id, owner_id, month, year) VALUES ($1, $2, $3, $4)"
            )
            .bind(budget_id)
            .bind(owner_id)
            .bind(1i16)
            .bind(2024i16)
            .execute(pool)
            .await
            .unwrap();
            budget_id
        }

        #[actix_rt::test]
        async fn test_create_category_success() {
            let pool = setup_test_db().await;
            let user_id = create_test_user(&pool).await;
            let budget_id = create_test_budget(&pool, user_id).await;

            let dto = CreateCategoryDto {
                budget_id,
                name: "Groceries".to_string(),
                allocated_amount: 500.0,
                color_hex: "#22c55e".to_string(),
            };

            let result = CategoryService::create(&pool, dto, user_id).await;
            assert!(result.is_ok());

            let category = result.unwrap();
            assert_eq!(category.name, "Groceries");
            assert_eq!(category.budget_id, budget_id);
        }

        #[actix_rt::test]
        async fn test_create_category_unauthorized_budget() {
            let pool = setup_test_db().await;
            let user_id = create_test_user(&pool).await;
            let other_user_id = create_test_user(&pool).await;
            let budget_id = create_test_budget(&pool, other_user_id).await;

            let dto = CreateCategoryDto {
                budget_id,
                name: "Groceries".to_string(),
                allocated_amount: 500.0,
                color_hex: "#22c55e".to_string(),
            };

            let result = CategoryService::create(&pool, dto, user_id).await;
            assert!(result.is_err());
        }

        #[actix_rt::test]
        async fn test_spent_amount_calculation() {
            let pool = setup_test_db().await;
            let user_id = create_test_user(&pool).await;
            let budget_id = create_test_budget(&pool, user_id).await;

            // Create category
            let dto = CreateCategoryDto {
                budget_id,
                name: "Food".to_string(),
                allocated_amount: 300.0,
                color_hex: "#64748b".to_string(),
            };
            let category = CategoryService::create(&pool, dto, user_id).await.unwrap();

            // Add expense transactions
            sqlx::query(
                "INSERT INTO transactions (id, category_id, amount, date, type)
                 VALUES ($1, $2, $3, $4, 'expense')"
            )
            .bind(Uuid::new_v4())
            .bind(category.id)
            .bind(BigDecimal::from_f64(50.0).unwrap())
            .bind(1704067200i64) // Jan 1, 2024
            .execute(&pool)
            .await
            .unwrap();

            sqlx::query(
                "INSERT INTO transactions (id, category_id, amount, date, type)
                 VALUES ($1, $2, $3, $4, 'expense')"
            )
            .bind(Uuid::new_v4())
            .bind(category.id)
            .bind(BigDecimal::from_f64(75.50).unwrap())
            .bind(1704153600i64)
            .execute(&pool)
            .await
            .unwrap();

            // Verify spent amount
            let fetched = CategoryService::get_by_id(&pool, category.id, user_id)
                .await
                .unwrap();

            let dto = CategoryResponseDto::from_category_with_spent(fetched);
            assert_eq!(dto.spent_amount, 125.50);
            assert_eq!(dto.remaining_amount, 174.50);
        }

        #[actix_rt::test]
        async fn test_income_not_counted_as_spent() {
            let pool = setup_test_db().await;
            let user_id = create_test_user(&pool).await;
            let budget_id = create_test_budget(&pool, user_id).await;

            let dto = CreateCategoryDto {
                budget_id,
                name: "Salary".to_string(),
                allocated_amount: 0.0,
                color_hex: "#64748b".to_string(),
            };
            let category = CategoryService::create(&pool, dto, user_id).await.unwrap();

            // Add income transaction
            sqlx::query(
                "INSERT INTO transactions (id, category_id, amount, date, type)
                 VALUES ($1, $2, $3, $4, 'income')"
            )
            .bind(Uuid::new_v4())
            .bind(category.id)
            .bind(BigDecimal::from_f64(5000.0).unwrap())
            .bind(1704067200i64)
            .execute(&pool)
            .await
            .unwrap();

            let fetched = CategoryService::get_by_id(&pool, category.id, user_id)
                .await
                .unwrap();

            let dto = CategoryResponseDto::from_category_with_spent(fetched);
            // Income should NOT be counted as spent
            assert_eq!(dto.spent_amount, 0.0);
        }

        #[actix_rt::test]
        async fn test_delete_cascades_transactions() {
            let pool = setup_test_db().await;
            let user_id = create_test_user(&pool).await;
            let budget_id = create_test_budget(&pool, user_id).await;

            let dto = CreateCategoryDto {
                budget_id,
                name: "To Delete".to_string(),
                allocated_amount: 100.0,
                color_hex: "#64748b".to_string(),
            };
            let category = CategoryService::create(&pool, dto, user_id).await.unwrap();
            let category_id = category.id;

            // Add transaction
            let tx_id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO transactions (id, category_id, amount, date, type)
                 VALUES ($1, $2, $3, $4, 'expense')"
            )
            .bind(tx_id)
            .bind(category_id)
            .bind(BigDecimal::from_f64(50.0).unwrap())
            .bind(1704067200i64)
            .execute(&pool)
            .await
            .unwrap();

            // Delete category
            CategoryService::delete(&pool, category_id, user_id).await.unwrap();

            // Verify transaction is also deleted
            let tx_count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM transactions WHERE id = $1"
            )
            .bind(tx_id)
            .fetch_one(&pool)
            .await
            .unwrap();

            assert_eq!(tx_count, 0);
        }

        #[actix_rt::test]
        async fn test_update_partial_fields() {
            let pool = setup_test_db().await;
            let user_id = create_test_user(&pool).await;
            let budget_id = create_test_budget(&pool, user_id).await;

            let create_dto = CreateCategoryDto {
                budget_id,
                name: "Original".to_string(),
                allocated_amount: 100.0,
                color_hex: "#64748b".to_string(),
            };
            let category = CategoryService::create(&pool, create_dto, user_id).await.unwrap();

            // Update only name
            let update_dto = UpdateCategoryDto {
                name: Some("Updated".to_string()),
                allocated_amount: None,
                color_hex: None,
            };

            let updated = CategoryService::update(&pool, category.id, update_dto, user_id)
                .await
                .unwrap();

            assert_eq!(updated.name, "Updated");
            // Other fields unchanged
            let dto = CategoryResponseDto::from_category_with_spent(updated);
            assert_eq!(dto.allocated_amount, 100.0);
            assert_eq!(dto.color_hex, "#64748b");
        }

        #[actix_rt::test]
        async fn test_name_trimmed_on_create() {
            let pool = setup_test_db().await;
            let user_id = create_test_user(&pool).await;
            let budget_id = create_test_budget(&pool, user_id).await;

            let dto = CreateCategoryDto {
                budget_id,
                name: "  Groceries  ".to_string(),
                allocated_amount: 100.0,
                color_hex: "#64748b".to_string(),
            };

            let category = CategoryService::create(&pool, dto, user_id).await.unwrap();
            assert_eq!(category.name, "Groceries");
        }
    }

    // ==================== HTTP HANDLER TESTS ====================

    mod handler_tests {
        use super::*;
        use actix_web::http::StatusCode;

        #[actix_rt::test]
        async fn test_list_categories_unauthorized() {
            let app = test::init_service(
                App::new()
                    .app_data(web::Data::new(setup_test_db().await))
                    .app_data(web::Data::new("secret".to_string()))
                    .service(list_categories)
            ).await;

            let req = test::TestRequest::get()
                .uri("/categories")
                .to_request();

            let resp = test::call_service(&app, req).await;
            assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        }

        #[actix_rt::test]
        async fn test_create_category_validation_error() {
            let pool = setup_test_db().await;
            let user_id = create_test_user(&pool).await;
            let token = create_test_token(user_id);

            let app = test::init_service(
                App::new()
                    .app_data(web::Data::new(pool))
                    .app_data(web::Data::new("secret".to_string()))
                    .service(create_category)
            ).await;

            let req = test::TestRequest::post()
                .uri("/categories")
                .insert_header(("Authorization", format!("Bearer {}", token)))
                .set_json(serde_json::json!({
                    "budget_id": Uuid::new_v4(),
                    "name": "",  // Invalid: empty
                    "allocated_amount": 100.0
                }))
                .to_request();

            let resp = test::call_service(&app, req).await;
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        }

        fn create_test_token(user_id: Uuid) -> String {
            use crate::auth::create_token;
            create_token(user_id, "secret").unwrap()
        }
    }
}
```

---

## 11. API Response Examples

### GET /categories

```json
[
  {
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "budget_id": "660e8400-e29b-41d4-a716-446655440001",
    "name": "Groceries",
    "allocated_amount": 500.00,
    "spent_amount": 325.50,
    "remaining_amount": 174.50,
    "color_hex": "#22c55e",
    "created_at": "2024-01-15T10:30:00Z",
    "updated_at": "2024-01-15T10:30:00Z"
  },
  {
    "id": "550e8400-e29b-41d4-a716-446655440002",
    "budget_id": "660e8400-e29b-41d4-a716-446655440001",
    "name": "Entertainment",
    "allocated_amount": 200.00,
    "spent_amount": 250.00,
    "remaining_amount": -50.00,
    "color_hex": "#ef4444",
    "created_at": "2024-01-15T10:30:00Z",
    "updated_at": "2024-01-15T10:30:00Z"
  }
]
```

### POST /categories (Request)

```json
{
  "budget_id": "660e8400-e29b-41d4-a716-446655440001",
  "name": "Utilities",
  "allocated_amount": 150.00,
  "color_hex": "#3b82f6"
}
```

### POST /categories (Response - 201 Created)

```json
{
  "id": "550e8400-e29b-41d4-a716-446655440003",
  "budget_id": "660e8400-e29b-41d4-a716-446655440001",
  "name": "Utilities",
  "allocated_amount": 150.00,
  "spent_amount": 0.00,
  "remaining_amount": 150.00,
  "color_hex": "#3b82f6",
  "created_at": "2024-01-15T11:00:00Z",
  "updated_at": "2024-01-15T11:00:00Z"
}
```

### Error Response (404 Not Found)

```json
{
  "error": "NOT_FOUND",
  "message": "Category not found"
}
```

### Error Response (400 Validation)

```json
{
  "error": "VALIDATION_ERROR",
  "message": "name: Name must be 1-50 characters"
}
```

---

## 12. Implementation Checklist

- [ ] Add `bigdecimal`, `lazy_static`, `regex` to `Cargo.toml`
- [ ] Create migration file for categories table
- [ ] Add `NotFound` variant to `AppError`
- [ ] Create `src/categories/` directory structure
- [ ] Implement `src/categories/models.rs`
- [ ] Implement `src/categories/service.rs`
- [ ] Implement `src/categories/handlers.rs`
- [ ] Create `src/categories/mod.rs`
- [ ] Register routes in `src/main.rs`
- [ ] Run migration: `sqlx migrate run`
- [ ] Write and run tests
- [ ] Test with curl/Postman

---

## 13. Prerequisites (Dependencies)

Before implementing categories, ensure:

1. **Budgets table exists** - Categories reference budgets via foreign key
2. **Transactions table exists** - For spent_amount calculation (can be added later, queries will return 0)

If budgets table doesn't exist yet, create a migration first:

```sql
CREATE TABLE budgets (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    month SMALLINT NOT NULL CHECK (month >= 0 AND month <= 11),
    year SMALLINT NOT NULL CHECK (year >= 2000 AND year <= 2100),
    total_income NUMERIC(12,2) NOT NULL DEFAULT 0 CHECK (total_income >= 0),
    savings_rate NUMERIC(5,2) NOT NULL DEFAULT 0 CHECK (savings_rate >= 0 AND savings_rate <= 100),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (owner_id, month, year)
);

CREATE INDEX idx_budgets_owner_id ON budgets(owner_id);
```
