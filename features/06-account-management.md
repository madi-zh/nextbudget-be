# Account Management Feature Plan (Feature 6)

## Overview

This document provides a detailed implementation plan for the Account Management feature in the Rust Actix-web backend. The feature allows users to manage financial accounts (checking, savings, credit) with full CRUD operations, ownership-based authorization, and computed financial summaries.

---

## 1. File Structure

```
src/
├── main.rs                    # Add accounts module registration
├── lib.rs                     # Add accounts module export
├── auth.rs                    # Extract authentication utilities for reuse
├── errors.rs                  # Add NotFound error variant
├── models.rs                  # Existing user models (unchanged)
└── accounts/
    ├── mod.rs                 # Module exports
    ├── models.rs              # Account, AccountType, DTOs
    └── handlers.rs            # HTTP handlers (8 endpoints)

migrations/
└── 20250111000000_create_accounts_table.sql

tests/
├── common/mod.rs              # Add authenticated request helpers
└── account_tests.rs           # Account endpoint tests
```

---

## 2. Database Migration

**File:** `migrations/20250111000000_create_accounts_table.sql`

```sql
-- Create accounts table
CREATE TABLE accounts (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name VARCHAR(50) NOT NULL,
    type VARCHAR(10) NOT NULL CHECK (type IN ('checking', 'savings', 'credit')),
    balance NUMERIC(12,2) NOT NULL DEFAULT 0,
    color_hex CHAR(7) NOT NULL CHECK (color_hex ~ '^#[0-9A-Fa-f]{6}$'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Index for fast lookups by owner
CREATE INDEX idx_accounts_owner_id ON accounts(owner_id);

-- Index for type filtering
CREATE INDEX idx_accounts_owner_type ON accounts(owner_id, type);
```

---

## 3. Model and DTO Definitions

### 3.1 Account Type Enum

**File:** `src/accounts/models.rs`

```rust
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccountType {
    Checking,
    Savings,
    Credit,
}

impl fmt::Display for AccountType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AccountType::Checking => write!(f, "checking"),
            AccountType::Savings => write!(f, "savings"),
            AccountType::Credit => write!(f, "credit"),
        }
    }
}

impl AccountType {
    pub fn as_str(&self) -> &'static str {
        match self {
            AccountType::Checking => "checking",
            AccountType::Savings => "savings",
            AccountType::Credit => "credit",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "checking" => Some(AccountType::Checking),
            "savings" => Some(AccountType::Savings),
            "credit" => Some(AccountType::Credit),
            _ => None,
        }
    }
}
```

