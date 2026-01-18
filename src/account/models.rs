use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use validator::{Validate, ValidationError};

/// Account type enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccountType {
    Checking,
    Savings,
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

/// Response DTO for account
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountResponse {
    pub id: Uuid,
    pub name: String,
    #[serde(rename = "type")]
    pub account_type: String,
    pub balance: Decimal,
    pub color_hex: String,
    pub created_at: DateTime<Utc>,
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
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountsListResponse {
    pub accounts: Vec<AccountResponse>,
    pub count: usize,
}

/// Summary statistics for accounts
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountsSummary {
    pub total_savings: Decimal,
    pub total_spending: Decimal,
    pub net_worth: Decimal,
    pub accounts_count: i64,
}

/// Response for accounts with summary
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountsSummaryResponse {
    pub accounts: Vec<AccountResponse>,
    pub summary: AccountsSummary,
}

/// Summary row from database query
#[derive(Debug, FromRow)]
pub struct SummaryRow {
    pub total_savings: Option<Decimal>,
    pub total_spending: Option<Decimal>,
    pub accounts_count: Option<i64>,
}

/// Delete response
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteResponse {
    pub message: String,
    pub id: Uuid,
}

/// DTO for creating an account
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct CreateAccountDto {
    #[validate(length(min = 1, max = 50, message = "Name must be 1-50 characters"))]
    pub name: String,

    #[serde(rename = "type")]
    pub account_type: AccountType,

    #[serde(default)]
    pub balance: Option<Decimal>,

    #[validate(custom(
        function = "validate_color_hex",
        message = "Color must be #RRGGBB format"
    ))]
    pub color_hex: String,
}

/// DTO for updating an account (all fields optional)
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAccountDto {
    #[validate(length(min = 1, max = 50, message = "Name must be 1-50 characters"))]
    pub name: Option<String>,

    #[serde(rename = "type")]
    pub account_type: Option<AccountType>,

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

/// DTO for updating balance only
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateBalanceDto {
    pub balance: Decimal,
}

/// Path parameters for account ID
#[derive(Debug, Deserialize)]
pub struct AccountIdPath {
    pub id: Uuid,
}

/// Path parameters for account type
#[derive(Debug, Deserialize)]
pub struct AccountTypePath {
    #[serde(rename = "type")]
    pub account_type: String,
}
