# Multicurrency Implementation Plan

## Executive Summary

This plan outlines the implementation of multicurrency support for the BudgetFlow Rust backend. The design prioritizes data integrity, performance, and backwards compatibility while enabling users to track finances across multiple currencies.

---

## Design Decisions

Based on requirements gathering:

| Decision | Choice |
|----------|--------|
| **Exchange Rate Source** | External API integration (e.g., Open Exchange Rates) |
| **Default Currency** | USD for all existing data |
| **Budget Model** | Single budget per month (transactions convert to budget currency) |

---

## Current State

- **Database**: PostgreSQL with SQLx
- **Monetary precision**: `NUMERIC(12,2)` in DB, `rust_decimal::Decimal` in Rust
- **Tables with monetary values**:
  - `budgets`: total_income, savings_rate
  - `accounts`: balance
  - `categories`: allocated_amount
  - `transactions`: amount
- **No currency field anywhere** - single currency assumption throughout

---

## 1. Database Schema Design

### 1.1 New Tables

#### `currencies` Table

```sql
-- Migration: 20260122000001_create_currencies.sql
CREATE TABLE IF NOT EXISTS currencies (
    code CHAR(3) PRIMARY KEY,              -- ISO 4217 code (USD, EUR, GBP, etc.)
    name VARCHAR(100) NOT NULL,            -- "United States Dollar"
    symbol VARCHAR(10) NOT NULL,           -- "$", "€", "£"
    decimal_places SMALLINT NOT NULL DEFAULT 2,
    is_active BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Seed common currencies
INSERT INTO currencies (code, name, symbol, decimal_places) VALUES
    ('USD', 'United States Dollar', '$', 2),
    ('EUR', 'Euro', '€', 2),
    ('GBP', 'British Pound Sterling', '£', 2),
    ('JPY', 'Japanese Yen', '¥', 0),
    ('KZT', 'Kazakhstani Tenge', '₸', 2),
    ('RUB', 'Russian Ruble', '₽', 2),
    ('CNY', 'Chinese Yuan', '¥', 2),
    ('CAD', 'Canadian Dollar', 'CA$', 2),
    ('AUD', 'Australian Dollar', 'A$', 2),
    ('CHF', 'Swiss Franc', 'CHF', 2);

CREATE INDEX idx_currencies_active ON currencies(is_active) WHERE is_active = true;
```

#### `exchange_rates` Table

```sql
-- Migration: 20260122000002_create_exchange_rates.sql
CREATE TABLE IF NOT EXISTS exchange_rates (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    base_currency CHAR(3) NOT NULL REFERENCES currencies(code),
    target_currency CHAR(3) NOT NULL REFERENCES currencies(code),
    rate NUMERIC(18,8) NOT NULL,           -- High precision for rates
    effective_date DATE NOT NULL,          -- Date the rate is valid for
    source VARCHAR(50) NOT NULL DEFAULT 'manual',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    CONSTRAINT chk_exchange_rates_positive CHECK (rate > 0),
    CONSTRAINT chk_exchange_rates_different CHECK (base_currency != target_currency),
    CONSTRAINT uq_exchange_rates_pair_date UNIQUE (base_currency, target_currency, effective_date)
);

CREATE INDEX idx_exchange_rates_lookup
    ON exchange_rates(base_currency, target_currency, effective_date DESC);
CREATE INDEX idx_exchange_rates_date ON exchange_rates(effective_date DESC);
```

### 1.2 Modifications to Existing Tables

#### `users` - Add Default Currency

```sql
-- Migration: 20260122000003_add_user_default_currency.sql
ALTER TABLE users
    ADD COLUMN default_currency CHAR(3) NOT NULL DEFAULT 'USD'
    REFERENCES currencies(code);

CREATE INDEX idx_users_currency ON users(default_currency);
```

#### `budgets` - Add Currency

```sql
-- Migration: 20260122000004_add_budget_currency.sql
-- Single budget per month - all transactions convert to this budget's currency
ALTER TABLE budgets
    ADD COLUMN currency CHAR(3) NOT NULL DEFAULT 'USD'
    REFERENCES currencies(code);

-- Keep existing constraint: one budget per owner/month/year
-- (currency is the reporting currency, not a discriminator)

CREATE INDEX idx_budgets_currency ON budgets(currency);
```

#### `accounts` - Add Currency

```sql
-- Migration: 20260122000005_add_account_currency.sql
ALTER TABLE accounts
    ADD COLUMN currency CHAR(3) NOT NULL DEFAULT 'USD'
    REFERENCES currencies(code);

CREATE INDEX idx_accounts_currency ON accounts(currency);
```

#### `transactions` - Add Currency Fields