### 3.2 Account Domain Model

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::prelude::FromRow;
use sqlx::types::BigDecimal;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Account {
    pub id: Uuid,
    pub owner_id: Uuid,
    pub name: String,
    #[sqlx(rename = "type")]
    #[serde(rename = "type")]
    pub account_type: String,
    pub balance: BigDecimal,
    pub color_hex: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

### 3.3 Request DTOs

```rust
use serde::Deserialize;
use sqlx::types::BigDecimal;
use validator::Validate;

/// Custom validator for color_hex format (#RRGGBB)
fn validate_color_hex(color: &str) -> Result<(), validator::ValidationError> {
    if color.len() != 7 {
        return Err(validator::ValidationError::new("invalid_length"));
    }
    if !color.starts_with('#') {
        return Err(validator::ValidationError::new("missing_hash"));
    }
    if !color[1..].chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(validator::ValidationError::new("invalid_hex_chars"));
    }
    Ok(())
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateAccountDto {
    #[validate(length(min = 1, max = 50, message = "Name must be 1-50 characters"))]
    pub name: String,

    #[serde(rename = "type")]
    pub account_type: AccountType,

    pub balance: Option<BigDecimal>,  // Defaults to 0

    #[validate(custom(function = "validate_color_hex", message = "Color must be #RRGGBB format"))]
    pub color_hex: String,
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpdateAccountDto {
    #[validate(length(min = 1, max = 50, message = "Name must be 1-50 characters"))]
    pub name: Option<String>,

    #[serde(rename = "type")]
    pub account_type: Option<AccountType>,

    #[validate(custom(function = "validate_color_hex_optional"))]
    pub color_hex: Option<String>,
}

fn validate_color_hex_optional(color: &str) -> Result<(), validator::ValidationError> {
    validate_color_hex(color)
}

#[derive(Debug, Deserialize)]
pub struct UpdateBalanceDto {
    pub balance: BigDecimal,
}
```

### 3.4 Response DTOs

```rust
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::types::BigDecimal;
use uuid::Uuid;

#[derive(Debug, Serialize)]
pub struct AccountResponseDto {
    pub id: Uuid,
    pub name: String,
    #[serde(rename = "type")]
    pub account_type: String,
    pub balance: BigDecimal,
    pub color_hex: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl AccountResponseDto {
    pub fn from_account(account: Account) -> Self {
        Self {
            id: account.id,
            name: account.name,
            account_type: account.account_type,
            balance: account.balance,
            color_hex: account.color_hex,
            created_at: account.created_at,
            updated_at: account.updated_at,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct AccountsListResponse {
    pub accounts: Vec<AccountResponseDto>,
    pub count: usize,
}

#[derive(Debug, Serialize)]
pub struct AccountsSummaryResponse {
    pub accounts: Vec<AccountResponseDto>,
    pub summary: AccountsSummary,
}

#[derive(Debug, Serialize)]
pub struct AccountsSummary {
    pub total_savings: BigDecimal,
    pub total_spending: BigDecimal,
    pub net_worth: BigDecimal,
    pub accounts_count: i64,
}

#[derive(Debug, Serialize)]
pub struct DeleteResponse {
    pub message: String,
    pub id: Uuid,
}
```

---

## 4. Error Handling Updates

**File:** `src/errors.rs` - Add these variants:

```rust
#[derive(Debug)]
pub enum AppError {
    ValidationError(String),
    Unauthorized(String),
    NotFound(String),        // NEW
    Forbidden(String),       // NEW (optional, for explicit ownership errors)
    Conflict(String),
    InternalError(String),
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
            AppError::NotFound(msg) => (
                actix_web::http::StatusCode::NOT_FOUND,
                "NOT_FOUND",
                msg.clone(),
            ),
            AppError::Forbidden(msg) => (
                actix_web::http::StatusCode::FORBIDDEN,
                "FORBIDDEN",
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

## 5. Authentication Helper

**File:** `src/auth.rs` - Add this public helper function:

```rust
use actix_web::HttpRequest;
use uuid::Uuid;

/// Extract and validate user ID from Authorization header.
/// Returns the user's UUID from the JWT claims.
pub fn get_user_id_from_request(
    req: &HttpRequest,
    jwt_secret: &str,
) -> Result<Uuid, AppError> {
    let token = extract_token(req)?;
    let claims = decode_token(&token, jwt_secret)?;
    Ok(claims.sub)
}
```

This function reuses the existing `extract_token` and `decode_token` functions, making authentication consistent across all handlers.

---

## 6. Handler Implementations

**File:** `src/accounts/handlers.rs`

### 6.1 GET /accounts - List User's Accounts

```rust
use actix_web::{delete, get, patch, post, web, HttpRequest, HttpResponse};
use sqlx::PgPool;
use uuid::Uuid;

use crate::auth::get_user_id_from_request;
use crate::errors::AppError;
use super::models::*;

#[get("/accounts")]
pub async fn list_accounts(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    jwt_secret: web::Data<String>,
) -> Result<HttpResponse, AppError> {
    let user_id = get_user_id_from_request(&req, jwt_secret.get_ref())?;

    let accounts = sqlx::query_as::<_, Account>(
        r#"
        SELECT id, owner_id, name, type, balance, color_hex, created_at, updated_at
        FROM accounts
        WHERE owner_id = $1
        ORDER BY created_at DESC
        "#
    )
    .bind(user_id)
    .fetch_all(pool.get_ref())
    .await
    .map_err(|e| AppError::InternalError(e.to_string()))?;

    let response = AccountsListResponse {
        count: accounts.len(),
        accounts: accounts.into_iter().map(AccountResponseDto::from_account).collect(),
    };

    Ok(HttpResponse::Ok().json(response))
}
```

### 6.2 GET /accounts/{id} - Get Specific Account

```rust
#[get("/accounts/{id}")]
pub async fn get_account(
    req: HttpRequest,
    path: web::Path<Uuid>,
    pool: web::Data<PgPool>,
    jwt_secret: web::Data<String>,
) -> Result<HttpResponse, AppError> {
    let user_id = get_user_id_from_request(&req, jwt_secret.get_ref())?;
    let account_id = path.into_inner();

    let account = sqlx::query_as::<_, Account>(
        r#"
        SELECT id, owner_id, name, type, balance, color_hex, created_at, updated_at
        FROM accounts
        WHERE id = $1 AND owner_id = $2
        "#
    )
    .bind(account_id)
    .bind(user_id)
    .fetch_optional(pool.get_ref())
    .await
    .map_err(|e| AppError::InternalError(e.to_string()))?
    .ok_or_else(|| AppError::NotFound("Account not found".to_string()))?;

    Ok(HttpResponse::Ok().json(AccountResponseDto::from_account(account)))
}
```

### 6.3 GET /accounts/type/{type} - Get Accounts by Type

```rust
#[get("/accounts/type/{type}")]
pub async fn get_accounts_by_type(
    req: HttpRequest,
    path: web::Path<String>,
    pool: web::Data<PgPool>,
    jwt_secret: web::Data<String>,
) -> Result<HttpResponse, AppError> {
    let user_id = get_user_id_from_request(&req, jwt_secret.get_ref())?;
    let account_type_str = path.into_inner();

    // Validate type
    let valid_types = ["checking", "savings", "credit"];
    if !valid_types.contains(&account_type_str.as_str()) {
        return Err(AppError::ValidationError(
            format!("Invalid account type '{}'. Must be one of: {}",
                    account_type_str, valid_types.join(", "))
        ));
    }

    let accounts = sqlx::query_as::<_, Account>(
        r#"
        SELECT id, owner_id, name, type, balance, color_hex, created_at, updated_at
        FROM accounts
        WHERE owner_id = $1 AND type = $2
        ORDER BY created_at DESC
        "#
    )
    .bind(user_id)
    .bind(&account_type_str)
    .fetch_all(pool.get_ref())
    .await
    .map_err(|e| AppError::InternalError(e.to_string()))?;

    let response = AccountsListResponse {
        count: accounts.len(),
        accounts: accounts.into_iter().map(AccountResponseDto::from_account).collect(),
    };

    Ok(HttpResponse::Ok().json(response))
}
```

### 6.4 GET /accounts/summary - Get Accounts with Summary

```rust
use sqlx::types::BigDecimal;
use std::str::FromStr;

#[derive(Debug, sqlx::FromRow)]
struct SummaryRow {
    total_savings: Option<BigDecimal>,
    total_spending: Option<BigDecimal>,
    accounts_count: Option<i64>,
}

#[get("/accounts/summary")]
pub async fn get_accounts_summary(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    jwt_secret: web::Data<String>,
) -> Result<HttpResponse, AppError> {
    let user_id = get_user_id_from_request(&req, jwt_secret.get_ref())?;

    // Fetch all accounts for this user
    let accounts = sqlx::query_as::<_, Account>(
        r#"
        SELECT id, owner_id, name, type, balance, color_hex, created_at, updated_at
        FROM accounts
        WHERE owner_id = $1
        ORDER BY created_at DESC
        "#
    )
    .bind(user_id)
    .fetch_all(pool.get_ref())
    .await
    .map_err(|e| AppError::InternalError(e.to_string()))?;

    // Compute summary with a single aggregation query
    let summary_row = sqlx::query_as::<_, SummaryRow>(
        r#"
        SELECT
            COALESCE(SUM(CASE WHEN type = 'savings' THEN balance ELSE 0 END), 0) as total_savings,
            COALESCE(SUM(CASE WHEN type IN ('checking', 'credit') THEN balance ELSE 0 END), 0) as total_spending,
            COUNT(*) as accounts_count
        FROM accounts
        WHERE owner_id = $1
        "#
    )
    .bind(user_id)
    .fetch_one(pool.get_ref())
    .await
    .map_err(|e| AppError::InternalError(e.to_string()))?;

    let zero = BigDecimal::from_str("0").unwrap();
    let total_savings = summary_row.total_savings.unwrap_or_else(|| zero.clone());
    let total_spending = summary_row.total_spending.unwrap_or_else(|| zero.clone());
    let net_worth = &total_savings + &total_spending;

    let response = AccountsSummaryResponse {
        accounts: accounts.into_iter().map(AccountResponseDto::from_account).collect(),
        summary: AccountsSummary {
            total_savings,
            total_spending,
            net_worth,
            accounts_count: summary_row.accounts_count.unwrap_or(0),
        },
    };

    Ok(HttpResponse::Ok().json(response))
}
```

### 6.5 POST /accounts - Create Account

```rust
use validator::Validate;

#[post("/accounts")]
pub async fn create_account(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    jwt_secret: web::Data<String>,
    body: web::Json<CreateAccountDto>,
) -> Result<HttpResponse, AppError> {
    let user_id = get_user_id_from_request(&req, jwt_secret.get_ref())?;

    // Validate input
    body.validate()
        .map_err(|e| AppError::ValidationError(e.to_string()))?;

    // Trim and validate name
    let name = body.name.trim().to_string();
    if name.is_empty() {
        return Err(AppError::ValidationError("Name cannot be empty".to_string()));
    }

    let zero = BigDecimal::from_str("0").unwrap();
    let balance = body.balance.clone().unwrap_or(zero);
    let account_type = body.account_type.as_str();

    let account = sqlx::query_as::<_, Account>(
        r#"
        INSERT INTO accounts (owner_id, name, type, balance, color_hex)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id, owner_id, name, type, balance, color_hex, created_at, updated_at
        "#
    )
    .bind(user_id)
    .bind(&name)
    .bind(account_type)
    .bind(&balance)
    .bind(&body.color_hex)
    .fetch_one(pool.get_ref())
    .await
    .map_err(|e| AppError::InternalError(e.to_string()))?;

    Ok(HttpResponse::Created().json(AccountResponseDto::from_account(account)))
}
```

### 6.6 PATCH /accounts/{id} - Update Account

```rust
#[patch("/accounts/{id}")]
pub async fn update_account(
    req: HttpRequest,
    path: web::Path<Uuid>,
    pool: web::Data<PgPool>,
    jwt_secret: web::Data<String>,
    body: web::Json<UpdateAccountDto>,
) -> Result<HttpResponse, AppError> {
    let user_id = get_user_id_from_request(&req, jwt_secret.get_ref())?;
    let account_id = path.into_inner();

    // Validate input if present
    body.validate()
        .map_err(|e| AppError::ValidationError(e.to_string()))?;

    // Trim name if provided
    let name = body.name.as_ref().map(|n| {
        let trimmed = n.trim().to_string();
        if trimmed.is_empty() {
            Err(AppError::ValidationError("Name cannot be empty".to_string()))
        } else {
            Ok(trimmed)
        }
    }).transpose()?;

    let account_type = body.account_type.as_ref().map(|t| t.as_str());

    // Use COALESCE pattern for partial updates
    let account = sqlx::query_as::<_, Account>(
        r#"
        UPDATE accounts SET
            name = COALESCE($3, name),
            type = COALESCE($4, type),
            color_hex = COALESCE($5, color_hex),
            updated_at = NOW()
        WHERE id = $1 AND owner_id = $2
        RETURNING id, owner_id, name, type, balance, color_hex, created_at, updated_at
        "#
    )
    .bind(account_id)
    .bind(user_id)
    .bind(&name)
    .bind(account_type)
    .bind(&body.color_hex)
    .fetch_optional(pool.get_ref())
    .await
    .map_err(|e| AppError::InternalError(e.to_string()))?
    .ok_or_else(|| AppError::NotFound("Account not found".to_string()))?;

    Ok(HttpResponse::Ok().json(AccountResponseDto::from_account(account)))
}
```

### 6.7 PATCH /accounts/{id}/balance - Update Balance Only

```rust
#[patch("/accounts/{id}/balance")]
pub async fn update_account_balance(
    req: HttpRequest,
    path: web::Path<Uuid>,
    pool: web::Data<PgPool>,
    jwt_secret: web::Data<String>,
    body: web::Json<UpdateBalanceDto>,
) -> Result<HttpResponse, AppError> {
    let user_id = get_user_id_from_request(&req, jwt_secret.get_ref())?;
    let account_id = path.into_inner();

    let account = sqlx::query_as::<_, Account>(
        r#"
        UPDATE accounts
        SET balance = $3, updated_at = NOW()
        WHERE id = $1 AND owner_id = $2
        RETURNING id, owner_id, name, type, balance, color_hex, created_at, updated_at
        "#
    )
    .bind(account_id)
    .bind(user_id)
    .bind(&body.balance)
    .fetch_optional(pool.get_ref())
    .await
    .map_err(|e| AppError::InternalError(e.to_string()))?
    .ok_or_else(|| AppError::NotFound("Account not found".to_string()))?;

    Ok(HttpResponse::Ok().json(AccountResponseDto::from_account(account)))
}
```

### 6.8 DELETE /accounts/{id} - Delete Account

```rust
#[delete("/accounts/{id}")]
pub async fn delete_account(
    req: HttpRequest,
    path: web::Path<Uuid>,
    pool: web::Data<PgPool>,
    jwt_secret: web::Data<String>,
) -> Result<HttpResponse, AppError> {
    let user_id = get_user_id_from_request(&req, jwt_secret.get_ref())?;
    let account_id = path.into_inner();

    // Note: When transactions table exists, first set account_id to NULL:
    // sqlx::query("UPDATE transactions SET account_id = NULL WHERE account_id = $1")
    //     .bind(account_id)
    //     .execute(pool.get_ref())
    //     .await
    //     .map_err(|e| AppError::InternalError(e.to_string()))?;

    let result = sqlx::query(
        "DELETE FROM accounts WHERE id = $1 AND owner_id = $2"
    )
    .bind(account_id)
    .bind(user_id)
    .execute(pool.get_ref())
    .await
    .map_err(|e| AppError::InternalError(e.to_string()))?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Account not found".to_string()));
    }

    Ok(HttpResponse::Ok().json(DeleteResponse {
        message: "Account deleted successfully".to_string(),
        id: account_id,
    }))
}
```

---

## 7. Module Registration

### 7.1 src/accounts/mod.rs

```rust
mod handlers;
mod models;

