use actix_web::{HttpResponse, ResponseError};
use serde::Serialize;
use std::fmt;
use tracing::error;
use utoipa::ToSchema;

#[derive(Debug)]
pub enum AppError {
    ValidationError(String),
    Unauthorized(String),
    NotFound(String),
    Conflict(String),
    InternalError(String),
}

/// Standard error response format
#[derive(Serialize, ToSchema)]
pub struct ErrorResponse {
    /// Error type code (e.g., "VALIDATION_ERROR", "NOT_FOUND")
    #[schema(example = "VALIDATION_ERROR")]
    pub error: String,
    /// Human-readable error message
    #[schema(example = "Invalid input provided")]
    pub message: String,
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::ValidationError(msg) => write!(f, "Validation error: {msg}"),
            AppError::Unauthorized(msg) => write!(f, "Unauthorized: {msg}"),
            AppError::NotFound(msg) => write!(f, "Not found: {msg}"),
            AppError::Conflict(msg) => write!(f, "Conflict: {msg}"),
            AppError::InternalError(msg) => write!(f, "Internal error: {msg}"),
        }
    }
}

impl ResponseError for AppError {
    fn error_response(&self) -> HttpResponse {
        let (status, error_type, message) = match self {
            AppError::ValidationError(msg) => (
                actix_web::http::StatusCode::BAD_REQUEST,
                "VALIDATION_ERROR",
                msg.clone(),
            ),
            AppError::Unauthorized(msg) => (
                actix_web::http::StatusCode::UNAUTHORIZED,
                "UNAUTHORIZED",
                msg.clone(),
            ),
            AppError::NotFound(msg) => (
                actix_web::http::StatusCode::NOT_FOUND,
                "NOT_FOUND",
                msg.clone(),
            ),
            AppError::Conflict(msg) => (
                actix_web::http::StatusCode::CONFLICT,
                "CONFLICT",
                msg.clone(),
            ),
            AppError::InternalError(msg) => {
                // Log the actual error for debugging, but don't expose to client
                error!("Internal error: {msg}");
                (
                    actix_web::http::StatusCode::INTERNAL_SERVER_ERROR,
                    "INTERNAL_ERROR",
                    "An internal error occurred".to_string(),
                )
            }
        };

        HttpResponse::build(status).json(ErrorResponse {
            error: error_type.to_string(),
            message,
        })
    }
}

// Convenience conversion from sqlx::Error
impl From<sqlx::Error> for AppError {
    fn from(err: sqlx::Error) -> Self {
        match err {
            sqlx::Error::RowNotFound => AppError::NotFound("Resource not found".to_string()),
            _ => AppError::InternalError(err.to_string()),
        }
    }
}
