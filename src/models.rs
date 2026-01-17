use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;
use validator::{Validate, ValidationError};

/// Validate password complexity: at least one uppercase, one lowercase, and one digit
fn validate_password_complexity(password: &str) -> Result<(), ValidationError> {
    let has_lowercase = password.chars().any(|c| c.is_ascii_lowercase());
    let has_uppercase = password.chars().any(|c| c.is_ascii_uppercase());
    let has_digit = password.chars().any(|c| c.is_ascii_digit());

    if has_lowercase && has_uppercase && has_digit {
        Ok(())
    } else {
        Err(ValidationError::new("password_complexity"))
    }
}

// ============================================================================
// User Models
// ============================================================================

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    #[serde(skip_serializing)]
    pub password_hash: String,
    pub full_name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateUserDto {
    #[validate(email)]
    pub email: String,
    #[validate(length(min = 8, message = "Password must be at least 8 characters"))]
    #[validate(custom(function = "validate_password_complexity", message = "Password must contain at least one uppercase letter, one lowercase letter, and one number"))]
    pub password: String,
    #[validate(length(max = 100, message = "Full name must be at most 100 characters"))]
    pub full_name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct UserResponseDto {
    pub id: Uuid,
    pub email: String,
    pub full_name: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl UserResponseDto {
    pub fn from_user(user: &User) -> Self {
        Self {
            id: user.id,
            email: user.email.clone(),
            full_name: user.full_name.clone(),
            created_at: user.created_at,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct LoginDto {
    pub email: String,
    pub password: String,
}

// ============================================================================
// Token Models
// ============================================================================

/// JWT access token claims - short-lived (15 minutes)
#[derive(Debug, Serialize, Deserialize)]
pub struct TokenClaims {
    pub sub: Uuid,            // User ID
    pub email: String,        // User email
    pub name: Option<String>, // User display name
    pub iat: usize,           // Issued at
    pub exp: usize,           // Expiration
}

/// Refresh token stored in database
#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct RefreshToken {
    pub id: Uuid,
    pub user_id: Uuid,
    pub token_hash: String,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

/// Request to refresh tokens
#[derive(Debug, Deserialize)]
pub struct RefreshTokenDto {
    pub refresh_token: String,
}

/// Response containing both access and refresh tokens
#[derive(Debug, Serialize)]
pub struct AuthTokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: &'static str,
    pub expires_in: u64, // Access token expiry in seconds
    pub user: UserResponseDto,
}

impl AuthTokenResponse {
    pub fn new(access_token: String, refresh_token: String, user: &User) -> Self {
        Self {
            access_token,
            refresh_token,
            token_type: "Bearer",
            expires_in: 15 * 60, // 15 minutes
            user: UserResponseDto::from_user(user),
        }
    }
}