pub use handlers::*;
pub use models::*;
```

### 7.2 src/lib.rs

```rust
pub mod auth;
pub mod errors;
pub mod models;
pub mod accounts;  // Add this line
```

### 7.3 src/main.rs

```rust
mod accounts;  // Add this line
mod auth;
mod errors;
mod models;

// In HttpServer::new closure, add account routes:
HttpServer::new(move || {
    App::new()
        .app_data(web::Data::new(pool.clone()))
        .app_data(web::Data::new(jwt_secret.clone()))
        .service(health_check)
        // Auth routes
        .service(auth::register)
        .service(auth::login)
        .service(auth::me)
        // Account routes - ORDER MATTERS!
        // More specific routes must come before generic {id} route
        .service(accounts::list_accounts)          // GET /accounts
        .service(accounts::get_accounts_summary)   // GET /accounts/summary
        .service(accounts::get_accounts_by_type)   // GET /accounts/type/{type}
        .service(accounts::get_account)            // GET /accounts/{id}
        .service(accounts::create_account)         // POST /accounts
        .service(accounts::update_account)         // PATCH /accounts/{id}
        .service(accounts::update_account_balance) // PATCH /accounts/{id}/balance
        .service(accounts::delete_account)         // DELETE /accounts/{id}
})
```

**Important:** Route registration order matters. `/accounts/summary` and `/accounts/type/{type}` must be registered before `/accounts/{id}` to prevent the `{id}` pattern from matching "summary" or "type" as an ID.

---

## 8. Summary Computation SQL

The summary aggregation query efficiently computes all totals in a single database call:

```sql
SELECT
    COALESCE(SUM(CASE WHEN type = 'savings' THEN balance ELSE 0 END), 0) as total_savings,
    COALESCE(SUM(CASE WHEN type IN ('checking', 'credit') THEN balance ELSE 0 END), 0) as total_spending,
    COUNT(*) as accounts_count
