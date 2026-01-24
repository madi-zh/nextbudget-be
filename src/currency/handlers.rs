use actix_web::{get, post, web, HttpResponse};
use sqlx::PgPool;
use std::env;

use crate::errors::{AppError, ErrorResponse};
use crate::extractors::AuthenticatedUser;

use super::models::{CurrenciesListResponse, CurrencyResponse, SyncRatesResponse};
use super::service::CurrencyService;

/// GET /currencies - List all active currencies
#[utoipa::path(
    get,
    path = "/currencies",
    tag = "Currencies",
    responses(
        (status = 200, description = "List of active currencies", body = CurrenciesListResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
#[get("/currencies")]
pub async fn list_currencies(pool: web::Data<PgPool>) -> Result<HttpResponse, AppError> {
    let currencies = CurrencyService::list_currencies(pool.get_ref()).await?;

    let response = CurrenciesListResponse {
        count: currencies.len(),
        currencies: currencies
            .into_iter()
            .map(CurrencyResponse::from_currency)
            .collect(),
    };

    Ok(HttpResponse::Ok().json(response))
}

/// POST /currencies/sync-rates - Trigger exchange rate synchronization
#[utoipa::path(
    post,
    path = "/currencies/sync-rates",
    tag = "Currencies",
    responses(
        (status = 200, description = "Exchange rates synchronized", body = SyncRatesResponse),
        (status = 401, description = "Unauthorized", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    security(("bearer_auth" = []))
)]
#[post("/currencies/sync-rates")]
pub async fn sync_exchange_rates(
    pool: web::Data<PgPool>,
    _auth: AuthenticatedUser,
) -> Result<HttpResponse, AppError> {
    let api_key = env::var("OPENEXCHANGERATES_API_KEY").map_err(|_| {
        AppError::InternalError("Open Exchange Rates API key not configured".to_string())
    })?;

    let rates_updated = CurrencyService::fetch_and_store_rates(pool.get_ref(), &api_key).await?;

    Ok(HttpResponse::Ok().json(SyncRatesResponse {
        message: "Exchange rates synchronized successfully".to_string(),
        rates_updated,
    }))
}