```sql
-- Migration: 20260122000006_add_transaction_currency.sql
ALTER TABLE transactions
    ADD COLUMN currency CHAR(3) NOT NULL DEFAULT 'USD'
    REFERENCES currencies(code);

-- Store converted amount in budget's reporting currency
ALTER TABLE transactions
    ADD COLUMN converted_amount NUMERIC(12,2);

-- Store exchange rate used at transaction time (for audit)
ALTER TABLE transactions
    ADD COLUMN exchange_rate NUMERIC(18,8);

-- Reference to which currency the converted_amount is in
ALTER TABLE transactions
    ADD COLUMN converted_currency CHAR(3)
    REFERENCES currencies(code);

CREATE INDEX idx_transactions_currency ON transactions(currency);
CREATE INDEX idx_transactions_converted ON transactions(converted_currency)
    WHERE converted_currency IS NOT NULL;
```

---

## 2. Rust Models and DTOs

### 2.1 New Currency Module (`src/currency/`)

#### `src/currency/models.rs`

```rust
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::{IntoParams, ToSchema};
use validator::Validate;
use uuid::Uuid;

/// Database entity for currencies
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Currency {
    pub code: String,
    pub name: String,
    pub symbol: String,
    pub decimal_places: i16,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}

/// Currency response for API
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CurrencyResponse {
    pub code: String,
    pub name: String,
    pub symbol: String,
    pub decimal_places: i16,
}

/// Database entity for exchange rates
#[derive(Debug, Clone, FromRow)]
pub struct ExchangeRate {
    pub id: Uuid,
    pub base_currency: String,
    pub target_currency: String,
    pub rate: Decimal,
    pub effective_date: NaiveDate,
    pub source: String,
    pub created_at: DateTime<Utc>,
}

/// Exchange rate response for API
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExchangeRateResponse {
    pub id: Uuid,
    pub base_currency: String,
    pub target_currency: String,
    pub rate: Decimal,
    pub effective_date: NaiveDate,
    pub source: String,
}

/// Request to create/update exchange rate
#[derive(Debug, Deserialize, Validate, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpsertExchangeRateDto {
    #[validate(length(equal = 3))]
    pub base_currency: String,

    #[validate(length(equal = 3))]
    pub target_currency: String,

    pub rate: Decimal,
    pub effective_date: NaiveDate,

    #[serde(default = "default_source")]
    pub source: String,
}

fn default_source() -> String {
    "manual".to_string()
}

/// Query params for exchange rate lookup
#[derive(Debug, Deserialize, IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct ExchangeRateQuery {
    pub base: String,
    pub target: String,
    pub date: Option<NaiveDate>,
}

/// Conversion request
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConvertAmountDto {
    pub amount: Decimal,
    pub from_currency: String,
    pub to_currency: String,
    pub date: Option<NaiveDate>,
}

/// Conversion response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConversionResponse {
    pub original_amount: Decimal,
    pub converted_amount: Decimal,
    pub from_currency: String,
    pub to_currency: String,
    pub exchange_rate: Decimal,
    pub effective_date: NaiveDate,
}
```

### 2.2 Modified Models Summary

| File | Field Additions |
|------|-----------------|
| `src/auth/models.rs` | `User.default_currency`, `UserResponseDto.default_currency`, `CreateUserDto.default_currency` |
| `src/budget/models.rs` | `Budget.currency`, `BudgetResponse.currency`, `CreateBudgetDto.currency` |
| `src/account/models.rs` | `Account.currency`, `AccountResponse.currency`, `CreateAccountDto.currency` (required), `AccountsSummary.reporting_currency` |
| `src/transaction/models.rs` | `Transaction.{currency, converted_amount, exchange_rate, converted_currency}`, corresponding response/DTO fields |

---

## 3. Currency Conversion Strategy

### 3.1 Key Architectural Decisions

| Decision | Rationale |
|----------|-----------|
| **Store original + converted amounts** | Preserves original data while enabling reporting |
| **Convert at write time** | Ensures consistent historical data |
| **Store exchange rate used** | Enables audit trail and recalculation |
| **Budget determines reporting currency** | Each budget period can have its own currency |
| **Account currency is immutable** | Simplifies balance tracking |

### 3.2 Conversion Service (`src/currency/service.rs`)

```rust
pub struct CurrencyService;

impl CurrencyService {
    /// Get exchange rate for a specific date (falls back to most recent)
    pub async fn get_rate(
        pool: &PgPool,
        base: &str,
        target: &str,
        date: NaiveDate,
    ) -> Result<ExchangeRate, AppError> {
        sqlx::query_as::<_, ExchangeRate>(
            r#"
            SELECT id, base_currency, target_currency, rate,
                   effective_date, source, created_at
            FROM exchange_rates
            WHERE base_currency = $1
              AND target_currency = $2
              AND effective_date <= $3
            ORDER BY effective_date DESC
            LIMIT 1
            "#,
        )
        .bind(base)
        .bind(target)
        .bind(date)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound(format!(
            "No exchange rate found for {} -> {}",
            base, target
        )))
    }

    /// Convert amount between currencies
    pub async fn convert(
        pool: &PgPool,
        amount: Decimal,
        from: &str,
        to: &str,
        date: NaiveDate,
    ) -> Result<(Decimal, Decimal), AppError> {
        if from == to {
            return Ok((amount, Decimal::ONE));
        }

        let rate = Self::get_rate(pool, from, to, date).await?;
        let converted = (amount * rate.rate).round_dp(2);

        Ok((converted, rate.rate))
    }
}
```