FROM accounts
WHERE owner_id = $1
```

**Explanation:**
- Uses conditional aggregation (`CASE WHEN`) to compute multiple sums in one pass
- `COALESCE(..., 0)` ensures we get 0 instead of NULL when no accounts exist
- Net worth is computed in Rust: `net_worth = total_savings + total_spending`
- This approach avoids N+1 queries and is efficient even with many accounts

**Business Logic:**
```
totalSavings  = sum of balance WHERE type = 'savings'
totalSpending = sum of balance WHERE type IN ('checking', 'credit')
netWorth      = totalSavings + totalSpending
```

---

## 9. Authorization Summary

| Endpoint | Authorization Check |
|----------|-------------------|
| `GET /accounts` | JWT required, filter by `owner_id = user_id` |
| `GET /accounts/{id}` | JWT required, `WHERE id = $1 AND owner_id = $2` |
| `GET /accounts/type/{type}` | JWT required, filter by `owner_id = user_id` |
| `GET /accounts/summary` | JWT required, filter by `owner_id = user_id` |
| `POST /accounts` | JWT required, auto-set `owner_id` from JWT claims |
| `PATCH /accounts/{id}` | JWT required, `WHERE id = $1 AND owner_id = $2` |
| `PATCH /accounts/{id}/balance` | JWT required, `WHERE id = $1 AND owner_id = $2` |
| `DELETE /accounts/{id}` | JWT required, `WHERE id = $1 AND owner_id = $2` |

**Key Authorization Patterns:**

1. **All queries include owner_id filter** - Users can never see or modify other users' data
2. **Create operations auto-set owner_id** - Users cannot create accounts owned by others
3. **Ownership verified atomically** - The WHERE clause combines id and owner_id, preventing race conditions
4. **Returns 404 for unauthorized access** - Does not leak existence of accounts to other users

---

## 10. Cargo.toml Dependencies

Add or update these dependencies:

```toml
[dependencies]
# Existing dependencies...
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
    "bigdecimal"    # ADD THIS for NUMERIC/DECIMAL support
] }
validator = { version = "0.20.0", features = ["derive"] }
uuid = { version = "1.12.1", features = ["v4", "serde"] }
chrono = { version = "0.4.39", features = ["serde"] }

