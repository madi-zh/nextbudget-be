# Budget Management Feature - Detailed Implementation Plan

## Overview

This document outlines the detailed implementation plan for Feature 3: Budget Management in the Rust Actix-web backend. The feature allows users to create and manage monthly budgets with income tracking and savings rate configuration.

---

## 1. File Structure

```
src/
├── main.rs                  # Add budget module and routes
├── auth.rs                  # Existing (extract helper for reuse)
├── errors.rs                # Add NotFound variant
├── models.rs                # Existing user models
├── budget/
│   ├── mod.rs               # Module exports
│   ├── models.rs            # Budget entity and DTOs
│   ├── handlers.rs          # HTTP endpoint handlers
│   └── service.rs           # Business logic layer
└── extractors/
    ├── mod.rs               # Module exports
    └── auth.rs              # Reusable AuthenticatedUser extractor

migrations/
└── 20260111000000_create_budgets_table.sql
```

---

## 2. Database Migration

**File:** `migrations/20260111000000_create_budgets_table.sql`

```sql
-- Create budgets table
CREATE TABLE IF NOT EXISTS budgets (
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

-- Index for efficient owner queries
CREATE INDEX idx_budgets_owner_id ON budgets(owner_id);

-- Index for month/year lookups
CREATE INDEX idx_budgets_owner_month_year ON budgets(owner_id, month, year);
```

---

## 3. Error Handling Updates

**File:** `src/errors.rs`

Add `NotFound` variant to support 404 responses:

```rust
use actix_web::{HttpResponse, ResponseError};
use serde::Serialize;
use std::fmt;

#[derive(Debug)]
pub enum AppError {
    ValidationError(String),
    Unauthorized(String),
    NotFound(String),           // NEW
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
            AppError::NotFound(msg) => write!(f, "Not found: {}", msg),  // NEW
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
            AppError::NotFound(msg) => (                               // NEW
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
```

---

## 4. Authentication Extractor

**File:** `src/extractors/mod.rs`

```rust
mod auth;

pub use auth::AuthenticatedUser;
```

**File:** `src/extractors/auth.rs`

Create a reusable extractor for authenticated requests:

```rust
use actix_web::{dev::Payload, web, FromRequest, HttpRequest};
use futures::future::{err, ok, Ready};
use uuid::Uuid;

use crate::auth::decode_token;
use crate::errors::AppError;

/// Extractor that validates JWT and provides the authenticated user's ID.
///
/// Usage in handlers:
/// ```rust
/// async fn my_handler(auth: AuthenticatedUser) -> Result<HttpResponse, AppError> {
///     let user_id = auth.user_id;
///     // ...
/// }
/// ```
pub struct AuthenticatedUser {
    pub user_id: Uuid,
}

impl FromRequest for AuthenticatedUser {
    type Error = AppError;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        // Extract JWT secret from app data
        let jwt_secret = match req.app_data::<web::Data<String>>() {
            Some(secret) => secret.get_ref().clone(),
            None => {
                return err(AppError::InternalError(
                    "JWT secret not configured".to_string(),
                ))
            }
        };

        // Extract token from Authorization header
        let token = match req
            .headers()
            .get("Authorization")
            .and_then(|h| h.to_str().ok())
            .and_then(|h| h.strip_prefix("Bearer "))
        {
            Some(t) => t.to_string(),
            None => {
                return err(AppError::Unauthorized(
                    "Missing or invalid Authorization header".to_string(),
                ))
            }
        };

        // Decode and validate token
        match decode_token(&token, &jwt_secret) {
            Ok(claims) => ok(AuthenticatedUser {
                user_id: claims.sub,
            }),
            Err(e) => err(e),
        }
    }
}
```

---

## 5. Budget Models and DTOs

**File:** `src/budget/models.rs`

```rust
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use validator::Validate;

