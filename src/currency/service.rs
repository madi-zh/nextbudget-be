#![allow(dead_code)]

use chrono::{NaiveDate, Utc};
use sqlx::PgPool;

use super::models::{Currency, ExchangeRate, OxrApiResponse};
use crate::errors::AppError;

/// Service layer for currency business logic.
pub struct CurrencyService;

impl CurrencyService {
    /// List all active currencies.
    pub async fn list_currencies(pool: &PgPool) -> Result<Vec<Currency>, AppError> {
        sqlx::query_as::<_, Currency>(
            r#"
            SELECT code, name, symbol, decimal_places, is_active, created_at
            FROM currencies
            WHERE is_active = true
            ORDER BY code ASC
            "#,
        )
        .fetch_all(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))
    }

    /// Get a specific currency by code.
    pub async fn get_currency(pool: &PgPool, code: &str) -> Result<Currency, AppError> {
        let code_upper = code.to_uppercase();

        sqlx::query_as::<_, Currency>(
            r#"
            SELECT code, name, symbol, decimal_places, is_active, created_at
            FROM currencies
            WHERE code = $1
            "#,
        )
        .bind(&code_upper)
        .fetch_optional(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?
        .ok_or_else(|| AppError::NotFound(format!("Currency '{}' not found", code_upper)))
    }

    /// Validate that a currency code exists and is active.
    pub async fn validate_currency(pool: &PgPool, code: &str) -> Result<bool, AppError> {
        let code_upper = code.to_uppercase();

        let result = sqlx::query_scalar::<_, bool>(
            r#"
            SELECT EXISTS(
                SELECT 1 FROM currencies
                WHERE code = $1 AND is_active = true
            )
            "#,
        )
        .bind(&code_upper)
        .fetch_one(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?;

        Ok(result)
    }

    /// Get exchange rate between two currencies for a specific date.
    pub async fn get_exchange_rate(
        pool: &PgPool,
        base: &str,
        target: &str,
        date: NaiveDate,
    ) -> Result<ExchangeRate, AppError> {
        let base_upper = base.to_uppercase();
        let target_upper = target.to_uppercase();

        // Try to find the exact date first, then fall back to the most recent rate
        sqlx::query_as::<_, ExchangeRate>(
            r#"
            SELECT id, base_currency, target_currency, rate, rate_date, created_at
            FROM exchange_rates
            WHERE base_currency = $1 AND target_currency = $2 AND rate_date <= $3
            ORDER BY rate_date DESC
            LIMIT 1
            "#,
        )
        .bind(&base_upper)
        .bind(&target_upper)
        .bind(date)
        .fetch_optional(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "Exchange rate for {}/{} not found",
                base_upper, target_upper
            ))
        })
    }

    /// Fetch exchange rates from Open Exchange Rates API and store them.
    pub async fn fetch_and_store_rates(pool: &PgPool, api_key: &str) -> Result<usize, AppError> {
        let url = format!(
            "https://openexchangerates.org/api/latest.json?app_id={}",
            api_key
        );

        // Fetch rates from the API
        let client = reqwest::Client::new();
        let response = client.get(&url).send().await.map_err(|e| {
            AppError::InternalError(format!("Failed to fetch exchange rates: {}", e))
        })?;

        if !response.status().is_success() {
            return Err(AppError::InternalError(format!(
                "Exchange rates API returned status: {}",
                response.status()
            )));
        }

        let oxr_response: OxrApiResponse = response.json().await.map_err(|e| {
            AppError::InternalError(format!("Failed to parse exchange rates: {}", e))
        })?;

        let base_currency = oxr_response.base;
        let rate_date = Utc::now().date_naive();
        let mut rates_updated: usize = 0;

        // Store each rate in the database using upsert
        for (target_currency, rate) in oxr_response.rates {
            let result = sqlx::query(
                r#"
                INSERT INTO exchange_rates (base_currency, target_currency, rate, rate_date)
                VALUES ($1, $2, $3, $4)
                ON CONFLICT (base_currency, target_currency, rate_date)
                DO UPDATE SET rate = EXCLUDED.rate, created_at = NOW()
                "#,
            )
            .bind(&base_currency)
            .bind(&target_currency)
            .bind(rate)
            .bind(rate_date)
            .execute(pool)
            .await;

            if result.is_ok() {
                rates_updated += 1;
            }
        }

        Ok(rates_updated)
    }
}