# Add for BigDecimal (money handling)
bigdecimal = { version = "0.4", features = ["serde"] }
```

**Note:** `sqlx` needs the `bigdecimal` feature to map PostgreSQL `NUMERIC(12,2)` columns.

---

## 11. Test Cases

**File:** `tests/account_tests.rs`

### 11.1 Test Helper Extensions

First, extend `tests/common/mod.rs` with authenticated request helpers:

```rust
impl TestApp {
    /// Register a user and return their JWT token
    pub async fn create_authenticated_user(&self, prefix: &str) -> String {
        let email = self.unique_email(prefix);
        let payload = json!({
            "email": email,
            "password": "password123",
            "full_name": format!("{} User", prefix)
        });
        let response = self.post("/auth/register", &payload).await;
        let body: Value = response.json().await;
        body["token"].as_str().unwrap().to_string()
    }

    pub async fn get_authenticated(&self, path: &str, token: &str) -> TestResponse {
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(self.pool.clone()))
                .app_data(web::Data::new(JWT_SECRET.to_string()))
                // Register all routes...
        )
        .await;

        let req = test::TestRequest::get()
            .uri(path)
            .insert_header(("Authorization", format!("Bearer {}", token)))
            .to_request();
        let resp = test::call_service(&app, req).await;

        let status = resp.status().as_u16();
        let body = test::read_body(resp).await;
        TestResponse { status, body }
    }

    pub async fn post_authenticated(&self, path: &str, payload: &Value, token: &str) -> TestResponse {
        // Similar implementation with POST and JSON body
    }

    pub async fn patch_authenticated(&self, path: &str, payload: &Value, token: &str) -> TestResponse {
        // Similar implementation with PATCH and JSON body
    }

    pub async fn delete_authenticated(&self, path: &str, token: &str) -> TestResponse {
        // Similar implementation with DELETE
    }
}
```

### 11.2 Test Cases

```rust
// tests/account_tests.rs

mod common;
use common::TestApp;
use serde_json::{json, Value};

// ==================== CREATE ACCOUNT ====================

#[actix_rt::test]
async fn test_create_account_success() {
    let app = TestApp::new().await;
    let token = app.create_authenticated_user("createacc").await;

    let payload = json!({
        "name": "My Checking",
        "type": "checking",
        "balance": 1000.50,
        "color_hex": "#FF5733"
    });

    let response = app.post_authenticated("/accounts", &payload, &token).await;

    assert_eq!(response.status(), 201);
    let body: Value = response.json().await;
    assert_eq!(body["name"], "My Checking");
    assert_eq!(body["type"], "checking");
    assert!(body["id"].is_string());
}

#[actix_rt::test]
async fn test_create_account_default_balance() {
    let app = TestApp::new().await;
    let token = app.create_authenticated_user("defbal").await;

    let payload = json!({
        "name": "Savings",
        "type": "savings",
        "color_hex": "#00FF00"
    });

    let response = app.post_authenticated("/accounts", &payload, &token).await;

    assert_eq!(response.status(), 201);
    let body: Value = response.json().await;
    // Balance should default to 0
    assert_eq!(body["balance"].as_str().unwrap().parse::<f64>().unwrap(), 0.0);
}

#[actix_rt::test]
async fn test_create_account_negative_balance_allowed() {
    let app = TestApp::new().await;
    let token = app.create_authenticated_user("negbal").await;

    let payload = json!({
        "name": "Credit Card",
        "type": "credit",
        "balance": -500.00,
        "color_hex": "#0000FF"
    });

    let response = app.post_authenticated("/accounts", &payload, &token).await;

    assert_eq!(response.status(), 201);
    let body: Value = response.json().await;
    assert!(body["balance"].as_str().unwrap().parse::<f64>().unwrap() < 0.0);
}

