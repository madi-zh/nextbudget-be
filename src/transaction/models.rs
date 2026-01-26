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
    pub destination_account_id: Option<Uuid>,
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
    /// Account used for this transaction (optional, source account for transfers)
    pub account_id: Option<Uuid>,
    /// Destination account for transfer transactions (only present for transfers)
    pub destination_account_id: Option<Uuid>,
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
            destination_account_id: t.destination_account_id,
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

    /// Account to use (optional, source account for transfers)
    pub account_id: Option<Uuid>,

    /// Destination account for transfer transactions (only allowed for transfers)
    pub destination_account_id: Option<Uuid>,

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

impl CreateTransactionDto {
    /// Validate transfer-specific constraints
    pub fn validate_transfer(&self) -> Result<(), ValidationError> {
        // destination_account_id is only allowed for transfers
        if self.transaction_type != TransactionType::Transfer
            && self.destination_account_id.is_some()
        {
            return Err(ValidationError::new(
                "destination_account_id is only allowed for transfer transactions",
            ));
        }
        // For transfers, source and destination must be different if both provided
        if self.transaction_type == TransactionType::Transfer {
            if let (Some(src), Some(dst)) = (self.account_id, self.destination_account_id) {
                if src == dst {
                    return Err(ValidationError::new(
                        "source and destination accounts must be different",
                    ));
                }
            }
        }
        Ok(())
    }
}