/// Database entity for budgets
#[derive(Debug, Clone, FromRow)]
pub struct Budget {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub month: i16,
    pub year: i16,
    pub total_income: Decimal,
    pub savings_rate: Decimal,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Response DTO with computed fields.
///
/// Computed fields:
/// - `savings_target`: total_income * (savings_rate / 100)
/// - `spending_budget`: total_income - savings_target
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetResponse {
    pub id: Uuid,
    pub month: i16,
    pub year: i16,
    pub total_income: Decimal,
    pub savings_rate: Decimal,
    pub savings_target: Decimal,
    pub spending_budget: Decimal,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl BudgetResponse {
    pub fn from_budget(budget: Budget) -> Self {
        let hundred = Decimal::from(100);
        let savings_target = budget.total_income * budget.savings_rate / hundred;
        let spending_budget = budget.total_income - savings_target;

        Self {
            id: budget.id,
            month: budget.month,
            year: budget.year,
            total_income: budget.total_income,
            savings_rate: budget.savings_rate,
            savings_target,
            spending_budget,
            created_at: budget.created_at,
            updated_at: budget.updated_at,
        }
    }
}

/// DTO for creating a new budget
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct CreateBudgetDto {
    #[validate(range(min = 0, max = 11, message = "Month must be between 0 and 11"))]
    pub month: i16,

    #[validate(range(min = 2000, max = 2100, message = "Year must be between 2000 and 2100"))]
    pub year: i16,

    #[validate(range(min = 0.0, message = "Total income must be non-negative"))]
    #[serde(default)]
    pub total_income: Option<Decimal>,

    #[validate(range(min = 0.0, max = 100.0, message = "Savings rate must be between 0 and 100"))]
    #[serde(default)]
    pub savings_rate: Option<Decimal>,
}

/// DTO for updating a budget (all fields optional for PATCH semantics)
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct UpdateBudgetDto {
    #[validate(range(min = 0, max = 11, message = "Month must be between 0 and 11"))]
    pub month: Option<i16>,

    #[validate(range(min = 2000, max = 2100, message = "Year must be between 2000 and 2100"))]
    pub year: Option<i16>,

    #[validate(range(min = 0.0, message = "Total income must be non-negative"))]
    pub total_income: Option<Decimal>,

    #[validate(range(min = 0.0, max = 100.0, message = "Savings rate must be between 0 and 100"))]
    pub savings_rate: Option<Decimal>,
}

/// DTO for updating income only
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct UpdateIncomeDto {
    #[validate(range(min = 0.0, message = "Total income must be non-negative"))]
    pub total_income: Decimal,
}

/// DTO for updating savings rate only
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSavingsRateDto {
    #[validate(range(min = 0.0, max = 100.0, message = "Savings rate must be between 0 and 100"))]
    pub savings_rate: Decimal,
}

/// Path parameters for budget ID
#[derive(Debug, Deserialize)]
pub struct BudgetIdPath {
    pub id: Uuid,
}

/// Path parameters for month/year lookup
#[derive(Debug, Deserialize, Validate)]
pub struct MonthYearPath {
    #[validate(range(min = 0, max = 11, message = "Month must be between 0 and 11"))]
    pub month: i16,

    #[validate(range(min = 2000, max = 2100, message = "Year must be between 2000 and 2100"))]
    pub year: i16,
}

/// Query parameters for listing budgets
#[derive(Debug, Deserialize, Validate)]
pub struct ListBudgetsQuery {
    #[validate(range(min = 2000, max = 2100))]
    pub year: Option<i16>,

    #[validate(range(min = 1, max = 100))]
    #[serde(default = "default_limit")]
    pub limit: i64,

    #[validate(range(min = 0))]
    #[serde(default)]
    pub offset: i64,
}

fn default_limit() -> i64 {
    20
}
```

---

## 6. Budget Service Layer

**File:** `src/budget/service.rs`

```rust
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use crate::errors::AppError;
use super::models::{
    Budget, CreateBudgetDto, ListBudgetsQuery, UpdateBudgetDto,
    UpdateIncomeDto, UpdateSavingsRateDto,
};

/// Service layer for budget business logic.
///
/// All methods enforce ownership checks - users can only access their own budgets.
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
                SELECT id, owner_id, month, year, total_income, savings_rate, created_at, updated_at
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
                SELECT id, owner_id, month, year, total_income, savings_rate, created_at, updated_at
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
    ///
    /// Returns NotFound if the budget doesn't exist OR if it belongs to another user
    /// (to prevent information leakage about existence).
    pub async fn get_budget_by_id(
        pool: &PgPool,
        budget_id: Uuid,
        owner_id: Uuid,
    ) -> Result<Budget, AppError> {
        sqlx::query_as::<_, Budget>(
            r#"
            SELECT id, owner_id, month, year, total_income, savings_rate, created_at, updated_at
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
            SELECT id, owner_id, month, year, total_income, savings_rate, created_at, updated_at
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
        .ok_or_else(|| {
            AppError::NotFound(format!("Budget not found for {}/{}", month + 1, year))
        })
    }

    /// Create a new budget.
    ///
    /// Enforces unique constraint on (owner_id, month, year).
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

        sqlx::query_as::<_, Budget>(
            r#"
            INSERT INTO budgets (owner_id, month, year, total_income, savings_rate)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING id, owner_id, month, year, total_income, savings_rate, created_at, updated_at
            "#,
        )
        .bind(owner_id)
        .bind(dto.month)
        .bind(dto.year)
        .bind(total_income)
        .bind(savings_rate)
        .fetch_one(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))
    }

    /// Update a budget (partial update - PATCH semantics).
    ///
    /// If month/year is changed, enforces unique constraint.
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
            RETURNING id, owner_id, month, year, total_income, savings_rate, created_at, updated_at
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