#[actix_rt::test]
async fn test_create_account_invalid_name_too_long() {
    let app = TestApp::new().await;
    let token = app.create_authenticated_user("longname").await;

    let long_name: String = "A".repeat(51);
    let payload = json!({
        "name": long_name,
        "type": "checking",
        "color_hex": "#FF0000"
    });

    let response = app.post_authenticated("/accounts", &payload, &token).await;

    assert_eq!(response.status(), 400);
    let body: Value = response.json().await;
    assert_eq!(body["error"], "VALIDATION_ERROR");
}

#[actix_rt::test]
async fn test_create_account_invalid_color_hex() {
    let app = TestApp::new().await;
    let token = app.create_authenticated_user("badcolor").await;

    let payload = json!({
        "name": "Test",
        "type": "checking",
        "color_hex": "red"  // Invalid format
    });

    let response = app.post_authenticated("/accounts", &payload, &token).await;

    assert_eq!(response.status(), 400);
    let body: Value = response.json().await;
    assert_eq!(body["error"], "VALIDATION_ERROR");
}

#[actix_rt::test]
async fn test_create_account_invalid_type() {
    let app = TestApp::new().await;
    let token = app.create_authenticated_user("badtype").await;

    let payload = json!({
        "name": "Test",
        "type": "investment",  // Invalid type
        "color_hex": "#FF0000"
    });

    let response = app.post_authenticated("/accounts", &payload, &token).await;

    // Serde will fail to deserialize, resulting in 400
    assert_eq!(response.status(), 400);
}

#[actix_rt::test]
async fn test_create_account_unauthenticated() {
    let app = TestApp::new().await;

    let payload = json!({
        "name": "Test",
        "type": "checking",
        "color_hex": "#FF0000"
    });

    let response = app.post("/accounts", &payload).await;

    assert_eq!(response.status(), 401);
}

// ==================== LIST ACCOUNTS ====================

#[actix_rt::test]
async fn test_list_accounts_empty() {
    let app = TestApp::new().await;
    let token = app.create_authenticated_user("listempty").await;

    let response = app.get_authenticated("/accounts", &token).await;

    assert_eq!(response.status(), 200);
    let body: Value = response.json().await;
    assert_eq!(body["count"], 0);
    assert!(body["accounts"].as_array().unwrap().is_empty());
}

#[actix_rt::test]
async fn test_list_accounts_returns_only_own() {
    let app = TestApp::new().await;
    let token1 = app.create_authenticated_user("user1").await;
    let token2 = app.create_authenticated_user("user2").await;

    // User 1 creates an account
    let payload = json!({
        "name": "User1 Account",
        "type": "checking",
        "color_hex": "#FF0000"
    });
    app.post_authenticated("/accounts", &payload, &token1).await;

    // User 2 should not see User 1's account
    let response = app.get_authenticated("/accounts", &token2).await;
    let body: Value = response.json().await;
    assert_eq!(body["count"], 0);

    // User 1 should see their account
    let response = app.get_authenticated("/accounts", &token1).await;
    let body: Value = response.json().await;
    assert_eq!(body["count"], 1);
    assert_eq!(body["accounts"][0]["name"], "User1 Account");
}

// ==================== GET SINGLE ACCOUNT ====================

#[actix_rt::test]
async fn test_get_account_success() {
    let app = TestApp::new().await;
    let token = app.create_authenticated_user("getone").await;

    // Create account
    let payload = json!({
        "name": "My Account",
        "type": "savings",
        "balance": 5000,
        "color_hex": "#00FF00"
    });
    let create_resp = app.post_authenticated("/accounts", &payload, &token).await;
    let created: Value = create_resp.json().await;
    let account_id = created["id"].as_str().unwrap();

    // Fetch it
    let response = app.get_authenticated(&format!("/accounts/{}", account_id), &token).await;

    assert_eq!(response.status(), 200);
    let body: Value = response.json().await;
    assert_eq!(body["name"], "My Account");
}

#[actix_rt::test]
async fn test_get_account_not_found() {
    let app = TestApp::new().await;
    let token = app.create_authenticated_user("notfound").await;

    let fake_id = uuid::Uuid::new_v4();
    let response = app.get_authenticated(&format!("/accounts/{}", fake_id), &token).await;

    assert_eq!(response.status(), 404);
    let body: Value = response.json().await;
    assert_eq!(body["error"], "NOT_FOUND");
}

#[actix_rt::test]
async fn test_get_account_cannot_access_others() {
    let app = TestApp::new().await;
    let token1 = app.create_authenticated_user("owner").await;
    let token2 = app.create_authenticated_user("intruder").await;

    // User 1 creates account
    let payload = json!({
        "name": "Private",
        "type": "checking",
        "color_hex": "#FF0000"
    });
    let resp = app.post_authenticated("/accounts", &payload, &token1).await;
    let created: Value = resp.json().await;
    let account_id = created["id"].as_str().unwrap();

    // User 2 tries to access it - should get 404 (not 403)
    let response = app.get_authenticated(&format!("/accounts/{}", account_id), &token2).await;

    assert_eq!(response.status(), 404);
}

// ==================== GET BY TYPE ====================

#[actix_rt::test]
async fn test_get_accounts_by_type() {
    let app = TestApp::new().await;
    let token = app.create_authenticated_user("bytype").await;

    // Create multiple accounts of different types
    for (name, acc_type) in [("Checking1", "checking"), ("Savings1", "savings"), ("Checking2", "checking")] {
        let payload = json!({
            "name": name,
            "type": acc_type,
            "color_hex": "#FF0000"
        });
        app.post_authenticated("/accounts", &payload, &token).await;
    }

    // Get only checking accounts
    let response = app.get_authenticated("/accounts/type/checking", &token).await;

    assert_eq!(response.status(), 200);
    let body: Value = response.json().await;
    assert_eq!(body["count"], 2);
}

