use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use validator::{Validate, ValidationError};

/// Validate hex color format (#RRGGBB)
fn validate_color_hex(color: &str) -> Result<(), ValidationError> {
    if color.len() != 7 {
        return Err(ValidationError::new("invalid_length"));
    }
    if !color.starts_with('#') {
        return Err(ValidationError::new("missing_hash"));
    }
    if !color[1..].chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(ValidationError::new("invalid_hex_chars"));
    }
    Ok(())
}

/// Validate that a Decimal is non-negative
fn validate_non_negative(value: &Decimal) -> Result<(), ValidationError> {
    if *value < Decimal::ZERO {
        return Err(ValidationError::new("must be non-negative"));
    }
    Ok(())
}

/// Database entity for categories
#[derive(Debug, Clone, FromRow)]
pub struct Category {
    pub id: Uuid,
    pub budget_id: Uuid,
    pub name: String,
    pub allocated_amount: Decimal,
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
    pub allocated_amount: Decimal,
    pub color_hex: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub spent_amount: Decimal,
}

/// Response DTO for category
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryResponse {
    pub id: Uuid,
    pub budget_id: Uuid,
    pub name: String,
    pub allocated_amount: Decimal,
    pub spent_amount: Decimal,
    pub remaining_amount: Decimal,
    pub color_hex: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl CategoryResponse {
    pub fn from_category_with_spent(cat: CategoryWithSpent) -> Self {
        let remaining_amount = cat.allocated_amount - cat.spent_amount;

        Self {
            id: cat.id,
            budget_id: cat.budget_id,
            name: cat.name,
            allocated_amount: cat.allocated_amount,
            spent_amount: cat.spent_amount,
            remaining_amount,
            color_hex: cat.color_hex,
            created_at: cat.created_at,
            updated_at: cat.updated_at,
        }
    }

    pub fn from_category(cat: Category) -> Self {
        Self {
            id: cat.id,
            budget_id: cat.budget_id,
            name: cat.name,
            allocated_amount: cat.allocated_amount,
            spent_amount: Decimal::ZERO,
            remaining_amount: cat.allocated_amount,
            color_hex: cat.color_hex,
            created_at: cat.created_at,
            updated_at: cat.updated_at,
        }
    }
}

fn default_color() -> String {
    "#64748b".to_string()
}

/// DTO for creating a category
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct CreateCategoryDto {
    pub budget_id: Uuid,

    #[validate(length(min = 1, max = 50, message = "Name must be 1-50 characters"))]
    pub name: String,

    #[serde(default)]
    pub allocated_amount: Option<Decimal>,

    #[validate(custom(
        function = "validate_color_hex",
        message = "Color must be in #RRGGBB format"
    ))]
    #[serde(default = "default_color")]
    pub color_hex: String,
}

impl CreateCategoryDto {
    /// Validate decimal fields
    pub fn validate_decimals(&self) -> Result<(), ValidationError> {
        if let Some(amount) = &self.allocated_amount {
            validate_non_negative(amount)?;
        }
        Ok(())
    }
}

/// DTO for updating a category (all fields optional)
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCategoryDto {
    #[validate(length(min = 1, max = 50, message = "Name must be 1-50 characters"))]
    pub name: Option<String>,

    pub allocated_amount: Option<Decimal>,

    pub color_hex: Option<String>,
}

impl UpdateCategoryDto {
    /// Validate decimal and color fields
    pub fn validate_fields(&self) -> Result<(), ValidationError> {
        if let Some(amount) = &self.allocated_amount {
            validate_non_negative(amount)?;
        }
        if let Some(color) = &self.color_hex {
            validate_color_hex(color)?;
        }
        Ok(())
    }
}

/// Path parameters for category ID
#[derive(Debug, Deserialize)]
pub struct CategoryIdPath {
    pub id: Uuid,
}

/// Path parameters for budget ID
#[derive(Debug, Deserialize)]
pub struct BudgetIdPath {
    pub budget_id: Uuid,
}