    /// Update only the income field.
    pub async fn update_income(
        pool: &PgPool,
        budget_id: Uuid,
        owner_id: Uuid,
        dto: &UpdateIncomeDto,
    ) -> Result<Budget, AppError> {
        // Verify ownership first
        let _ = Self::get_budget_by_id(pool, budget_id, owner_id).await?;

        sqlx::query_as::<_, Budget>(
            r#"
            UPDATE budgets
            SET total_income = $1, updated_at = NOW()
            WHERE id = $2 AND owner_id = $3
            RETURNING id, owner_id, month, year, total_income, savings_rate, created_at, updated_at
            "#,
        )
        .bind(dto.total_income)
        .bind(budget_id)
        .bind(owner_id)
        .fetch_one(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))
    }

    /// Update only the savings rate field.
    pub async fn update_savings_rate(
        pool: &PgPool,
        budget_id: Uuid,
        owner_id: Uuid,
        dto: &UpdateSavingsRateDto,
    ) -> Result<Budget, AppError> {
        // Verify ownership first
        let _ = Self::get_budget_by_id(pool, budget_id, owner_id).await?;

        sqlx::query_as::<_, Budget>(
            r#"
            UPDATE budgets
            SET savings_rate = $1, updated_at = NOW()
            WHERE id = $2 AND owner_id = $3
            RETURNING id, owner_id, month, year, total_income, savings_rate, created_at, updated_at
            "#,
        )
        .bind(dto.savings_rate)
        .bind(budget_id)
        .bind(owner_id)
        .fetch_one(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))
    }

    /// Delete a budget.
    ///
    /// Note: CASCADE constraint will delete associated categories and transactions.
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
```

---

## 7. Budget Handlers

**File:** `src/budget/handlers.rs`

```rust
use actix_web::{delete, get, patch, post, web, HttpResponse};
use sqlx::PgPool;
use validator::Validate;

use crate::errors::AppError;
use crate::extractors::AuthenticatedUser;

use super::models::{
    BudgetIdPath, BudgetResponse, CreateBudgetDto, ListBudgetsQuery,
    MonthYearPath, UpdateBudgetDto, UpdateIncomeDto, UpdateSavingsRateDto,
};
use super::service::BudgetService;

/// GET /budgets - List all budgets for the authenticated user
///
/// Query parameters:
/// - `year` (optional): Filter by year
/// - `limit` (optional, default 20): Max results
/// - `offset` (optional, default 0): Pagination offset
#[get("/budgets")]
pub async fn list_budgets(
    pool: web::Data<PgPool>,
    auth: AuthenticatedUser,
    query: web::Query<ListBudgetsQuery>,
) -> Result<HttpResponse, AppError> {
    query
        .validate()
        .map_err(|e| AppError::ValidationError(e.to_string()))?;

    let budgets = BudgetService::list_budgets(pool.get_ref(), auth.user_id, &query).await?;

    let response: Vec<BudgetResponse> = budgets
        .into_iter()
        .map(BudgetResponse::from_budget)
        .collect();

    Ok(HttpResponse::Ok().json(response))
}

/// GET /budgets/{id} - Get a specific budget by ID
#[get("/budgets/{id}")]
pub async fn get_budget(
    pool: web::Data<PgPool>,
    auth: AuthenticatedUser,
    path: web::Path<BudgetIdPath>,
) -> Result<HttpResponse, AppError> {
    let budget = BudgetService::get_budget_by_id(pool.get_ref(), path.id, auth.user_id).await?;

    Ok(HttpResponse::Ok().json(BudgetResponse::from_budget(budget)))
}

/// GET /budgets/month/{month}/year/{year} - Get budget for specific month/year
///
/// Month is 0-indexed (0 = January, 11 = December)
#[get("/budgets/month/{month}/year/{year}")]
pub async fn get_budget_by_month_year(
    pool: web::Data<PgPool>,
    auth: AuthenticatedUser,
    path: web::Path<MonthYearPath>,
) -> Result<HttpResponse, AppError> {
    path.validate()
        .map_err(|e| AppError::ValidationError(e.to_string()))?;

    let budget = BudgetService::get_budget_by_month_year(
        pool.get_ref(),
        auth.user_id,
        path.month,
        path.year,
    )
    .await?;

    Ok(HttpResponse::Ok().json(BudgetResponse::from_budget(budget)))
}

/// POST /budgets - Create a new budget
///
/// Request body:
/// - `month` (required): 0-11
/// - `year` (required): 2000-2100
/// - `totalIncome` (optional, default 0): Non-negative decimal
/// - `savingsRate` (optional, default 0): 0-100
#[post("/budgets")]
pub async fn create_budget(
    pool: web::Data<PgPool>,
    auth: AuthenticatedUser,
    body: web::Json<CreateBudgetDto>,
) -> Result<HttpResponse, AppError> {
    body.validate()
        .map_err(|e| AppError::ValidationError(e.to_string()))?;

    let budget = BudgetService::create_budget(pool.get_ref(), auth.user_id, &body).await?;

    Ok(HttpResponse::Created().json(BudgetResponse::from_budget(budget)))
}

/// PATCH /budgets/{id} - Update a budget (partial update)
///
/// All fields are optional. Only provided fields will be updated.
#[patch("/budgets/{id}")]
pub async fn update_budget(
    pool: web::Data<PgPool>,
    auth: AuthenticatedUser,
    path: web::Path<BudgetIdPath>,
    body: web::Json<UpdateBudgetDto>,
) -> Result<HttpResponse, AppError> {
    body.validate()
        .map_err(|e| AppError::ValidationError(e.to_string()))?;

    let budget =
        BudgetService::update_budget(pool.get_ref(), path.id, auth.user_id, &body).await?;

    Ok(HttpResponse::Ok().json(BudgetResponse::from_budget(budget)))
}

