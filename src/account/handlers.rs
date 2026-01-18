use actix_web::{delete, get, patch, post, web, HttpResponse};
use sqlx::PgPool;
use validator::Validate;

use crate::errors::AppError;
use crate::extractors::AuthenticatedUser;

use super::models::{
    AccountIdPath, AccountResponse, AccountTypePath, AccountsListResponse, AccountsSummaryResponse,
    CreateAccountDto, DeleteResponse, UpdateAccountDto, UpdateBalanceDto,
};
use super::service::AccountService;

/// GET /accounts - List all accounts for the authenticated user
#[get("/accounts")]
pub async fn list_accounts(
    pool: web::Data<PgPool>,
    auth: AuthenticatedUser,
) -> Result<HttpResponse, AppError> {
    let accounts = AccountService::list_accounts(pool.get_ref(), auth.user_id).await?;

    let response = AccountsListResponse {
        count: accounts.len(),
        accounts: accounts
            .into_iter()
            .map(AccountResponse::from_account)
            .collect(),
    };

    Ok(HttpResponse::Ok().json(response))
}

/// GET /accounts/summary - Get all accounts with financial summary
#[get("/accounts/summary")]
pub async fn get_accounts_summary(
    pool: web::Data<PgPool>,
    auth: AuthenticatedUser,
) -> Result<HttpResponse, AppError> {
    let (accounts, summary) =
        AccountService::get_accounts_summary(pool.get_ref(), auth.user_id).await?;

    let response = AccountsSummaryResponse {
        accounts: accounts
            .into_iter()
            .map(AccountResponse::from_account)
            .collect(),
        summary,
    };

    Ok(HttpResponse::Ok().json(response))
}

/// GET /accounts/type/{type} - Get accounts by type
#[get("/accounts/type/{type}")]
pub async fn get_accounts_by_type(
    pool: web::Data<PgPool>,
    auth: AuthenticatedUser,
    path: web::Path<AccountTypePath>,
) -> Result<HttpResponse, AppError> {
    let accounts =
        AccountService::get_accounts_by_type(pool.get_ref(), auth.user_id, &path.account_type)
            .await?;

    let response = AccountsListResponse {
        count: accounts.len(),
        accounts: accounts
            .into_iter()
            .map(AccountResponse::from_account)
            .collect(),
    };

    Ok(HttpResponse::Ok().json(response))
}

/// GET /accounts/{id} - Get a specific account by ID
#[get("/accounts/{id}")]
pub async fn get_account(
    pool: web::Data<PgPool>,
    auth: AuthenticatedUser,
    path: web::Path<AccountIdPath>,
) -> Result<HttpResponse, AppError> {
    let account = AccountService::get_account_by_id(pool.get_ref(), path.id, auth.user_id).await?;

    Ok(HttpResponse::Ok().json(AccountResponse::from_account(account)))
}

/// POST /accounts - Create a new account
#[post("/accounts")]
pub async fn create_account(
    pool: web::Data<PgPool>,
    auth: AuthenticatedUser,
    body: web::Json<CreateAccountDto>,
) -> Result<HttpResponse, AppError> {
    body.validate()
        .map_err(|e| AppError::ValidationError(e.to_string()))?;

    let account = AccountService::create_account(pool.get_ref(), auth.user_id, &body).await?;

    Ok(HttpResponse::Created().json(AccountResponse::from_account(account)))
}

/// PATCH /accounts/{id} - Update an account (partial update)
#[patch("/accounts/{id}")]
pub async fn update_account(
    pool: web::Data<PgPool>,
    auth: AuthenticatedUser,
    path: web::Path<AccountIdPath>,
    body: web::Json<UpdateAccountDto>,
) -> Result<HttpResponse, AppError> {
    body.validate()
        .map_err(|e| AppError::ValidationError(e.to_string()))?;
    body.validate_color_hex()
        .map_err(|e| AppError::ValidationError(e.to_string()))?;

    let account =
        AccountService::update_account(pool.get_ref(), path.id, auth.user_id, &body).await?;

    Ok(HttpResponse::Ok().json(AccountResponse::from_account(account)))
}

/// PATCH /accounts/{id}/balance - Update account balance only
#[patch("/accounts/{id}/balance")]
pub async fn update_account_balance(
    pool: web::Data<PgPool>,
    auth: AuthenticatedUser,
    path: web::Path<AccountIdPath>,
    body: web::Json<UpdateBalanceDto>,
) -> Result<HttpResponse, AppError> {
    let account =
        AccountService::update_balance(pool.get_ref(), path.id, auth.user_id, &body).await?;

    Ok(HttpResponse::Ok().json(AccountResponse::from_account(account)))
}

/// DELETE /accounts/{id} - Delete an account
#[delete("/accounts/{id}")]
pub async fn delete_account(
    pool: web::Data<PgPool>,
    auth: AuthenticatedUser,
    path: web::Path<AccountIdPath>,
) -> Result<HttpResponse, AppError> {
    AccountService::delete_account(pool.get_ref(), path.id, auth.user_id).await?;

    Ok(HttpResponse::Ok().json(DeleteResponse {
        message: "Account deleted successfully".to_string(),
        id: path.id,
    }))
}