#[actix_rt::test]
async fn test_get_accounts_invalid_type() {
    let app = TestApp::new().await;
    let token = app.create_authenticated_user("invalidtype").await;

    let response = app.get_authenticated("/accounts/type/investment", &token).await;

    assert_eq!(response.status(), 400);
}

// ==================== SUMMARY ====================

#[actix_rt::test]
async fn test_accounts_summary() {
    let app = TestApp::new().await;
    let token = app.create_authenticated_user("summary").await;

    // Create accounts: savings=10000, checking=5000, credit=-2000
    let accounts = [
        ("Savings", "savings", 10000.00),
        ("Checking", "checking", 5000.00),
        ("Credit Card", "credit", -2000.00),
    ];

    for (name, acc_type, balance) in accounts {
        let payload = json!({
            "name": name,
            "type": acc_type,
            "balance": balance,
            "color_hex": "#FF0000"
        });
        app.post_authenticated("/accounts", &payload, &token).await;
    }

    let response = app.get_authenticated("/accounts/summary", &token).await;

    assert_eq!(response.status(), 200);
    let body: Value = response.json().await;

    // total_savings = 10000
    // total_spending = 5000 + (-2000) = 3000
    // net_worth = 10000 + 3000 = 13000
    assert_eq!(body["summary"]["accounts_count"], 3);
    assert_eq!(body["accounts"].as_array().unwrap().len(), 3);

    // Note: BigDecimal serializes as string
    let total_savings: f64 = body["summary"]["total_savings"].as_str()
        .unwrap_or_else(|| body["summary"]["total_savings"].to_string().as_str())
        .parse().unwrap();
    assert!((total_savings - 10000.0).abs() < 0.01);
}

#[actix_rt::test]
async fn test_accounts_summary_empty() {
    let app = TestApp::new().await;
    let token = app.create_authenticated_user("emptysum").await;

    let response = app.get_authenticated("/accounts/summary", &token).await;

    assert_eq!(response.status(), 200);
    let body: Value = response.json().await;
    assert_eq!(body["summary"]["accounts_count"], 0);
}

// ==================== UPDATE ACCOUNT ====================

#[actix_rt::test]
async fn test_update_account_name() {
    let app = TestApp::new().await;
    let token = app.create_authenticated_user("update").await;

    // Create
    let payload = json!({
        "name": "Original",
        "type": "checking",
        "color_hex": "#FF0000"
    });
    let resp = app.post_authenticated("/accounts", &payload, &token).await;
    let created: Value = resp.json().await;
    let account_id = created["id"].as_str().unwrap();

    // Update name only
    let update_payload = json!({ "name": "Updated Name" });
    let response = app.patch_authenticated(
        &format!("/accounts/{}", account_id),
        &update_payload,
        &token
    ).await;

    assert_eq!(response.status(), 200);
    let body: Value = response.json().await;
    assert_eq!(body["name"], "Updated Name");
    assert_eq!(body["type"], "checking");  // Unchanged
}

#[actix_rt::test]
async fn test_update_account_cannot_modify_others() {
    let app = TestApp::new().await;
    let token1 = app.create_authenticated_user("owner2").await;
    let token2 = app.create_authenticated_user("attacker").await;

    // User 1 creates account
    let payload = json!({
        "name": "Original",
        "type": "checking",
        "color_hex": "#FF0000"
    });
    let resp = app.post_authenticated("/accounts", &payload, &token1).await;
    let created: Value = resp.json().await;
    let account_id = created["id"].as_str().unwrap();

    // User 2 tries to update - should fail with 404
    let update_payload = json!({ "name": "Hacked" });
    let response = app.patch_authenticated(
        &format!("/accounts/{}", account_id),
        &update_payload,
        &token2
    ).await;

    assert_eq!(response.status(), 404);
}

// ==================== UPDATE BALANCE ====================

#[actix_rt::test]
async fn test_update_balance() {
    let app = TestApp::new().await;
    let token = app.create_authenticated_user("balance").await;

    // Create
    let payload = json!({
        "name": "Account",
        "type": "checking",
        "balance": 100,
        "color_hex": "#FF0000"
    });
    let resp = app.post_authenticated("/accounts", &payload, &token).await;
    let created: Value = resp.json().await;
    let account_id = created["id"].as_str().unwrap();

    // Update balance
    let update_payload = json!({ "balance": 999.99 });
    let response = app.patch_authenticated(
        &format!("/accounts/{}/balance", account_id),
        &update_payload,
        &token
    ).await;

    assert_eq!(response.status(), 200);
    let body: Value = response.json().await;
    let balance: f64 = body["balance"].as_str()
        .unwrap_or_else(|| body["balance"].to_string().as_str())
        .parse().unwrap();
    assert!((balance - 999.99).abs() < 0.01);
}

// ==================== DELETE ACCOUNT ====================