/// PATCH /budgets/{id}/income - Update income only
///
/// Request body:
/// - `totalIncome` (required): Non-negative decimal
#[patch("/budgets/{id}/income")]
pub async fn update_income(
    pool: web::Data<PgPool>,
    auth: AuthenticatedUser,
    path: web::Path<BudgetIdPath>,
    body: web::Json<UpdateIncomeDto>,
) -> Result<HttpResponse, AppError> {
    body.validate()
        .map_err(|e| AppError::ValidationError(e.to_string()))?;

    let budget =
        BudgetService::update_income(pool.get_ref(), path.id, auth.user_id, &body).await?;

    Ok(HttpResponse::Ok().json(BudgetResponse::from_budget(budget)))
}

/// PATCH /budgets/{id}/savings-rate - Update savings rate only
///
/// Request body:
/// - `savingsRate` (required): 0-100
#[patch("/budgets/{id}/savings-rate")]
pub async fn update_savings_rate(
    pool: web::Data<PgPool>,
    auth: AuthenticatedUser,
    path: web::Path<BudgetIdPath>,
    body: web::Json<UpdateSavingsRateDto>,
) -> Result<HttpResponse, AppError> {
    body.validate()
        .map_err(|e| AppError::ValidationError(e.to_string()))?;

    let budget =
        BudgetService::update_savings_rate(pool.get_ref(), path.id, auth.user_id, &body).await?;

    Ok(HttpResponse::Ok().json(BudgetResponse::from_budget(budget)))
}

/// DELETE /budgets/{id} - Delete a budget
///
/// Note: This will cascade delete all associated categories and transactions.
#[delete("/budgets/{id}")]
pub async fn delete_budget(
    pool: web::Data<PgPool>,
    auth: AuthenticatedUser,
    path: web::Path<BudgetIdPath>,
) -> Result<HttpResponse, AppError> {
    BudgetService::delete_budget(pool.get_ref(), path.id, auth.user_id).await?;

    Ok(HttpResponse::NoContent().finish())
}
```

**File:** `src/budget/mod.rs`

```rust
pub mod handlers;
pub mod models;
pub mod service;

