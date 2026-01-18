use actix_web::{delete, get, patch, post, web, HttpResponse};
use sqlx::PgPool;
use validator::Validate;

use crate::errors::{AppError, ErrorResponse};
use crate::extractors::AuthenticatedUser;

use super::models::{
    CategoriesQueryDto, CategoryIdPath, CreateTransactionDto, PaginatedTransactionResponse,
    TransactionFilters, TransactionIdPath, TransactionResponse, UpdateTransactionDto,
};
use super::service::TransactionService;

/// GET /transactions - List transactions with optional filters
#[utoipa::path(
    get,
    path = "/transactions",
    tag = "Transactions",
    params(TransactionFilters),
    responses(
        (status = 200, description = "Paginated list of transactions", body = PaginatedTransactionResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
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

    Ok(HttpResponse::Ok().json(PaginatedTransactionResponse {
        data: response,
        total,
        limit: query.limit,
        offset: query.offset,
    }))
}

/// GET /transactions/category/{category_id} - Get all transactions for a category
#[utoipa::path(
    get,
    path = "/transactions/category/{category_id}",
    tag = "Transactions",
    params(CategoryIdPath),
    responses(
        (status = 200, description = "List of transactions for category", body = Vec<TransactionResponse>),
        (status = 404, description = "Category not found", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
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
#[utoipa::path(
    post,
    path = "/transactions/categories",
    tag = "Transactions",
    request_body = CategoriesQueryDto,
    responses(
        (status = 200, description = "Transactions for specified categories", body = Vec<TransactionResponse>),
        (status = 401, description = "Unauthorized", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
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
#[utoipa::path(
    get,
    path = "/transactions/{id}",
    tag = "Transactions",
    params(TransactionIdPath),
    responses(
        (status = 200, description = "Transaction details", body = TransactionResponse),
        (status = 404, description = "Transaction not found", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
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
#[utoipa::path(
    post,
    path = "/transactions",
    tag = "Transactions",
    request_body = CreateTransactionDto,
    responses(
        (status = 201, description = "Transaction created", body = TransactionResponse),
        (status = 400, description = "Validation error", body = ErrorResponse),
        (status = 404, description = "Category or account not found", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
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
#[utoipa::path(
    patch,
    path = "/transactions/{id}",
    tag = "Transactions",
    params(TransactionIdPath),
    request_body = UpdateTransactionDto,
    responses(
        (status = 200, description = "Transaction updated", body = TransactionResponse),
        (status = 400, description = "Validation error", body = ErrorResponse),
        (status = 404, description = "Transaction not found", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
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
#[utoipa::path(
    delete,
    path = "/transactions/{id}",
    tag = "Transactions",
    params(TransactionIdPath),
    responses(
        (status = 204, description = "Transaction deleted"),
        (status = 404, description = "Transaction not found", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
#[delete("/transactions/{id}")]
pub async fn delete_transaction(
    pool: web::Data<PgPool>,
    auth: AuthenticatedUser,
    path: web::Path<TransactionIdPath>,
) -> Result<HttpResponse, AppError> {
    TransactionService::delete_transaction(pool.get_ref(), auth.user_id, path.id).await?;

    Ok(HttpResponse::NoContent().finish())
}
