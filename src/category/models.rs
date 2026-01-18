use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::{IntoParams, ToSchema};
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

/// Category information returned in responses
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CategoryResponse {
    /// Unique category identifier
    pub id: Uuid,
    /// Parent budget ID
    pub budget_id: Uuid,
    /// Category name
    #[schema(example = "Groceries")]
    pub name: String,
    /// Amount allocated to this category
    #[schema(example = 500.00)]
    pub allocated_amount: Decimal,
    /// Computed: total expenses in this category
    #[schema(example = 350.00)]
    pub spent_amount: Decimal,
    /// Computed: allocated - spent
    #[schema(example = 150.00)]
    pub remaining_amount: Decimal,
    /// Display color in hex format
    #[schema(example = "#4CAF50")]
    pub color_hex: String,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Last update timestamp
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

/// Request body for creating a category
#[derive(Debug, Deserialize, Validate, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateCategoryDto {
    /// Parent budget ID
    pub budget_id: Uuid,

    /// Category name (1-50 characters)
    #[validate(length(min = 1, max = 50, message = "Name must be 1-50 characters"))]
    #[schema(example = "Groceries")]
    pub name: String,

    /// Amount allocated (defaults to 0)
    #[serde(default)]
    #[schema(example = 500.00)]
    pub allocated_amount: Option<Decimal>,

    /// Display color in hex format (defaults to #64748b)
    #[validate(custom(
        function = "validate_color_hex",
        message = "Color must be in #RRGGBB format"
    ))]
    #[serde(default = "default_color")]
    #[schema(example = "#4CAF50")]
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

/// Request body for updating a category (PATCH - all fields optional)
#[derive(Debug, Deserialize, Validate, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCategoryDto {
    /// Category name
    #[validate(length(min = 1, max = 50, message = "Name must be 1-50 characters"))]
    #[schema(example = "Food & Dining")]
    pub name: Option<String>,

    /// Amount allocated
    #[schema(example = 600.00)]
    pub allocated_amount: Option<Decimal>,

    /// Display color in hex format
    #[schema(example = "#2196F3")]
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
#[derive(Debug, Deserialize, IntoParams)]
pub struct CategoryIdPath {
    /// Category UUID
    pub id: Uuid,
}

/// Path parameters for budget ID
#[derive(Debug, Deserialize, IntoParams)]
pub struct BudgetIdPath {
    /// Budget UUID
    pub budget_id: Uuid,
}