### 3.3 Transaction Creation Flow

1. Determine transaction currency (from DTO → account → budget)
2. Get budget's reporting currency via category lookup
3. If currencies differ, fetch exchange rate and compute converted amount
4. Store original amount, converted amount, and rate used

---

## 4. API Endpoint Changes

### 4.1 New Currency Endpoints

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/currencies` | List all active currencies |
| `GET` | `/currencies/{code}` | Get currency details |
| `GET` | `/exchange-rates` | Get rate for currency pair |
| `POST` | `/exchange-rates` | Create/update exchange rate |
| `GET` | `/exchange-rates/latest` | Get latest rates for base |
| `POST` | `/convert` | Convert amount between currencies |

### 4.2 Modified Endpoints

| Endpoint | Changes |
|----------|---------|
| `POST /budgets` | Accept optional `currency` (defaults to user's default) |
| `GET /budgets/*` | Response includes `currency` |
| `POST /accounts` | **Require** `currency` field |
| `GET /accounts/*` | Response includes `currency` |
| `GET /accounts/summary` | Add `reportingCurrency` query param |
| `POST /transactions` | Accept optional `currency` |
| `GET /transactions/*` | Response includes currency + conversion fields |
| `POST /auth/register` | Accept optional `defaultCurrency` |
| `GET /auth/me` | Include `defaultCurrency` |

---

## 5. Files to Create

| File | Purpose |
|------|---------|
| `src/currency/mod.rs` | Module exports |
| `src/currency/models.rs` | Currency and ExchangeRate structs/DTOs |
| `src/currency/service.rs` | Currency conversion logic |
| `src/currency/handlers.rs` | HTTP handlers |
| `migrations/20260122000001_create_currencies.sql` | Currencies table |
| `migrations/20260122000002_create_exchange_rates.sql` | Exchange rates table |
| `migrations/20260122000003_add_user_default_currency.sql` | User currency field |
| `migrations/20260122000004_add_budget_currency.sql` | Budget currency field |
| `migrations/20260122000005_add_account_currency.sql` | Account currency field |
| `migrations/20260122000006_add_transaction_currency.sql` | Transaction currency fields |

## 6. Files to Modify

| File | Changes |
|------|---------|
| `src/main.rs` | Register currency module and routes |
| `src/lib.rs` | Export currency module |
| `src/openapi.rs` | Add currency schemas and endpoints |
| `src/auth/models.rs` | Add `default_currency` to User, DTOs |
| `src/auth/service.rs` | Handle default_currency in registration |
| `src/auth/handlers.rs` | Update register/me handlers |
| `src/budget/models.rs` | Add `currency` field |
| `src/budget/service.rs` | Handle currency in create/queries |
| `src/budget/handlers.rs` | Pass currency through |
| `src/account/models.rs` | Add `currency` field |
| `src/account/service.rs` | Handle currency, update summary logic |
| `src/account/handlers.rs` | Accept currency, add summary params |
| `src/category/service.rs` | Use converted_amount in aggregations |
| `src/transaction/models.rs` | Add currency and conversion fields |
| `src/transaction/service.rs` | Currency conversion on create/update |
| `src/transaction/handlers.rs` | Pass currency through |

---

## 7. Implementation Phases

### Phase 1: Foundation
- Create currencies table and seed data
- Create exchange_rates table
- Implement currency module (models, service, handlers)
- Add currency endpoints to OpenAPI

### Phase 2: User & Budget Currency
- Add default_currency to users
- Add currency to budgets
- Update auth and budget services/handlers

### Phase 3: Account Currency
- Add currency to accounts
- Update account service for multicurrency summary
- Handle currency in account creation

### Phase 4: Transaction Currency
- Add currency fields to transactions
- Implement conversion logic in transaction creation
- Update category spent calculations to use converted amounts
- Handle currency changes in update/delete

### Phase 5: Testing & Polish
- Integration tests for currency conversion
- Edge case handling (missing rates, same currency)
- API documentation updates
- Performance testing for aggregation queries

---

## 8. Migration Strategy

### Backwards Compatibility

- All currency columns default to `'USD'`
- Existing API endpoints continue to work (currency optional in requests)
- Response DTOs add currency fields (non-breaking)
- `converted_amount` is nullable (only for cross-currency)

### Existing Data

```sql
-- Existing data will have:
-- currency = 'USD' (default)
-- converted_amount = NULL
-- exchange_rate = NULL
-- converted_currency = NULL
```

No data transformation needed - existing records are assumed single-currency (USD).

---

## 9. Verification Plan

1. **Unit Tests**: Currency conversion service with various rate scenarios
2. **Integration Tests**:
   - Create transaction in different currency than budget
   - Account summary aggregation across currencies
   - Category spent calculation with mixed currencies
3. **Manual Testing**:
   - Create accounts in different currencies
   - Create transactions and verify conversion
   - Check budget reports show correct totals
4. **Edge Cases**:
   - Missing exchange rate
   - Same currency (no conversion)
   - Zero amount transactions
   - Rate precision handling
