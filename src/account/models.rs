use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;
use validator::{Validate, ValidationError};

/// Account type enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum AccountType {
    /// Checking account for daily transactions
    Checking,
    /// Savings account
    Savings,
    /// Credit card account
    Credit,
}

impl AccountType {
    pub fn as_str(&self) -> &'static str {
        match self {
            AccountType::Checking => "checking",
            AccountType::Savings => "savings",
            AccountType::Credit => "credit",
        }
    }

    #[allow(dead_code)]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "checking" => Some(AccountType::Checking),
            "savings" => Some(AccountType::Savings),
            "credit" => Some(AccountType::Credit),
            _ => None,
        }
    }
}

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

/// Database entity for accounts
#[derive(Debug, Clone, FromRow)]
pub struct Account {
    pub id: Uuid,
    #[allow(dead_code)]
    pub owner_id: Uuid,
    pub name: String,
    #[sqlx(rename = "account_type")]
    pub account_type: String,
    pub balance: Decimal,
    pub color_hex: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Account information returned in responses
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AccountResponse {
    /// Unique account identifier
    pub id: Uuid,
    /// Account name
    #[schema(example = "My Checking")]
    pub name: String,
    /// Account type (checking, savings, credit)
    #[serde(rename = "type")]
    #[schema(example = "checking")]
    pub account_type: String,
    /// Current balance
    #[schema(example = 1500.00)]
    pub balance: Decimal,
    /// Display color in hex format
    #[schema(example = "#4CAF50")]
    pub color_hex: String,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Last update timestamp
    pub updated_at: DateTime<Utc>,
}

impl AccountResponse {
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

/// Response for listing accounts
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AccountsListResponse {
    /// List of accounts
    pub accounts: Vec<AccountResponse>,
    /// Total count
    #[schema(example = 3)]
    pub count: usize,
}

/// Summary statistics for accounts
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AccountsSummary {
    /// Total balance in savings accounts
    #[schema(example = 10000.00)]
    pub total_savings: Decimal,
    /// Total balance in checking/credit accounts
    #[schema(example = 2500.00)]
    pub total_spending: Decimal,
    /// Net worth (savings + spending)
    #[schema(example = 12500.00)]
    pub net_worth: Decimal,
    /// Number of accounts
    #[schema(example = 3)]
    pub accounts_count: i64,
}

/// Response for accounts with summary
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AccountsSummaryResponse {
    /// List of accounts
    pub accounts: Vec<AccountResponse>,
    /// Financial summary
    pub summary: AccountsSummary,
}

/// Summary row from database query
#[derive(Debug, FromRow)]
pub struct SummaryRow {
    pub total_savings: Option<Decimal>,
    pub total_spending: Option<Decimal>,
    pub accounts_count: Option<i64>,
}

/// Delete operation response
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeleteResponse {
    /// Success message
    #[schema(example = "Account deleted successfully")]
    pub message: String,
    /// Deleted resource ID
    pub id: Uuid,
}

/// Request body for creating an account
#[derive(Debug, Deserialize, Validate, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateAccountDto {
    /// Account name (1-50 characters)
    #[validate(length(min = 1, max = 50, message = "Name must be 1-50 characters"))]
    #[schema(example = "My Checking")]
    pub name: String,

    /// Account type
    #[serde(rename = "type")]
    pub account_type: AccountType,

    /// Initial balance (defaults to 0)
    #[serde(default)]
    #[schema(example = 1000.00)]
    pub balance: Option<Decimal>,

    /// Display color in hex format (#RRGGBB)
    #[validate(custom(
        function = "validate_color_hex",
        message = "Color must be #RRGGBB format"
    ))]
    #[schema(example = "#4CAF50")]
    pub color_hex: String,
}

/// Request body for updating an account (PATCH - all fields optional)
#[derive(Debug, Deserialize, Validate, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAccountDto {
    /// Account name
    #[validate(length(min = 1, max = 50, message = "Name must be 1-50 characters"))]
    #[schema(example = "My Savings")]
    pub name: Option<String>,

    /// Account type
    #[serde(rename = "type")]
    pub account_type: Option<AccountType>,

    /// Display color in hex format
    #[schema(example = "#2196F3")]
    pub color_hex: Option<String>,
}

impl UpdateAccountDto {
    /// Validate optional color_hex field
    pub fn validate_color_hex(&self) -> Result<(), ValidationError> {
        if let Some(color) = &self.color_hex {
            validate_color_hex(color)?;
        }
        Ok(())
    }
}

/// Request body for updating balance only
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateBalanceDto {
    /// New balance value
    #[schema(example = 2500.00)]
    pub balance: Decimal,
}

/// Path parameters for account ID
#[derive(Debug, Deserialize, IntoParams)]
pub struct AccountIdPath {
    /// Account UUID
    pub id: Uuid,
}

/// Path parameters for account type
#[derive(Debug, Deserialize, IntoParams)]
pub struct AccountTypePath {
    /// Account type (checking, savings, credit)
    #[serde(rename = "type")]
    #[param(example = "checking")]
    pub account_type: String,
}
