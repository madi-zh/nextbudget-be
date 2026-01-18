use actix_web::{delete, get, patch, post, web, HttpResponse};
use sqlx::PgPool;
use validator::Validate;

use crate::errors::{AppError, ErrorResponse};
use crate::extractors::AuthenticatedUser;

use super::models::{
    BudgetIdPath, CategoryIdPath, CategoryResponse, CreateCategoryDto, UpdateCategoryDto,
};
use super::service::CategoryService;

/// GET /categories - List all categories for the authenticated user
#[utoipa::path(
    get,
    path = "/categories",
    tag = "Categories",
    responses(
        (status = 200, description = "List of categories", body = Vec<CategoryResponse>),
        (status = 401, description = "Unauthorized", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
#[get("/categories")]
pub async fn list_categories(
    pool: web::Data<PgPool>,
    auth: AuthenticatedUser,
) -> Result<HttpResponse, AppError> {
    let categories = CategoryService::get_all_for_user(pool.get_ref(), auth.user_id).await?;

    let response: Vec<CategoryResponse> = categories
        .into_iter()
        .map(CategoryResponse::from_category_with_spent)
        .collect();

    Ok(HttpResponse::Ok().json(response))
}

/// GET /categories/budget/{budget_id} - Get all categories for a budget
#[utoipa::path(
    get,
    path = "/categories/budget/{budget_id}",
    tag = "Categories",
    params(BudgetIdPath),
    responses(
        (status = 200, description = "List of categories for budget", body = Vec<CategoryResponse>),
        (status = 404, description = "Budget not found", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
#[get("/categories/budget/{budget_id}")]
pub async fn get_categories_by_budget(
    pool: web::Data<PgPool>,
    auth: AuthenticatedUser,
    path: web::Path<BudgetIdPath>,
) -> Result<HttpResponse, AppError> {
    let categories =
        CategoryService::get_by_budget_id(pool.get_ref(), path.budget_id, auth.user_id).await?;

    let response: Vec<CategoryResponse> = categories
        .into_iter()
        .map(CategoryResponse::from_category_with_spent)
        .collect();

    Ok(HttpResponse::Ok().json(response))
}

/// GET /categories/{id} - Get a specific category
#[utoipa::path(
    get,
    path = "/categories/{id}",
    tag = "Categories",
    params(CategoryIdPath),
    responses(
        (status = 200, description = "Category details", body = CategoryResponse),
        (status = 404, description = "Category not found", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
#[get("/categories/{id}")]
pub async fn get_category(
    pool: web::Data<PgPool>,
    auth: AuthenticatedUser,
    path: web::Path<CategoryIdPath>,
) -> Result<HttpResponse, AppError> {
    let category = CategoryService::get_by_id(pool.get_ref(), path.id, auth.user_id).await?;

    Ok(HttpResponse::Ok().json(CategoryResponse::from_category_with_spent(category)))
}

/// POST /categories - Create a new category
#[utoipa::path(
    post,
    path = "/categories",
    tag = "Categories",
    request_body = CreateCategoryDto,
    responses(
        (status = 201, description = "Category created", body = CategoryResponse),
        (status = 400, description = "Validation error", body = ErrorResponse),
        (status = 404, description = "Budget not found", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
#[post("/categories")]
pub async fn create_category(
    pool: web::Data<PgPool>,
    auth: AuthenticatedUser,
    body: web::Json<CreateCategoryDto>,
) -> Result<HttpResponse, AppError> {
    body.validate()
        .map_err(|e| AppError::ValidationError(e.to_string()))?;
    body.validate_decimals()
        .map_err(|e| AppError::ValidationError(e.to_string()))?;

    let category = CategoryService::create(pool.get_ref(), &body, auth.user_id).await?;

    Ok(HttpResponse::Created().json(CategoryResponse::from_category(category)))
}

/// PATCH /categories/{id} - Update a category
#[utoipa::path(
    patch,
    path = "/categories/{id}",
    tag = "Categories",
    params(CategoryIdPath),
    request_body = UpdateCategoryDto,
    responses(
        (status = 200, description = "Category updated", body = CategoryResponse),
        (status = 400, description = "Validation error", body = ErrorResponse),
        (status = 404, description = "Category not found", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
#[patch("/categories/{id}")]
pub async fn update_category(
    pool: web::Data<PgPool>,
    auth: AuthenticatedUser,
    path: web::Path<CategoryIdPath>,
    body: web::Json<UpdateCategoryDto>,
) -> Result<HttpResponse, AppError> {
    body.validate()
        .map_err(|e| AppError::ValidationError(e.to_string()))?;
    body.validate_fields()
        .map_err(|e| AppError::ValidationError(e.to_string()))?;

    let category = CategoryService::update(pool.get_ref(), path.id, &body, auth.user_id).await?;

    Ok(HttpResponse::Ok().json(CategoryResponse::from_category(category)))
}

/// DELETE /categories/{id} - Delete a category
#[utoipa::path(
    delete,
    path = "/categories/{id}",
    tag = "Categories",
    params(CategoryIdPath),
    responses(
        (status = 204, description = "Category deleted"),
        (status = 404, description = "Category not found", body = ErrorResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
#[delete("/categories/{id}")]
pub async fn delete_category(
    pool: web::Data<PgPool>,
    auth: AuthenticatedUser,
    path: web::Path<CategoryIdPath>,
) -> Result<HttpResponse, AppError> {
    CategoryService::delete(pool.get_ref(), path.id, auth.user_id).await?;

    Ok(HttpResponse::NoContent().finish())
}
