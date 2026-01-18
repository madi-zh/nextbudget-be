use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;
use validator::{Validate, ValidationError};

/// Transaction type enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum TransactionType {
    /// Money spent (decreases account balance)
    #[default]
    Expense,
    /// Money received (increases account balance)
    Income,
    /// Transfer between accounts (no balance change)
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

/// Transaction information returned in responses
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TransactionResponse {
    /// Unique transaction identifier
    pub id: Uuid,
    /// Category this transaction belongs to
    pub category_id: Uuid,
    /// Account used for this transaction (optional)
    pub account_id: Option<Uuid>,
    /// Transaction amount (always positive)
    #[schema(example = 50.00)]
    pub amount: Decimal,
    /// Date of the transaction
    pub transaction_date: DateTime<Utc>,
    /// Optional description
    #[schema(example = "Weekly groceries")]
    pub description: Option<String>,
    /// Transaction type (expense, income, transfer)
    #[schema(example = "expense")]
    pub transaction_type: String,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Last update timestamp
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

/// Request body for creating a transaction
#[derive(Debug, Deserialize, Validate, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateTransactionDto {
    /// Category this transaction belongs to
    pub category_id: Uuid,

    /// Account to use (optional)
    pub account_id: Option<Uuid>,

    /// Transaction amount (must be positive)
    #[validate(custom(
        function = "validate_positive_amount",
        message = "Amount must be positive"
    ))]
    #[schema(example = 50.00)]
    pub amount: Decimal,

    /// Date of the transaction
    pub transaction_date: DateTime<Utc>,

    /// Optional description (max 200 chars)
    #[validate(length(max = 200, message = "Description cannot exceed 200 characters"))]
    #[schema(example = "Weekly groceries")]
    pub description: Option<String>,

    /// Transaction type (defaults to expense)
    #[serde(default)]
    pub transaction_type: TransactionType,
}

/// Request body for updating a transaction (PATCH - all fields optional)
#[derive(Debug, Deserialize, Validate, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateTransactionDto {
    /// Category ID
    pub category_id: Option<Uuid>,

    /// Account ID (use null to remove account association)
    pub account_id: Option<Option<Uuid>>,

    /// Transaction amount
    #[schema(example = 75.00)]
    pub amount: Option<Decimal>,

    /// Transaction date
    pub transaction_date: Option<DateTime<Utc>>,

    /// Description
    #[validate(length(max = 200, message = "Description cannot exceed 200 characters"))]
    #[schema(example = "Updated description")]
    pub description: Option<String>,

    /// Transaction type
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
#[derive(Debug, Deserialize, Validate, IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct TransactionFilters {
    /// Filter by start date
    pub start_date: Option<DateTime<Utc>>,
    /// Filter by end date
    pub end_date: Option<DateTime<Utc>>,
    /// Filter by category
    pub category_id: Option<Uuid>,
    /// Filter by account
    pub account_id: Option<Uuid>,
    /// Filter by type (expense, income, transfer)
    #[param(example = "expense")]
    pub transaction_type: Option<String>,

    /// Maximum results (1-100)
    #[validate(range(min = 1, max = 100))]
    #[serde(default = "default_limit")]
    #[param(example = 50)]
    pub limit: i64,

    /// Number of results to skip
    #[validate(range(min = 0))]
    #[serde(default)]
    #[param(example = 0)]
    pub offset: i64,
}

fn default_limit() -> i64 {
    50
}

/// Request body for fetching transactions by multiple categories
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CategoriesQueryDto {
    /// List of category IDs to fetch transactions for
    pub category_ids: Vec<Uuid>,
}

/// Paginated response wrapper
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PaginatedTransactionResponse {
    /// List of transactions
    pub data: Vec<TransactionResponse>,
    /// Total count matching filters
    #[schema(example = 100)]
    pub total: i64,
    /// Limit used
    #[schema(example = 50)]
    pub limit: i64,
    /// Offset used
    #[schema(example = 0)]
    pub offset: i64,
}

/// Path parameters for transaction ID
#[derive(Debug, Deserialize, IntoParams)]
pub struct TransactionIdPath {
    /// Transaction UUID
    pub id: Uuid,
}

/// Path parameters for category ID
#[derive(Debug, Deserialize, IntoParams)]
pub struct CategoryIdPath {
    /// Category UUID
    pub category_id: Uuid,
}