pub use handlers::*;
```

---

## 8. Main.rs Updates

**File:** `src/main.rs`

```rust
mod auth;
mod budget;        // NEW
mod errors;
mod extractors;    // NEW
mod models;

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
            // Auth endpoints
            .service(auth::register)
            .service(auth::login)
            .service(auth::me)
            // Budget endpoints (NEW)
            .service(budget::list_budgets)
            .service(budget::get_budget)
            .service(budget::get_budget_by_month_year)
            .service(budget::create_budget)
            .service(budget::update_budget)
            .service(budget::update_income)
            .service(budget::update_savings_rate)
            .service(budget::delete_budget)
    })
    .bind(("0.0.0.0", 8080))?
    .run()
    .await
}
```

---

## 9. Cargo.toml Updates

Add the following dependencies:

```toml
[dependencies]
# ... existing dependencies ...
rust_decimal = { version = "1.33", features = ["db-postgres", "serde-with-str"] }
futures = "0.3"  # For the AuthenticatedUser extractor
```

**Note:** The `rust_decimal` crate is used for precise financial calculations. The `serde-with-str` feature serializes decimals as strings in JSON to preserve precision.

---

## 10. Authorization Summary

All authorization is handled via the `AuthenticatedUser` extractor and SQL WHERE clauses:

| Endpoint | Authorization Method |
|----------|---------------------|
| `GET /budgets` | Extractor validates JWT; SQL filters by `owner_id` |
| `GET /budgets/:id` | Extractor validates JWT; SQL WHERE includes `owner_id` |
| `GET /budgets/month/:month/year/:year` | Extractor validates JWT; SQL WHERE includes `owner_id` |
| `POST /budgets` | Extractor validates JWT; `owner_id` set from JWT claims |
| `PATCH /budgets/:id` | Extractor validates JWT; ownership verified in service |
| `PATCH /budgets/:id/income` | Extractor validates JWT; ownership verified in service |
| `PATCH /budgets/:id/savings-rate` | Extractor validates JWT; ownership verified in service |
| `DELETE /budgets/:id` | Extractor validates JWT; SQL WHERE includes `owner_id` |

**Security Note:** When a user attempts to access a budget they don't own, the API returns 404 (not 403) to prevent information leakage about the existence of resources.

---

## 11. Input Validation Summary

| Field | Validation Rules | Error Message |
|-------|------------------|---------------|
| `month` | Range 0-11 | "Month must be between 0 and 11" |
| `year` | Range 2000-2100 | "Year must be between 2000 and 2100" |
| `total_income` | >= 0 | "Total income must be non-negative" |
| `savings_rate` | Range 0.0-100.0 | "Savings rate must be between 0 and 100" |
| `limit` (query) | Range 1-100 | Default: 20 |
| `offset` (query) | >= 0 | Default: 0 |

---

## 12. Test Cases

### Unit Tests

**File:** `src/budget/models.rs` (add at end with `#[cfg(test)]`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_budget_response_computes_savings_target() {
        let budget = Budget {
            id: Uuid::new_v4(),
            owner_id: Uuid::new_v4(),
            month: 0,
            year: 2024,
            total_income: dec!(5000.00),
            savings_rate: dec!(20.00),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let response = BudgetResponse::from_budget(budget);

        assert_eq!(response.savings_target, dec!(1000.00));
        assert_eq!(response.spending_budget, dec!(4000.00));
    }

    #[test]
    fn test_budget_response_zero_savings_rate() {
        let budget = Budget {
            id: Uuid::new_v4(),
            owner_id: Uuid::new_v4(),
            month: 5,
            year: 2024,
            total_income: dec!(3000.00),
            savings_rate: dec!(0.00),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let response = BudgetResponse::from_budget(budget);

        assert_eq!(response.savings_target, dec!(0.00));
        assert_eq!(response.spending_budget, dec!(3000.00));
    }

    #[test]
    fn test_budget_response_100_percent_savings() {
        let budget = Budget {
            id: Uuid::new_v4(),
            owner_id: Uuid::new_v4(),
            month: 11,
            year: 2024,
            total_income: dec!(10000.00),
            savings_rate: dec!(100.00),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let response = BudgetResponse::from_budget(budget);

        assert_eq!(response.savings_target, dec!(10000.00));
        assert_eq!(response.spending_budget, dec!(0.00));
    }

    #[test]
    fn test_budget_response_fractional_rate() {
        let budget = Budget {
            id: Uuid::new_v4(),
            owner_id: Uuid::new_v4(),
            month: 6,
            year: 2024,
            total_income: dec!(7500.00),
            savings_rate: dec!(15.50),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let response = BudgetResponse::from_budget(budget);

        // 7500 * 0.155 = 1162.50
        assert_eq!(response.savings_target, dec!(1162.50));
        assert_eq!(response.spending_budget, dec!(6337.50));
    }

    #[test]
    fn test_create_budget_dto_valid() {
        let dto = CreateBudgetDto {
            month: 5,
            year: 2024,
            total_income: Some(dec!(5000.00)),
            savings_rate: Some(dec!(20.00)),
        };

        assert!(dto.validate().is_ok());
    }

    #[test]
    fn test_create_budget_dto_invalid_month_high() {
        let dto = CreateBudgetDto {
            month: 12, // Invalid: should be 0-11
            year: 2024,
            total_income: None,
            savings_rate: None,
        };

        assert!(dto.validate().is_err());
    }

    #[test]
    fn test_create_budget_dto_invalid_month_negative() {
        let dto = CreateBudgetDto {
            month: -1, // Invalid: should be >= 0
            year: 2024,
            total_income: None,
            savings_rate: None,
        };

        assert!(dto.validate().is_err());
    }

    #[test]
    fn test_create_budget_dto_invalid_year_low() {
        let dto = CreateBudgetDto {
            month: 5,
            year: 1999, // Invalid: should be >= 2000
            total_income: None,
            savings_rate: None,
        };

        assert!(dto.validate().is_err());
    }

    #[test]
    fn test_create_budget_dto_invalid_year_high() {
        let dto = CreateBudgetDto {
            month: 5,
            year: 2101, // Invalid: should be <= 2100
            total_income: None,
            savings_rate: None,
        };

        assert!(dto.validate().is_err());
    }

    #[test]
    fn test_create_budget_dto_invalid_savings_rate_high() {
        let dto = CreateBudgetDto {
            month: 5,
            year: 2024,
            total_income: Some(dec!(5000.00)),
            savings_rate: Some(dec!(150.00)), // Invalid: > 100
        };

        assert!(dto.validate().is_err());
    }

    #[test]
    fn test_update_income_dto_valid() {
        let dto = UpdateIncomeDto {
            total_income: dec!(6000.00),
        };

        assert!(dto.validate().is_ok());
    }

    #[test]
    fn test_update_savings_rate_dto_valid() {
        let dto = UpdateSavingsRateDto {
            savings_rate: dec!(25.00),
        };

        assert!(dto.validate().is_ok());
    }

    #[test]
    fn test_update_savings_rate_dto_invalid() {
        let dto = UpdateSavingsRateDto {
            savings_rate: dec!(101.00), // Invalid: > 100
        };

        assert!(dto.validate().is_err());
    }
}
```

### Integration Tests

**File:** `tests/budget_integration.rs`

```rust
use actix_web::{test, web, App};
use serde_json::json;
use sqlx::PgPool;

// Test helper: Creates a test app with database
async fn setup_test_app() -> (
    impl actix_web::dev::Service<
        actix_http::Request,
        Response = actix_web::dev::ServiceResponse,
        Error = actix_web::Error,
    >,
    PgPool,
) {
    // Setup test database connection
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let pool = sqlx::PgPool::connect(&database_url).await.unwrap();
    let jwt_secret = "test_secret_key".to_string();

    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .app_data(web::Data::new(jwt_secret))
            .service(be_rust::auth::register)
            .service(be_rust::auth::login)
            .service(be_rust::budget::list_budgets)
            .service(be_rust::budget::get_budget)
            .service(be_rust::budget::get_budget_by_month_year)
            .service(be_rust::budget::create_budget)
            .service(be_rust::budget::update_budget)
            .service(be_rust::budget::update_income)
            .service(be_rust::budget::update_savings_rate)
            .service(be_rust::budget::delete_budget),
    )
    .await;

    (app, pool)
}

// Test helper: Register user and get JWT token
async fn get_auth_token(app: &impl actix_web::dev::Service<...>, email: &str) -> String {
    let req = test::TestRequest::post()
        .uri("/auth/register")
        .set_json(json!({
            "email": email,
            "password": "password123",
            "full_name": "Test User"
        }))
        .to_request();

    let resp: serde_json::Value = test::call_and_read_body_json(app, req).await;
    resp["token"].as_str().unwrap().to_string()
}

#[actix_web::test]
async fn test_create_budget_success() {
    let (app, pool) = setup_test_app().await;
    let token = get_auth_token(&app, "budget_create@test.com").await;

    let req = test::TestRequest::post()
        .uri("/budgets")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .set_json(json!({
            "month": 0,
            "year": 2024,
            "totalIncome": 5000.00,
            "savingsRate": 20.00
        }))
        .to_request();

    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 201);

    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["month"], 0);
    assert_eq!(body["year"], 2024);
    assert_eq!(body["totalIncome"], "5000.00");
    assert_eq!(body["savingsRate"], "20.00");
    assert_eq!(body["savingsTarget"], "1000.00");
    assert_eq!(body["spendingBudget"], "4000.00");

    // Cleanup
    sqlx::query("DELETE FROM users WHERE email = $1")
        .bind("budget_create@test.com")
        .execute(&pool)
        .await
        .unwrap();
}

