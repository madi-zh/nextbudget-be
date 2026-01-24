use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::{IntoParams, ToSchema};
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
    pub currency: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Budget response with computed fields
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BudgetResponse {
    /// Unique budget identifier
    pub id: Uuid,
    /// Month (0-11, where 0 = January)
    #[schema(example = 0, minimum = 0, maximum = 11)]
    pub month: i16,
    /// Year
    #[schema(example = 2024)]
    pub year: i16,
    /// Total monthly income
    #[schema(example = 5000.00)]
    pub total_income: Decimal,
    /// Savings rate percentage (0-100)
    #[schema(example = 20.0)]
    pub savings_rate: Decimal,
    /// Computed: income * savings_rate / 100
    #[schema(example = 1000.00)]
    pub savings_target: Decimal,
    /// Computed: income - savings_target
    #[schema(example = 4000.00)]
    pub spending_budget: Decimal,
    /// ISO 4217 currency code
    #[schema(example = "USD")]
    pub currency: String,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Last update timestamp
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
            currency: budget.currency,
            created_at: budget.created_at,
            updated_at: budget.updated_at,
        }
    }
}

/// Request body for creating a new budget
#[derive(Debug, Deserialize, Validate, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateBudgetDto {
    /// Month (0-11, where 0 = January)
    #[validate(range(min = 0, max = 11, message = "Month must be between 0 and 11"))]
    #[schema(example = 0, minimum = 0, maximum = 11)]
    pub month: i16,

    /// Year
    #[validate(range(min = 2000, max = 2100, message = "Year must be between 2000 and 2100"))]
    #[schema(example = 2024)]
    pub year: i16,

    /// Total monthly income (optional, defaults to 0)
    #[serde(default)]
    #[schema(example = 5000.00)]
    pub total_income: Option<Decimal>,

    /// Savings rate percentage 0-100 (optional, defaults to 0)
    #[serde(default)]
    #[schema(example = 20.0)]
    pub savings_rate: Option<Decimal>,

    /// Currency code (optional, defaults to user's default_currency)
    #[schema(example = "USD")]
    pub currency: Option<String>,
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

/// Request body for updating a budget (PATCH - all fields optional)
#[derive(Debug, Deserialize, Validate, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateBudgetDto {
    /// Month (0-11)
    #[validate(range(min = 0, max = 11, message = "Month must be between 0 and 11"))]
    #[schema(example = 0)]
    pub month: Option<i16>,

    /// Year
    #[validate(range(min = 2000, max = 2100, message = "Year must be between 2000 and 2100"))]
    #[schema(example = 2024)]
    pub year: Option<i16>,

    /// Total monthly income
    #[schema(example = 5000.00)]
    pub total_income: Option<Decimal>,

    /// Savings rate percentage (0-100)
    #[schema(example = 20.0)]
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

/// Request body for updating income only
#[derive(Debug, Deserialize, Validate, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateIncomeDto {
    /// Total monthly income (must be non-negative)
    #[validate(custom(function = "validate_non_negative"))]
    #[schema(example = 5000.00)]
    pub total_income: Decimal,
}

/// Request body for updating savings rate only
#[derive(Debug, Deserialize, Validate, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSavingsRateDto {
    /// Savings rate percentage (0-100)
    #[validate(custom(function = "validate_percentage"))]
    #[schema(example = 20.0)]
    pub savings_rate: Decimal,
}

/// Path parameters for budget ID
#[derive(Debug, Deserialize, IntoParams)]
pub struct BudgetIdPath {
    /// Budget UUID
    pub id: Uuid,
}

/// Path parameters for month/year lookup
#[derive(Debug, Deserialize, Validate, IntoParams)]
pub struct MonthYearPath {
    /// Month (0-11)
    #[validate(range(min = 0, max = 11, message = "Month must be between 0 and 11"))]
    #[param(example = 0)]
    pub month: i16,

    /// Year
    #[validate(range(min = 2000, max = 2100, message = "Year must be between 2000 and 2100"))]
    #[param(example = 2024)]
    pub year: i16,
}

/// Query parameters for listing budgets
#[derive(Debug, Deserialize, Validate, IntoParams)]
pub struct ListBudgetsQuery {
    /// Filter by year
    #[validate(range(min = 2000, max = 2100))]
    #[param(example = 2024)]
    pub year: Option<i16>,

    /// Maximum number of results (1-100)
    #[validate(range(min = 1, max = 100))]
    #[serde(default = "default_limit")]
    #[param(example = 20)]
    pub limit: i64,

    /// Number of results to skip
    #[validate(range(min = 0))]
    #[serde(default)]
    #[param(example = 0)]
    pub offset: i64,
}

fn default_limit() -> i64 {
    20
}