/// Request body for updating a transaction (PATCH - all fields optional)
#[derive(Debug, Deserialize, Validate, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateTransactionDto {
    /// Category ID
    pub category_id: Option<Uuid>,

    /// Account ID (use null to remove account association)
    pub account_id: Option<Option<Uuid>>,

    /// Destination account ID for transfers (use null to remove)
    pub destination_account_id: Option<Option<Uuid>>,

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

    /// Validate transfer-specific constraints (requires knowing the final transaction type)
    pub fn validate_transfer(
        &self,
        final_type: TransactionType,
        final_account_id: Option<Uuid>,
        final_destination_id: Option<Uuid>,
    ) -> Result<(), ValidationError> {
        // destination_account_id is only allowed for transfers
        if final_type != TransactionType::Transfer && final_destination_id.is_some() {
            return Err(ValidationError::new(
                "destination_account_id is only allowed for transfer transactions",
            ));
        }
        // For transfers, source and destination must be different if both provided
        if final_type == TransactionType::Transfer {
            if let (Some(src), Some(dst)) = (final_account_id, final_destination_id) {
                if src == dst {
                    return Err(ValidationError::new(
                        "source and destination accounts must be different",
                    ));
                }
            }
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

/// Path parameters for account ID (transactions by account)
#[derive(Debug, Deserialize, IntoParams)]
pub struct AccountIdPath {
    /// Account UUID
    pub account_id: Uuid,
}

/// Embedded account information for detailed responses
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddedAccountInfo {
    /// Account ID
    pub id: Uuid,
    /// Account name
    #[schema(example = "My Checking")]
    pub name: String,
    /// Account type (checking, savings, credit)
    #[serde(rename = "type")]
    #[schema(example = "checking")]
    pub account_type: String,
    /// Display color in hex format
    #[schema(example = "#4CAF50")]
    pub color_hex: String,
    /// Currency code
    #[schema(example = "USD")]
    pub currency: String,
}

/// Embedded category information for detailed responses
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddedCategoryInfo {
    /// Category ID
    pub id: Uuid,
    /// Category name
    #[schema(example = "Groceries")]
    pub name: String,
    /// Display color in hex format
    #[schema(example = "#4CAF50")]
    pub color_hex: String,
}

/// Detailed transaction response with embedded account and category info
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TransactionDetailResponse {
    /// Unique transaction identifier
    pub id: Uuid,
    /// Category details
    pub category: EmbeddedCategoryInfo,
    /// Source account details (optional)
    pub account: Option<EmbeddedAccountInfo>,
    /// Destination account details for transfers (optional)
    pub destination_account: Option<EmbeddedAccountInfo>,
    /// Transaction amount (always positive)
    #[schema(example = 50.00)]
    pub amount: Decimal,
    /// Transaction type (expense, income, transfer)
    #[schema(example = "expense")]
    pub transaction_type: String,
    /// Date of the transaction
    pub transaction_date: DateTime<Utc>,
    /// Optional description
    #[schema(example = "Weekly groceries")]
    pub description: Option<String>,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Last update timestamp
    pub updated_at: DateTime<Utc>,
}

/// Database row for detailed transaction query with JOINs
#[derive(Debug, FromRow)]
pub struct TransactionDetailRow {
    pub id: Uuid,
    pub amount: Decimal,
    pub transaction_type: String,
    pub transaction_date: DateTime<Utc>,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    // Category fields
    pub category_id: Uuid,
    pub category_name: String,
    pub category_color_hex: String,
    // Source account fields (optional)
    pub account_id: Option<Uuid>,
    pub account_name: Option<String>,
    pub account_type: Option<String>,
    pub account_color_hex: Option<String>,
    pub account_currency: Option<String>,
    // Destination account fields (optional, for transfers)
    pub dest_account_id: Option<Uuid>,
    pub dest_account_name: Option<String>,
    pub dest_account_type: Option<String>,
    pub dest_account_color_hex: Option<String>,
    pub dest_account_currency: Option<String>,
}

impl TransactionDetailRow {
    pub fn into_response(self) -> TransactionDetailResponse {
        let category = EmbeddedCategoryInfo {
            id: self.category_id,
            name: self.category_name,
            color_hex: self.category_color_hex,
        };

        let account = match (
            self.account_id,
            self.account_name,
            self.account_type,
            self.account_color_hex,
            self.account_currency,
        ) {
            (Some(id), Some(name), Some(account_type), Some(color_hex), Some(currency)) => {
                Some(EmbeddedAccountInfo {
                    id,
                    name,
                    account_type,
                    color_hex,
                    currency,
                })
            }
            _ => None,
        };

        let destination_account = match (
            self.dest_account_id,
            self.dest_account_name,
            self.dest_account_type,
            self.dest_account_color_hex,
            self.dest_account_currency,
        ) {
            (Some(id), Some(name), Some(account_type), Some(color_hex), Some(currency)) => {
                Some(EmbeddedAccountInfo {
                    id,
                    name,
                    account_type,
                    color_hex,
                    currency,
                })
            }
            _ => None,
        };

        TransactionDetailResponse {
            id: self.id,
            category,
            account,
            destination_account,
            amount: self.amount,
            transaction_type: self.transaction_type,
            transaction_date: self.transaction_date,
            description: self.description,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

/// Paginated response for detailed transactions
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PaginatedDetailedTransactionResponse {
    /// List of detailed transactions
    pub data: Vec<TransactionDetailResponse>,
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

/// Summary of spending by category
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CategorySpendingSummary {
    /// Category ID
    pub category_id: Uuid,
    /// Category name
    #[schema(example = "Groceries")]
    pub category_name: String,
    /// Category color
    #[schema(example = "#4CAF50")]
    pub category_color_hex: String,
    /// Total amount spent in this category
    #[schema(example = 350.00)]
    pub total_amount: Decimal,
    /// Number of transactions
    #[schema(example = 15)]
    pub transaction_count: i64,
}

/// Database row for category summary query
#[derive(Debug, FromRow)]
pub struct CategorySummaryRow {
    pub category_id: Uuid,
    pub category_name: String,
    pub category_color_hex: String,
    pub total_amount: Decimal,
    pub transaction_count: i64,
}

impl From<CategorySummaryRow> for CategorySpendingSummary {
    fn from(row: CategorySummaryRow) -> Self {
        Self {
            category_id: row.category_id,
            category_name: row.category_name,
            category_color_hex: row.category_color_hex,
            total_amount: row.total_amount,
            transaction_count: row.transaction_count,
        }
    }
}

/// Transaction summary with totals and breakdown by category
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TransactionSummary {
    /// Total income in the period
    #[schema(example = 5000.00)]
    pub total_income: Decimal,
    /// Total expenses in the period
    #[schema(example = 3500.00)]
    pub total_expenses: Decimal,
    /// Net change (income - expenses)
    #[schema(example = 1500.00)]
    pub net_change: Decimal,
    /// Total number of transactions
    #[schema(example = 45)]
    pub transaction_count: i64,
    /// Breakdown by category
    pub by_category: Vec<CategorySpendingSummary>,
}

/// Query parameters for transaction summary
#[derive(Debug, Deserialize, Validate, IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct SummaryFilters {
    /// Filter by start date
    pub start_date: Option<DateTime<Utc>>,
    /// Filter by end date
    pub end_date: Option<DateTime<Utc>>,
    /// Filter by account
    pub account_id: Option<Uuid>,
}

/// Query parameters for listing transactions (with detailed option)
#[derive(Debug, Deserialize, Validate, IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct TransactionFiltersDetailed {
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

    /// Include full account/category details in response
    #[serde(default)]
    #[param(example = false)]
    pub detailed: bool,
}
