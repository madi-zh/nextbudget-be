use actix_web::{delete, get, patch, post, web, HttpResponse};
use sqlx::PgPool;
use validator::Validate;

use crate::errors::{AppError, ErrorResponse};
use crate::extractors::AuthenticatedUser;

use super::models::{
    BudgetIdPath, BudgetResponse, CreateBudgetDto, ListBudgetsQuery, MonthYearPath,
    UpdateBudgetDto, UpdateIncomeDto, UpdateSavingsRateDto,
};
use super::service::BudgetService;

/// GET /budgets - List all budgets for the authenticated user
#[utoipa::path(
    get,
    path = "/budgets",
    tag = "Budgets",
    params(ListBudgetsQuery),
    responses(
        (status = 200, description = "List of budgets", body = Vec<BudgetResponse>),
        (status = 401, description = "Unauthorized", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
#[get("/budgets")]
pub async fn list_budgets(
    pool: web::Data<PgPool>,
    auth: AuthenticatedUser,
    query: web::Query<ListBudgetsQuery>,
) -> Result<HttpResponse, AppError> {
    query
        .validate()
        .map_err(|e| AppError::ValidationError(e.to_string()))?;

    let budgets = BudgetService::list_budgets(pool.get_ref(), auth.user_id, &query).await?;

    let response: Vec<BudgetResponse> = budgets
        .into_iter()
        .map(BudgetResponse::from_budget)
        .collect();

    Ok(HttpResponse::Ok().json(response))
}

/// GET /budgets/{id} - Get a specific budget by ID
#[utoipa::path(
    get,
    path = "/budgets/{id}",
    tag = "Budgets",
    params(BudgetIdPath),
    responses(
        (status = 200, description = "Budget details", body = BudgetResponse),
        (status = 404, description = "Budget not found", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
#[get("/budgets/{id}")]
pub async fn get_budget(
    pool: web::Data<PgPool>,
    auth: AuthenticatedUser,
    path: web::Path<BudgetIdPath>,
) -> Result<HttpResponse, AppError> {
    let budget = BudgetService::get_budget_by_id(pool.get_ref(), path.id, auth.user_id).await?;

    Ok(HttpResponse::Ok().json(BudgetResponse::from_budget(budget)))
}

/// GET /budgets/month/{month}/year/{year} - Get budget for specific month/year
#[utoipa::path(
    get,
    path = "/budgets/month/{month}/year/{year}",
    tag = "Budgets",
    params(MonthYearPath),
    responses(
        (status = 200, description = "Budget details", body = BudgetResponse),
        (status = 404, description = "Budget not found", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
#[get("/budgets/month/{month}/year/{year}")]
pub async fn get_budget_by_month_year(
    pool: web::Data<PgPool>,
    auth: AuthenticatedUser,
    path: web::Path<MonthYearPath>,
) -> Result<HttpResponse, AppError> {
    path.validate()
        .map_err(|e| AppError::ValidationError(e.to_string()))?;

    let budget = BudgetService::get_budget_by_month_year(
        pool.get_ref(),
        auth.user_id,
        path.month,
        path.year,
    )
    .await?;

    Ok(HttpResponse::Ok().json(BudgetResponse::from_budget(budget)))
}

/// POST /budgets - Create a new budget
#[utoipa::path(
    post,
    path = "/budgets",
    tag = "Budgets",
    request_body = CreateBudgetDto,
    responses(
        (status = 201, description = "Budget created", body = BudgetResponse),
        (status = 400, description = "Validation error", body = ErrorResponse),
        (status = 409, description = "Budget already exists for this month/year", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
#[post("/budgets")]
pub async fn create_budget(
    pool: web::Data<PgPool>,
    auth: AuthenticatedUser,
    body: web::Json<CreateBudgetDto>,
) -> Result<HttpResponse, AppError> {
    body.validate()
        .map_err(|e| AppError::ValidationError(e.to_string()))?;
    body.validate_decimals()
        .map_err(|e| AppError::ValidationError(e.to_string()))?;

    let budget = BudgetService::create_budget(pool.get_ref(), auth.user_id, &body).await?;

    Ok(HttpResponse::Created().json(BudgetResponse::from_budget(budget)))
}

/// PATCH /budgets/{id} - Update a budget (partial update)
#[utoipa::path(
    patch,
    path = "/budgets/{id}",
    tag = "Budgets",
    params(BudgetIdPath),
    request_body = UpdateBudgetDto,
    responses(
        (status = 200, description = "Budget updated", body = BudgetResponse),
        (status = 400, description = "Validation error", body = ErrorResponse),
        (status = 404, description = "Budget not found", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
#[patch("/budgets/{id}")]
pub async fn update_budget(
    pool: web::Data<PgPool>,
    auth: AuthenticatedUser,
    path: web::Path<BudgetIdPath>,
    body: web::Json<UpdateBudgetDto>,
) -> Result<HttpResponse, AppError> {
    body.validate()
        .map_err(|e| AppError::ValidationError(e.to_string()))?;
    body.validate_decimals()
        .map_err(|e| AppError::ValidationError(e.to_string()))?;

    let budget = BudgetService::update_budget(pool.get_ref(), path.id, auth.user_id, &body).await?;

    Ok(HttpResponse::Ok().json(BudgetResponse::from_budget(budget)))
}

/// PATCH /budgets/{id}/income - Update income only
#[utoipa::path(
    patch,
    path = "/budgets/{id}/income",
    tag = "Budgets",
    params(BudgetIdPath),
    request_body = UpdateIncomeDto,
    responses(
        (status = 200, description = "Income updated", body = BudgetResponse),
        (status = 400, description = "Validation error", body = ErrorResponse),
        (status = 404, description = "Budget not found", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
#[patch("/budgets/{id}/income")]
pub async fn update_income(
    pool: web::Data<PgPool>,
    auth: AuthenticatedUser,
    path: web::Path<BudgetIdPath>,
    body: web::Json<UpdateIncomeDto>,
) -> Result<HttpResponse, AppError> {
    body.validate()
        .map_err(|e| AppError::ValidationError(e.to_string()))?;

    let budget = BudgetService::update_income(pool.get_ref(), path.id, auth.user_id, &body).await?;

    Ok(HttpResponse::Ok().json(BudgetResponse::from_budget(budget)))
}

/// PATCH /budgets/{id}/savings-rate - Update savings rate only
#[utoipa::path(
    patch,
    path = "/budgets/{id}/savings-rate",
    tag = "Budgets",
    params(BudgetIdPath),
    request_body = UpdateSavingsRateDto,
    responses(
        (status = 200, description = "Savings rate updated", body = BudgetResponse),
        (status = 400, description = "Validation error", body = ErrorResponse),
        (status = 404, description = "Budget not found", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
#[patch("/budgets/{id}/savings-rate")]
pub async fn update_savings_rate(
    pool: web::Data<PgPool>,
    auth: AuthenticatedUser,
    path: web::Path<BudgetIdPath>,
    body: web::Json<UpdateSavingsRateDto>,
) -> Result<HttpResponse, AppError> {
    body.validate()
        .map_err(|e| AppError::ValidationError(e.to_string()))?;

    let budget =
        BudgetService::update_savings_rate(pool.get_ref(), path.id, auth.user_id, &body).await?;

    Ok(HttpResponse::Ok().json(BudgetResponse::from_budget(budget)))
}

/// DELETE /budgets/{id} - Delete a budget
#[utoipa::path(
    delete,
    path = "/budgets/{id}",
    tag = "Budgets",
    params(BudgetIdPath),
    responses(
        (status = 204, description = "Budget deleted"),
        (status = 404, description = "Budget not found", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
#[delete("/budgets/{id}")]
pub async fn delete_budget(
    pool: web::Data<PgPool>,
    auth: AuthenticatedUser,
    path: web::Path<BudgetIdPath>,
) -> Result<HttpResponse, AppError> {
    BudgetService::delete_budget(pool.get_ref(), path.id, auth.user_id).await?;

    Ok(HttpResponse::NoContent().finish())
}
