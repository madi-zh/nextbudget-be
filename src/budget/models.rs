use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use validator::{Validate, ValidationError};

/// Validate that a Decimal is non-negative
fn validate_non_negative(value: &Decimal) -> Result<(), ValidationError> {
    if *value < Decimal::ZERO {
        return Err(ValidationError::new("must be non-negative"));
    }
    Ok(())
}

/// Validate that a Decimal is between 0 and 100 (inclusive)
fn validate_percentage(value: &Decimal) -> Result<(), ValidationError> {
    let hundred = Decimal::from(100);
    if *value < Decimal::ZERO || *value > hundred {
        return Err(ValidationError::new("must be between 0 and 100"));
    }
    Ok(())
}

/// Database entity for budgets
#[derive(Debug, Clone, FromRow)]
pub struct Budget {
    pub id: Uuid,
    #[allow(dead_code)] // Used in SQL queries for ownership check
    pub owner_id: Uuid,
    pub month: i16,
    pub year: i16,
    pub total_income: Decimal,
    pub savings_rate: Decimal,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Response DTO with computed fields.
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

    #[serde(default)]
    pub total_income: Option<Decimal>,

    #[serde(default)]
    pub savings_rate: Option<Decimal>,
}

impl CreateBudgetDto {
    /// Validate decimal fields that can't use derive macro
    pub fn validate_decimals(&self) -> Result<(), ValidationError> {
        if let Some(income) = &self.total_income {
            validate_non_negative(income)?;
        }
        if let Some(rate) = &self.savings_rate {
            validate_percentage(rate)?;
        }
        Ok(())
    }
}

/// DTO for updating a budget (all fields optional for PATCH semantics)
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct UpdateBudgetDto {
    #[validate(range(min = 0, max = 11, message = "Month must be between 0 and 11"))]
    pub month: Option<i16>,

    #[validate(range(min = 2000, max = 2100, message = "Year must be between 2000 and 2100"))]
    pub year: Option<i16>,

    pub total_income: Option<Decimal>,

    pub savings_rate: Option<Decimal>,
}

impl UpdateBudgetDto {
    /// Validate decimal fields that can't use derive macro
    pub fn validate_decimals(&self) -> Result<(), ValidationError> {
        if let Some(income) = &self.total_income {
            validate_non_negative(income)?;
        }
        if let Some(rate) = &self.savings_rate {
            validate_percentage(rate)?;
        }
        Ok(())
    }
}

/// DTO for updating income only
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct UpdateIncomeDto {
    #[validate(custom(function = "validate_non_negative"))]
    pub total_income: Decimal,
}

/// DTO for updating savings rate only
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSavingsRateDto {
    #[validate(custom(function = "validate_percentage"))]
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
