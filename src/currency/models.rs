#![allow(dead_code)]

use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::collections::HashMap;
use utoipa::ToSchema;

/// Database entity for currencies
#[derive(Debug, Clone, FromRow)]
pub struct Currency {
    pub code: String,
    pub name: String,
    pub symbol: String,
    pub decimal_places: i16,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}

/// Currency information returned in responses
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CurrencyResponse {
    /// ISO 4217 currency code (e.g., "USD", "EUR")
    #[schema(example = "USD")]
    pub code: String,
    /// Full currency name
    #[schema(example = "United States Dollar")]
    pub name: String,
    /// Currency symbol
    #[schema(example = "$")]
    pub symbol: String,
    /// Number of decimal places
    #[schema(example = 2)]
    pub decimal_places: i16,
    /// Whether the currency is active
    #[schema(example = true)]
    pub is_active: bool,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
}

impl CurrencyResponse {
    pub fn from_currency(currency: Currency) -> Self {
        Self {
            code: currency.code,
            name: currency.name,
            symbol: currency.symbol,
            decimal_places: currency.decimal_places,
            is_active: currency.is_active,
            created_at: currency.created_at,
        }
    }
}

/// Response for listing currencies
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CurrenciesListResponse {
    /// List of currencies
    pub currencies: Vec<CurrencyResponse>,
    /// Total count
    #[schema(example = 10)]
    pub count: usize,
}

/// Database entity for exchange rates
#[derive(Debug, Clone, FromRow)]
pub struct ExchangeRate {
    pub id: i64,
    pub base_currency: String,
    pub target_currency: String,
    pub rate: Decimal,
    pub rate_date: NaiveDate,
    pub created_at: DateTime<Utc>,
}

/// Exchange rate information returned in responses
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExchangeRateResponse {
    /// Base currency code
    #[schema(example = "USD")]
    pub base_currency: String,
    /// Target currency code
    #[schema(example = "EUR")]
    pub target_currency: String,
    /// Exchange rate
    #[schema(example = 0.92)]
    pub rate: Decimal,
    /// Date of the exchange rate
    pub rate_date: NaiveDate,
}

impl ExchangeRateResponse {
    pub fn from_exchange_rate(rate: ExchangeRate) -> Self {
        Self {
            base_currency: rate.base_currency,
            target_currency: rate.target_currency,
            rate: rate.rate,
            rate_date: rate.rate_date,
        }
    }
}

/// Response for exchange rate sync operation
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SyncRatesResponse {
    /// Success message
    #[schema(example = "Exchange rates synchronized successfully")]
    pub message: String,
    /// Number of rates updated
    #[schema(example = 150)]
    pub rates_updated: usize,
}

/// Open Exchange Rates API response structure
#[derive(Debug, Deserialize)]
pub struct OxrApiResponse {
    /// API disclaimer
    pub disclaimer: String,
    /// License information
    pub license: String,
    /// Unix timestamp of the rates
    pub timestamp: i64,
    /// Base currency (usually USD for free tier)
    pub base: String,
    /// Exchange rates as currency code -> rate mapping
    pub rates: HashMap<String, Decimal>,
}