#[actix_web::test]
async fn test_create_budget_duplicate_month_year() {
    let (app, pool) = setup_test_app().await;
    let token = get_auth_token(&app, "budget_dup@test.com").await;

    // Create first budget
    let req = test::TestRequest::post()
        .uri("/budgets")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .set_json(json!({
            "month": 5,
            "year": 2024
        }))
        .to_request();
    test::call_service(&app, req).await;

    // Try to create duplicate
    let req = test::TestRequest::post()
        .uri("/budgets")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .set_json(json!({
            "month": 5,
            "year": 2024
        }))
        .to_request();

    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 409); // Conflict

    // Cleanup
    sqlx::query("DELETE FROM users WHERE email = $1")
        .bind("budget_dup@test.com")
        .execute(&pool)
        .await
        .unwrap();
}

#[actix_web::test]
async fn test_get_budget_unauthorized() {
    let (app, pool) = setup_test_app().await;

    // User A creates a budget
    let token_a = get_auth_token(&app, "user_a@test.com").await;
    let req = test::TestRequest::post()
        .uri("/budgets")
        .insert_header(("Authorization", format!("Bearer {}", token_a)))
        .set_json(json!({
            "month": 0,
            "year": 2024
        }))
        .to_request();
    let resp: serde_json::Value = test::call_and_read_body_json(&app, req).await;
    let budget_id = resp["id"].as_str().unwrap();

    // User B tries to access it
    let token_b = get_auth_token(&app, "user_b@test.com").await;
    let req = test::TestRequest::get()
        .uri(&format!("/budgets/{}", budget_id))
        .insert_header(("Authorization", format!("Bearer {}", token_b)))
        .to_request();

    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 404); // Not Found (not 403, to prevent info leakage)

    // Cleanup
    sqlx::query("DELETE FROM users WHERE email IN ($1, $2)")
        .bind("user_a@test.com")
        .bind("user_b@test.com")
        .execute(&pool)
        .await
        .unwrap();
}

#[actix_web::test]
async fn test_update_income_only() {
    let (app, pool) = setup_test_app().await;
    let token = get_auth_token(&app, "income_update@test.com").await;

    // Create budget
    let req = test::TestRequest::post()
        .uri("/budgets")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .set_json(json!({
            "month": 3,
            "year": 2024,
            "totalIncome": 5000.00,
            "savingsRate": 20.00
        }))
        .to_request();
    let resp: serde_json::Value = test::call_and_read_body_json(&app, req).await;
    let budget_id = resp["id"].as_str().unwrap();

    // Update income only
    let req = test::TestRequest::patch()
        .uri(&format!("/budgets/{}/income", budget_id))
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .set_json(json!({
            "totalIncome": 6000.00
        }))
        .to_request();

    let resp: serde_json::Value = test::call_and_read_body_json(&app, req).await;
    assert_eq!(resp["totalIncome"], "6000.00");
    assert_eq!(resp["savingsRate"], "20.00"); // Unchanged
    assert_eq!(resp["savingsTarget"], "1200.00"); // Recalculated
    assert_eq!(resp["spendingBudget"], "4800.00"); // Recalculated

    // Cleanup
    sqlx::query("DELETE FROM users WHERE email = $1")
        .bind("income_update@test.com")
        .execute(&pool)
        .await
        .unwrap();
}

