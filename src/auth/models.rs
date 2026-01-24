use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;
use uuid::Uuid;
use validator::{Validate, ValidationError};

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
    pub default_currency: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

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

/// Request body for user registration
#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct CreateUserDto {
    /// User's email address
    #[validate(email)]
    #[schema(example = "user@example.com")]
    pub email: String,
    /// Password (min 8 chars, must include uppercase, lowercase, and digit)
    #[validate(length(min = 8, message = "Password must be at least 8 characters"))]
    #[validate(custom(
        function = "validate_password_complexity",
        message = "Password must contain at least one uppercase letter, one lowercase letter, and one number"
    ))]
    #[schema(example = "Password123")]
    pub password: String,
    /// Optional full name
    #[validate(length(max = 100, message = "Full name must be at most 100 characters"))]
    #[schema(example = "John Doe")]
    pub full_name: Option<String>,
}

/// User information returned in responses
#[derive(Debug, Serialize, ToSchema)]
pub struct UserResponseDto {
    /// Unique user identifier
    pub id: Uuid,
    /// User's email address
    #[schema(example = "user@example.com")]
    pub email: String,
    /// User's full name
    #[schema(example = "John Doe")]
    pub full_name: Option<String>,
    /// User's default currency code
    #[schema(example = "USD")]
    pub default_currency: String,
    /// Account creation timestamp
    pub created_at: DateTime<Utc>,
}

impl UserResponseDto {
    pub fn from_user(user: &User) -> Self {
        Self {
            id: user.id,
            email: user.email.clone(),
            full_name: user.full_name.clone(),
            default_currency: user.default_currency.clone(),
            created_at: user.created_at,
        }
    }
}

/// Request body for user login
#[derive(Debug, Deserialize, ToSchema)]
pub struct LoginDto {
    /// User's email address
    #[schema(example = "user@example.com")]
    pub email: String,
    /// User's password
    #[schema(example = "Password123")]
    pub password: String,
}

/// Request body for Google OAuth login
#[derive(Debug, Deserialize, ToSchema)]
pub struct GoogleLoginDto {
    /// Google ID token from Google Sign-In
    #[schema(example = "eyJhbGciOiJSUzI1NiIsInR5cCI6...")]
    pub id_token: String,
}

/// Google token verification response structure
#[derive(Debug, Deserialize)]
pub struct GoogleTokenInfo {
    /// Google user ID (subject)
    pub sub: String,
    /// User's email address
    pub email: String,
    /// Whether the email has been verified
    pub email_verified: String,
    /// User's full name
    pub name: Option<String>,
    /// URL to user's profile picture
    pub picture: Option<String>,
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

/// Request body to refresh access token
#[derive(Debug, Deserialize, ToSchema)]
pub struct RefreshTokenDto {
    /// The refresh token obtained from login
    #[schema(example = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9...")]
    pub refresh_token: String,
}

/// Response containing both access and refresh tokens
#[derive(Debug, Serialize, ToSchema)]
pub struct AuthTokenResponse {
    /// JWT access token (short-lived, 15 minutes)
    #[schema(example = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9...")]
    pub access_token: String,
    /// Refresh token for obtaining new access tokens
    #[schema(example = "a1b2c3d4e5f6...")]
    pub refresh_token: String,
    /// Token type (always "Bearer")
    #[schema(example = "Bearer")]
    pub token_type: &'static str,
    /// Access token expiry time in seconds
    #[schema(example = 900)]
    pub expires_in: u64,
    /// User information
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
