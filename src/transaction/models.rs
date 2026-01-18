use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use validator::{Validate, ValidationError};

/// Transaction type enum for type safety
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TransactionType {
    #[default]
    Expense,
    Income,
    Transfer,
}

impl TransactionType {
    pub fn as_str(&self) -> &'static str {
        match self {
            TransactionType::Expense => "expense",
            TransactionType::Income => "income",
            TransactionType::Transfer => "transfer",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "expense" => Some(TransactionType::Expense),
            "income" => Some(TransactionType::Income),
            "transfer" => Some(TransactionType::Transfer),
            _ => None,
        }
    }
}

/// Validate that amount is positive
fn validate_positive_amount(amount: &Decimal) -> Result<(), ValidationError> {
    if *amount <= Decimal::ZERO {
        return Err(ValidationError::new("amount_must_be_positive"));
    }
    Ok(())
}

/// Database model for transactions
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Transaction {
    pub id: Uuid,
    pub category_id: Uuid,
    pub account_id: Option<Uuid>,
    pub amount: Decimal,
    pub transaction_date: DateTime<Utc>,
    pub description: Option<String>,
    pub transaction_type: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Transaction {
    pub fn get_type(&self) -> TransactionType {
        TransactionType::parse(&self.transaction_type).unwrap_or_default()
    }
}

/// Response DTO for transaction
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionResponse {
    pub id: Uuid,
    pub category_id: Uuid,
    pub account_id: Option<Uuid>,
    pub amount: Decimal,
    pub transaction_date: DateTime<Utc>,
    pub description: Option<String>,
    pub transaction_type: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<Transaction> for TransactionResponse {
    fn from(t: Transaction) -> Self {
        Self {
            id: t.id,
            category_id: t.category_id,
            account_id: t.account_id,
            amount: t.amount,
            transaction_date: t.transaction_date,
            description: t.description,
            transaction_type: t.transaction_type,
            created_at: t.created_at,
            updated_at: t.updated_at,
        }
    }
}

/// DTO for creating a transaction
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct CreateTransactionDto {
    pub category_id: Uuid,

    pub account_id: Option<Uuid>,

    #[validate(custom(
        function = "validate_positive_amount",
        message = "Amount must be positive"
    ))]
    pub amount: Decimal,

    pub transaction_date: DateTime<Utc>,

    #[validate(length(max = 200, message = "Description cannot exceed 200 characters"))]
    pub description: Option<String>,

    #[serde(default)]
    pub transaction_type: TransactionType,
}

/// DTO for updating a transaction (all fields optional)
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct UpdateTransactionDto {
    pub category_id: Option<Uuid>,

    /// None = don't update, Some(None) = set to NULL, Some(Some(id)) = set to id
    pub account_id: Option<Option<Uuid>>,

    pub amount: Option<Decimal>,

    pub transaction_date: Option<DateTime<Utc>>,

    #[validate(length(max = 200, message = "Description cannot exceed 200 characters"))]
    pub description: Option<String>,

    pub transaction_type: Option<TransactionType>,
}

impl UpdateTransactionDto {
    /// Validate amount if provided
    pub fn validate_amount(&self) -> Result<(), ValidationError> {
        if let Some(amount) = &self.amount {
            validate_positive_amount(amount)?;
        }
        Ok(())
    }
}

/// Query parameters for listing transactions
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct TransactionFilters {
    pub start_date: Option<DateTime<Utc>>,
    pub end_date: Option<DateTime<Utc>>,
    pub category_id: Option<Uuid>,
    pub account_id: Option<Uuid>,
    pub transaction_type: Option<String>,

    #[validate(range(min = 1, max = 100))]
    #[serde(default = "default_limit")]
    pub limit: i64,

    #[validate(range(min = 0))]
    #[serde(default)]
    pub offset: i64,
}

fn default_limit() -> i64 {
    50
}

/// Request body for fetching transactions by multiple categories
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoriesQueryDto {
    pub category_ids: Vec<Uuid>,
}

/// Paginated response wrapper
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaginatedResponse<T> {
    pub data: Vec<T>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

/// Path parameters for transaction ID
#[derive(Debug, Deserialize)]
pub struct TransactionIdPath {
    pub id: Uuid,
}

/// Path parameters for category ID
#[derive(Debug, Deserialize)]
pub struct CategoryIdPath {
    pub category_id: Uuid,
}