#[actix_web::test]
async fn test_list_budgets_filter_by_year() {
    let (app, pool) = setup_test_app().await;
    let token = get_auth_token(&app, "list_filter@test.com").await;

    // Create budgets for 2023 and 2024
    for (month, year) in [(0, 2023), (6, 2023), (0, 2024), (6, 2024)] {
        let req = test::TestRequest::post()
            .uri("/budgets")
            .insert_header(("Authorization", format!("Bearer {}", token)))
            .set_json(json!({
                "month": month,
                "year": year
            }))
            .to_request();
        test::call_service(&app, req).await;
    }

    // Filter by 2024
    let req = test::TestRequest::get()
        .uri("/budgets?year=2024")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();

    let resp: Vec<serde_json::Value> = test::call_and_read_body_json(&app, req).await;
    assert_eq!(resp.len(), 2);
    assert!(resp.iter().all(|b| b["year"] == 2024));

    // Cleanup
    sqlx::query("DELETE FROM users WHERE email = $1")
        .bind("list_filter@test.com")
        .execute(&pool)
        .await
        .unwrap();
}

#[actix_web::test]
async fn test_delete_budget() {
    let (app, pool) = setup_test_app().await;
    let token = get_auth_token(&app, "delete_test@test.com").await;

    // Create budget
    let req = test::TestRequest::post()
        .uri("/budgets")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .set_json(json!({
            "month": 8,
            "year": 2024
        }))
        .to_request();
    let resp: serde_json::Value = test::call_and_read_body_json(&app, req).await;
    let budget_id = resp["id"].as_str().unwrap();

    // Delete budget
    let req = test::TestRequest::delete()
        .uri(&format!("/budgets/{}", budget_id))
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();

    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 204);

    // Verify deletion
    let req = test::TestRequest::get()
        .uri(&format!("/budgets/{}", budget_id))
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();

    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 404);

    // Cleanup
    sqlx::query("DELETE FROM users WHERE email = $1")
        .bind("delete_test@test.com")
        .execute(&pool)
        .await
        .unwrap();
}

#[actix_web::test]
async fn test_get_budget_by_month_year() {
    let (app, pool) = setup_test_app().await;
    let token = get_auth_token(&app, "month_year@test.com").await;

    // Create budget
    let req = test::TestRequest::post()
        .uri("/budgets")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .set_json(json!({
            "month": 5,
            "year": 2024,
            "totalIncome": 7000.00
        }))
        .to_request();
    test::call_service(&app, req).await;

    // Get by month/year
    let req = test::TestRequest::get()
        .uri("/budgets/month/5/year/2024")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .to_request();

    let resp: serde_json::Value = test::call_and_read_body_json(&app, req).await;
    assert_eq!(resp["month"], 5);
    assert_eq!(resp["year"], 2024);
    assert_eq!(resp["totalIncome"], "7000.00");

    // Cleanup
    sqlx::query("DELETE FROM users WHERE email = $1")
        .bind("month_year@test.com")
        .execute(&pool)
        .await
        .unwrap();
}

#[actix_web::test]
async fn test_validation_invalid_month() {
    let (app, pool) = setup_test_app().await;
    let token = get_auth_token(&app, "validation@test.com").await;

    let req = test::TestRequest::post()
        .uri("/budgets")
        .insert_header(("Authorization", format!("Bearer {}", token)))
        .set_json(json!({
            "month": 15,  // Invalid
            "year": 2024
        }))
        .to_request();

    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 400);

    // Cleanup
    sqlx::query("DELETE FROM users WHERE email = $1")
        .bind("validation@test.com")
        .execute(&pool)
        .await
        .unwrap();
}

#[actix_web::test]
async fn test_no_auth_header() {
    let (app, _pool) = setup_test_app().await;

    let req = test::TestRequest::get()
        .uri("/budgets")
        .to_request();

    let resp = test::call_service(&app, req).await;
    assert_eq!(resp.status(), 401);
}
```

---

## 13. API Response Examples

### Create Budget

**Request:**
```http
POST /budgets
Authorization: Bearer <jwt_token>
Content-Type: application/json

