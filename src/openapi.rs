use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::{Modify, OpenApi};

use crate::account::models::{
    AccountResponse, AccountType, AccountsListResponse, AccountsSummary, AccountsSummaryResponse,
    CreateAccountDto, CurrencySummary, DeleteResponse, UpdateAccountDto, UpdateBalanceDto,
};
use crate::auth::models::{
    AuthTokenResponse, CreateUserDto, GoogleLoginDto, LoginDto, RefreshTokenDto, UserResponseDto,
};
use crate::budget::models::{
    BudgetResponse, CreateBudgetDto, UpdateBudgetDto, UpdateIncomeDto, UpdateSavingsRateDto,
};
use crate::category::models::{CategoryResponse, CreateCategoryDto, UpdateCategoryDto};
use crate::currency::models::{CurrenciesListResponse, CurrencyResponse, SyncRatesResponse};
use crate::errors::ErrorResponse;
use crate::transaction::models::{
    CategoriesQueryDto, CategorySpendingSummary, CreateTransactionDto, EmbeddedAccountInfo,
    EmbeddedCategoryInfo, PaginatedDetailedTransactionResponse, PaginatedTransactionResponse,
    TransactionDetailResponse, TransactionResponse, TransactionSummary, TransactionType,
    UpdateTransactionDto,
};

/// Security scheme modifier for Bearer token authentication
struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        if let Some(components) = openapi.components.as_mut() {
            components.add_security_scheme(
                "bearer_auth",
                SecurityScheme::Http(
                    HttpBuilder::new()
                        .scheme(HttpAuthScheme::Bearer)
                        .bearer_format("JWT")
                        .description(Some("JWT access token"))
                        .build(),
                ),
            );
        }
    }
}

/// OpenAPI documentation configuration
#[derive(OpenApi)]
#[openapi(
    info(
        title = "BudgetFlow API",
        version = "1.0.0",
        description = "RESTful API for budget and financial tracking",
        contact(
            name = "API Support",
            email = "support@example.com"
        ),
        license(
            name = "MIT"
        )
    ),
    servers(
        (url = "http://localhost:8080", description = "Development server"),
    ),
    tags(
        (name = "Health", description = "Health check endpoints"),
        (name = "Auth", description = "Authentication and user management"),
        (name = "Budgets", description = "Monthly budget management"),
        (name = "Accounts", description = "Financial account management"),
        (name = "Categories", description = "Budget category management"),
        (name = "Transactions", description = "Transaction management with atomic balance updates"),
        (name = "Currencies", description = "Currency and exchange rate management")
    ),
    paths(
        // Auth endpoints
        crate::auth::handlers::register,
        crate::auth::handlers::login,
        crate::auth::handlers::google_login,
        crate::auth::handlers::refresh,
        crate::auth::handlers::logout,
        crate::auth::handlers::me,
        // Budget endpoints
        crate::budget::handlers::list_budgets,
        crate::budget::handlers::get_budget,
        crate::budget::handlers::get_budget_by_month_year,
        crate::budget::handlers::create_budget,
        crate::budget::handlers::update_budget,
        crate::budget::handlers::update_income,
        crate::budget::handlers::update_savings_rate,
        crate::budget::handlers::delete_budget,
        // Account endpoints
        crate::account::handlers::list_accounts,
        crate::account::handlers::get_accounts_summary,
        crate::account::handlers::get_accounts_by_type,
        crate::account::handlers::get_account,
        crate::account::handlers::create_account,
        crate::account::handlers::update_account,
        crate::account::handlers::update_account_balance,
        crate::account::handlers::delete_account,
        // Category endpoints
        crate::category::handlers::list_categories,
        crate::category::handlers::get_categories_by_budget,
        crate::category::handlers::get_category,
        crate::category::handlers::create_category,
        crate::category::handlers::update_category,
        crate::category::handlers::delete_category,
        // Transaction endpoints
        crate::transaction::handlers::list_transactions,
        crate::transaction::handlers::get_by_category,
        crate::transaction::handlers::get_by_categories,
        crate::transaction::handlers::get_by_account,
        crate::transaction::handlers::get_summary,
        crate::transaction::handlers::get_transaction,
        crate::transaction::handlers::create_transaction,
        crate::transaction::handlers::update_transaction,
        crate::transaction::handlers::delete_transaction,
        // Currency endpoints
        crate::currency::handlers::list_currencies,
        crate::currency::handlers::sync_exchange_rates,
    ),
    components(
        schemas(
            // Error response
            ErrorResponse,
            // Auth schemas
            CreateUserDto,
            LoginDto,
            GoogleLoginDto,
            RefreshTokenDto,
            UserResponseDto,
            AuthTokenResponse,
            // Budget schemas
            BudgetResponse,
            CreateBudgetDto,
            UpdateBudgetDto,
            UpdateIncomeDto,
            UpdateSavingsRateDto,
            // Account schemas
            AccountType,
            AccountResponse,
            AccountsListResponse,
            AccountsSummary,
            CurrencySummary,
            AccountsSummaryResponse,
            CreateAccountDto,
            UpdateAccountDto,
            UpdateBalanceDto,
            DeleteResponse,
            // Category schemas
            CategoryResponse,
            CreateCategoryDto,
            UpdateCategoryDto,
            // Transaction schemas
            TransactionType,
            TransactionResponse,
            TransactionDetailResponse,
            EmbeddedAccountInfo,
            EmbeddedCategoryInfo,
            PaginatedTransactionResponse,
            PaginatedDetailedTransactionResponse,
            TransactionSummary,
            CategorySpendingSummary,
            CreateTransactionDto,
            UpdateTransactionDto,
            CategoriesQueryDto,
            // Currency schemas
            CurrencyResponse,
            CurrenciesListResponse,
            SyncRatesResponse,
        )
    ),
    modifiers(&SecurityAddon)
)]
pub struct ApiDoc;
