use actix_web::{delete, get, patch, post, web, HttpResponse};
use sqlx::PgPool;
use validator::Validate;

use crate::errors::AppError;
use crate::extractors::AuthenticatedUser;

use super::models::{
    CategoriesQueryDto, CategoryIdPath, CreateTransactionDto, PaginatedResponse,
    TransactionFilters, TransactionIdPath, TransactionResponse, UpdateTransactionDto,
};
use super::service::TransactionService;

/// GET /transactions - List transactions with optional filters
#[get("/transactions")]
pub async fn list_transactions(
    pool: web::Data<PgPool>,
    auth: AuthenticatedUser,
    query: web::Query<TransactionFilters>,
) -> Result<HttpResponse, AppError> {
    query
        .validate()
        .map_err(|e| AppError::ValidationError(e.to_string()))?;

    let (transactions, total) =
        TransactionService::list_transactions(pool.get_ref(), auth.user_id, &query).await?;

    let response: Vec<TransactionResponse> = transactions.into_iter().map(Into::into).collect();

    Ok(HttpResponse::Ok().json(PaginatedResponse {
        data: response,
        total,
        limit: query.limit,
        offset: query.offset,
    }))
}

/// GET /transactions/category/{category_id} - Get all transactions for a category
#[get("/transactions/category/{category_id}")]
pub async fn get_by_category(
    pool: web::Data<PgPool>,
    auth: AuthenticatedUser,
    path: web::Path<CategoryIdPath>,
) -> Result<HttpResponse, AppError> {
    let transactions =
        TransactionService::get_by_category(pool.get_ref(), auth.user_id, path.category_id).await?;

    let response: Vec<TransactionResponse> = transactions.into_iter().map(Into::into).collect();

    Ok(HttpResponse::Ok().json(response))
}

/// POST /transactions/categories - Get transactions for multiple categories
#[post("/transactions/categories")]
pub async fn get_by_categories(
    pool: web::Data<PgPool>,
    auth: AuthenticatedUser,
    body: web::Json<CategoriesQueryDto>,
) -> Result<HttpResponse, AppError> {
    let transactions = TransactionService::get_by_categories(
        pool.get_ref(),
        auth.user_id,
        body.into_inner().category_ids,
    )
    .await?;

    let response: Vec<TransactionResponse> = transactions.into_iter().map(Into::into).collect();

    Ok(HttpResponse::Ok().json(response))
}

/// GET /transactions/{id} - Get a specific transaction by ID
#[get("/transactions/{id}")]
pub async fn get_transaction(
    pool: web::Data<PgPool>,
    auth: AuthenticatedUser,
    path: web::Path<TransactionIdPath>,
) -> Result<HttpResponse, AppError> {
    let transaction =
        TransactionService::get_transaction(pool.get_ref(), auth.user_id, path.id).await?;

    Ok(HttpResponse::Ok().json(TransactionResponse::from(transaction)))
}

/// POST /transactions - Create a new transaction (atomically updates account balance)
#[post("/transactions")]
pub async fn create_transaction(
    pool: web::Data<PgPool>,
    auth: AuthenticatedUser,
    body: web::Json<CreateTransactionDto>,
) -> Result<HttpResponse, AppError> {
    body.validate()
        .map_err(|e| AppError::ValidationError(e.to_string()))?;

    let transaction =
        TransactionService::create_transaction(pool.get_ref(), auth.user_id, body.into_inner())
            .await?;

    Ok(HttpResponse::Created().json(TransactionResponse::from(transaction)))
}

/// PATCH /transactions/{id} - Update a transaction (handles balance adjustments atomically)
#[patch("/transactions/{id}")]
pub async fn update_transaction(
    pool: web::Data<PgPool>,
    auth: AuthenticatedUser,
    path: web::Path<TransactionIdPath>,
    body: web::Json<UpdateTransactionDto>,
) -> Result<HttpResponse, AppError> {
    body.validate()
        .map_err(|e| AppError::ValidationError(e.to_string()))?;
    body.validate_amount()
        .map_err(|e| AppError::ValidationError(e.to_string()))?;

    let transaction = TransactionService::update_transaction(
        pool.get_ref(),
        auth.user_id,
        path.id,
        body.into_inner(),
    )
    .await?;

    Ok(HttpResponse::Ok().json(TransactionResponse::from(transaction)))
}

/// DELETE /transactions/{id} - Delete a transaction (atomically restores account balance)
#[delete("/transactions/{id}")]
pub async fn delete_transaction(
    pool: web::Data<PgPool>,
    auth: AuthenticatedUser,
    path: web::Path<TransactionIdPath>,
) -> Result<HttpResponse, AppError> {
    TransactionService::delete_transaction(pool.get_ref(), auth.user_id, path.id).await?;

    Ok(HttpResponse::NoContent().finish())
}