{
    "month": 0,
    "year": 2024,
    "totalIncome": 5000.00,
    "savingsRate": 20.00
}
```

**Response (201 Created):**
```json
{
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "month": 0,
    "year": 2024,
    "totalIncome": "5000.00",
    "savingsRate": "20.00",
    "savingsTarget": "1000.00",
    "spendingBudget": "4000.00",
    "createdAt": "2024-01-15T10:30:00Z",
    "updatedAt": "2024-01-15T10:30:00Z"
}
```

### List Budgets

**Request:**
```http
GET /budgets?year=2024&limit=10
Authorization: Bearer <jwt_token>
```

**Response (200 OK):**
```json
[
    {
        "id": "550e8400-e29b-41d4-a716-446655440000",
        "month": 11,
        "year": 2024,
        "totalIncome": "6000.00",
        "savingsRate": "25.00",
        "savingsTarget": "1500.00",
        "spendingBudget": "4500.00",
        "createdAt": "2024-01-15T10:30:00Z",
        "updatedAt": "2024-01-15T10:30:00Z"
    },
    {
        "id": "550e8400-e29b-41d4-a716-446655440001",
        "month": 10,
        "year": 2024,
        "totalIncome": "5500.00",
        "savingsRate": "20.00",
        "savingsTarget": "1100.00",
        "spendingBudget": "4400.00",
        "createdAt": "2024-01-10T08:15:00Z",
        "updatedAt": "2024-01-10T08:15:00Z"
    }
]
```

### Get Budget by Month/Year

**Request:**
```http
GET /budgets/month/5/year/2024
Authorization: Bearer <jwt_token>
```

**Response (200 OK):**
```json
{
    "id": "550e8400-e29b-41d4-a716-446655440002",
    "month": 5,
    "year": 2024,
    "totalIncome": "7500.00",
    "savingsRate": "30.00",
    "savingsTarget": "2250.00",
    "spendingBudget": "5250.00",
    "createdAt": "2024-06-01T00:00:00Z",
    "updatedAt": "2024-06-15T12:00:00Z"
}
```

### Update Budget (Partial)

**Request:**
```http
PATCH /budgets/550e8400-e29b-41d4-a716-446655440000
Authorization: Bearer <jwt_token>
Content-Type: application/json

{
    "savingsRate": 25.00
}
```

**Response (200 OK):**
```json
{
    "id": "550e8400-e29b-41d4-a716-446655440000",
    "month": 0,
    "year": 2024,
    "totalIncome": "5000.00",
    "savingsRate": "25.00",
    "savingsTarget": "1250.00",
    "spendingBudget": "3750.00",
    "createdAt": "2024-01-15T10:30:00Z",
    "updatedAt": "2024-01-16T14:22:00Z"
}
```

### Error Response

**Request:**
```http
POST /budgets
Authorization: Bearer <jwt_token>
Content-Type: application/json

{
    "month": 15,
    "year": 2024
}
```

**Response (400 Bad Request):**
```json
{
    "error": "VALIDATION_ERROR",
    "message": "month: Month must be between 0 and 11"
}
```

### Conflict Response

**Request:**
```http
POST /budgets
Authorization: Bearer <jwt_token>
Content-Type: application/json

{
    "month": 0,
    "year": 2024
}
```

**Response (409 Conflict):**
```json
{
    "error": "CONFLICT",
    "message": "Budget already exists for 1/2024"
}
```

---

## 14. Implementation Order

### Phase 1: Foundation (Day 1)
1. Add `rust_decimal` and `futures` to `Cargo.toml`
2. Add `NotFound` variant to `src/errors.rs`
3. Create `src/extractors/mod.rs` and `src/extractors/auth.rs`
4. Create migration file and run `sqlx migrate run`
5. Verify with `cargo build`

### Phase 2: Core Implementation (Day 1-2)
1. Create `src/budget/models.rs` with all DTOs
2. Create `src/budget/service.rs` with business logic
3. Create `src/budget/handlers.rs` with endpoints
4. Create `src/budget/mod.rs`
5. Verify with `cargo build`

### Phase 3: Integration (Day 2)
1. Update `src/main.rs` with new modules and routes
2. Run `cargo build` to verify compilation
3. Run `cargo clippy` for linting
4. Run `cargo fmt` for formatting

### Phase 4: Testing (Day 2-3)
1. Add unit tests to `src/budget/models.rs`
2. Create `tests/budget_integration.rs`
3. Run `cargo test`
4. Manual API testing with curl/Postman

### Phase 5: Documentation (Day 3)
1. Update `CLAUDE.md` with new endpoints
2. Add OpenAPI/Swagger documentation (optional)

---

## 15. Security Considerations

1. **Ownership Enforcement**: All SQL queries include `owner_id` in WHERE clause
2. **No Data Leakage**: Returns 404 (not 403) when accessing others' budgets
3. **Input Validation**: All inputs validated before database operations via `validator` crate
4. **SQL Injection Prevention**: Using parameterized queries via sqlx's bind
5. **JWT Validation**: Token validated on every protected endpoint via extractor
6. **Decimal Precision**: Using `rust_decimal` for accurate financial calculations (avoids floating-point errors)
7. **HTTPS**: Enforced at deployment level (not in application code)

---

## Summary

This plan provides a complete implementation for the Budget Management feature following the existing patterns in the codebase:

| Component | Description |
|-----------|-------------|
| **Endpoints** | 8 REST endpoints covering full CRUD + specialized updates |
| **Service Layer** | Separates business logic from HTTP handling |
| **Auth Extractor** | Reusable `AuthenticatedUser` for cleaner handler signatures |
| **Validation** | Using `validator` crate with custom messages |
| **Computed Fields** | `savings_target` and `spending_budget` in responses |
| **Test Coverage** | Unit tests for DTOs + integration tests for API |

The implementation maintains consistency with existing `auth.rs` patterns while introducing a cleaner service-layer architecture that will scale well as more features are added.
