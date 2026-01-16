use actix_web::{delete, get, patch, post, web, HttpResponse};
use sqlx::PgPool;
use validator::Validate;

use crate::errors::AppError;
use crate::extractors::AuthenticatedUser;

use super::models::{
    BudgetIdPath, BudgetResponse, CreateBudgetDto, ListBudgetsQuery, MonthYearPath,
    UpdateBudgetDto, UpdateIncomeDto, UpdateSavingsRateDto,
};
use super::service::BudgetService;

/// GET /budgets - List all budgets for the authenticated user
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
#[post("/budgets")]
pub async fn create_budget(
    pool: web::Data<PgPool>,
    auth: AuthenticatedUser,
    body: web::Json<CreateBudgetDto>,
) -> Result<HttpResponse, AppError> {
    body.validate()
        .map_err(|e| AppError::ValidationError(e.to_string()))?;

    let budget = BudgetService::create_budget(pool.get_ref(), auth.user_id, &body).await?;

    Ok(HttpResponse::Created().json(BudgetResponse::from_budget(budget)))
}

/// PATCH /budgets/{id} - Update a budget (partial update)
#[patch("/budgets/{id}")]
pub async fn update_budget(
    pool: web::Data<PgPool>,
    auth: AuthenticatedUser,
    path: web::Path<BudgetIdPath>,
    body: web::Json<UpdateBudgetDto>,
) -> Result<HttpResponse, AppError> {
    body.validate()
        .map_err(|e| AppError::ValidationError(e.to_string()))?;

    let budget = BudgetService::update_budget(pool.get_ref(), path.id, auth.user_id, &body).await?;

    Ok(HttpResponse::Ok().json(BudgetResponse::from_budget(budget)))
}

/// PATCH /budgets/{id}/income - Update income only
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
#[delete("/budgets/{id}")]
pub async fn delete_budget(
    pool: web::Data<PgPool>,
    auth: AuthenticatedUser,
    path: web::Path<BudgetIdPath>,
) -> Result<HttpResponse, AppError> {
    BudgetService::delete_budget(pool.get_ref(), path.id, auth.user_id).await?;

    Ok(HttpResponse::NoContent().finish())
}