#[actix_rt::test]
async fn test_delete_account_success() {
    let app = TestApp::new().await;
    let token = app.create_authenticated_user("delete").await;

    // Create
    let payload = json!({
        "name": "ToDelete",
        "type": "checking",
        "color_hex": "#FF0000"
    });
    let resp = app.post_authenticated("/accounts", &payload, &token).await;
    let created: Value = resp.json().await;
    let account_id = created["id"].as_str().unwrap();

    // Delete
    let response = app.delete_authenticated(&format!("/accounts/{}", account_id), &token).await;

    assert_eq!(response.status(), 200);
    let body: Value = response.json().await;
    assert_eq!(body["message"], "Account deleted successfully");

    // Verify it's gone
    let get_resp = app.get_authenticated(&format!("/accounts/{}", account_id), &token).await;
    assert_eq!(get_resp.status(), 404);
}

#[actix_rt::test]
async fn test_delete_account_not_found() {
    let app = TestApp::new().await;
    let token = app.create_authenticated_user("delnf").await;

    let fake_id = uuid::Uuid::new_v4();
    let response = app.delete_authenticated(&format!("/accounts/{}", fake_id), &token).await;

    assert_eq!(response.status(), 404);
}

#[actix_rt::test]
async fn test_delete_account_cannot_delete_others() {
    let app = TestApp::new().await;
    let token1 = app.create_authenticated_user("delowner").await;
    let token2 = app.create_authenticated_user("delattacker").await;

    // User 1 creates account
    let payload = json!({
        "name": "Private",
        "type": "checking",
        "color_hex": "#FF0000"
    });
    let resp = app.post_authenticated("/accounts", &payload, &token1).await;
    let created: Value = resp.json().await;
    let account_id = created["id"].as_str().unwrap();

    // User 2 tries to delete - should fail
    let response = app.delete_authenticated(&format!("/accounts/{}", account_id), &token2).await;
    assert_eq!(response.status(), 404);

    // Verify it still exists for User 1
    let get_resp = app.get_authenticated(&format!("/accounts/{}", account_id), &token1).await;
    assert_eq!(get_resp.status(), 200);
}
```

---

## 12. Implementation Checklist

1. [ ] **Database Migration**
   - Create `migrations/20250111000000_create_accounts_table.sql`
   - Run `sqlx migrate run`

2. [ ] **Dependencies**
   - Add `bigdecimal` feature to sqlx in `Cargo.toml`
   - Add `bigdecimal` crate

3. [ ] **Error Types**
   - Add `NotFound` variant to `AppError` in `src/errors.rs`
   - Add `Forbidden` variant (optional)

4. [ ] **Auth Helper**
   - Add `get_user_id_from_request` function to `src/auth.rs`

5. [ ] **Accounts Module**
   - Create `src/accounts/` directory
   - Create `src/accounts/mod.rs`
   - Create `src/accounts/models.rs`
   - Create `src/accounts/handlers.rs`

6. [ ] **Module Registration**
   - Add `pub mod accounts;` to `src/lib.rs`
   - Add `mod accounts;` to `src/main.rs`
   - Register all account routes in correct order

7. [ ] **Test Helpers**
   - Add authenticated request methods to `tests/common/mod.rs`
   - Register account routes in test app setup

8. [ ] **Tests**
   - Create `tests/account_tests.rs`
   - Run `cargo test` to verify all tests pass

---

## 13. API Response Examples

### List Accounts Response

```json
{
  "accounts": [
    {
      "id": "550e8400-e29b-41d4-a716-446655440000",
      "name": "Main Checking",
      "type": "checking",
      "balance": "5234.50",
      "color_hex": "#4CAF50",
      "created_at": "2025-01-11T10:30:00Z",
      "updated_at": "2025-01-11T10:30:00Z"
    }
  ],
  "count": 1
}
```

### Summary Response

```json
{
  "accounts": [
    { "id": "...", "name": "Savings", "type": "savings", "balance": "10000.00", ... },
    { "id": "...", "name": "Checking", "type": "checking", "balance": "5000.00", ... },
    { "id": "...", "name": "Credit Card", "type": "credit", "balance": "-2000.00", ... }
  ],
  "summary": {
    "total_savings": "10000.00",
    "total_spending": "3000.00",
    "net_worth": "13000.00",
    "accounts_count": 3
  }
}
```

### Error Response

```json
{
  "error": "VALIDATION_ERROR",
  "message": "Name must be 1-50 characters"
}
```

### Delete Response

```json
{
  "message": "Account deleted successfully",
  "id": "550e8400-e29b-41d4-a716-446655440000"
}
```

---

## 14. Security Considerations

1. **SQL Injection Prevention**: All queries use parameterized bindings (`$1`, `$2`, etc.)
2. **Authorization**: Every query includes `owner_id` filter - no data leakage possible
3. **Input Validation**: Validator crate ensures proper input before DB operations
4. **Error Messages**: `NotFound` returned instead of `Forbidden` to prevent account enumeration
5. **Decimal Handling**: Using `BigDecimal` for precise monetary calculations (no floating-point errors)

---

## 15. Future Improvements

1. **Pagination**: Add `offset` and `limit` query params to list endpoints
2. **Sorting**: Add `sort_by` and `order` query params
3. **Account Limits**: Consider max accounts per user
4. **Soft Delete**: Add `deleted_at` column instead of hard delete
5. **Audit Log**: Track balance changes with timestamp and reason
6. **Custom Extractor**: Create `AuthenticatedUser` extractor for cleaner handlers
7. **Transactions Integration**: When transactions table is added, update delete handler to set `account_id = NULL` on related transactions
