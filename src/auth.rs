use actix_web::{get, post, web, HttpRequest, HttpResponse};
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use rand::Rng;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;
use validator::Validate;

use crate::errors::AppError;
use crate::models::{
    AuthTokenResponse, CreateUserDto, LoginDto, RefreshToken, RefreshTokenDto, TokenClaims, User,
    UserResponseDto,
};

// Token expiration constants
const ACCESS_TOKEN_EXPIRY_MINUTES: i64 = 15;
const REFRESH_TOKEN_EXPIRY_DAYS: i64 = 7;

// ============================================================================
// Password Utilities
// ============================================================================

pub fn hash_password(password: &str) -> Result<String, AppError> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    argon2
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|e| AppError::InternalError(format!("Failed to hash password: {}", e)))
}

pub fn verify_password(password: &str, hash: &str) -> Result<bool, AppError> {
    let parsed_hash = PasswordHash::new(hash)
        .map_err(|e| AppError::InternalError(format!("Invalid password hash: {}", e)))?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok())
}

// ============================================================================
// JWT Access Token Utilities
// ============================================================================

pub fn create_access_token(user: &User, jwt_secret: &str) -> Result<String, AppError> {
    let now = Utc::now();
    let expires_at = now + Duration::minutes(ACCESS_TOKEN_EXPIRY_MINUTES);

    let claims = TokenClaims {
        sub: user.id,
        email: user.email.clone(),
        name: user.full_name.clone(),
        iat: now.timestamp() as usize,
        exp: expires_at.timestamp() as usize,
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(jwt_secret.as_bytes()),
    )
    .map_err(|e| AppError::InternalError(format!("Failed to create access token: {}", e)))
}

pub fn decode_token(token: &str, jwt_secret: &str) -> Result<TokenClaims, AppError> {
    decode::<TokenClaims>(
        token,
        &DecodingKey::from_secret(jwt_secret.as_bytes()),
        &Validation::default(),
    )
    .map(|data| data.claims)
    .map_err(|e| AppError::Unauthorized(format!("Invalid token: {}", e)))
}

pub fn extract_token(req: &HttpRequest) -> Result<String, AppError> {
    req.headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .map(|t| t.to_string())
        .ok_or_else(|| {
            AppError::Unauthorized("Missing or invalid Authorization header".to_string())
        })
}

// ============================================================================
// Refresh Token Utilities
// ============================================================================

/// Generate a random refresh token string
fn generate_refresh_token() -> String {
    let mut rng = rand::thread_rng();
    let bytes: [u8; 32] = rng.gen();
    hex::encode(bytes)
}

/// Hash a refresh token for storage
fn hash_refresh_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

/// Create and store a new refresh token
async fn create_refresh_token(pool: &PgPool, user_id: Uuid) -> Result<String, AppError> {
    let raw_token = generate_refresh_token();
    let token_hash = hash_refresh_token(&raw_token);
    let expires_at = Utc::now() + Duration::days(REFRESH_TOKEN_EXPIRY_DAYS);

    sqlx::query(
        r#"
        INSERT INTO refresh_tokens (user_id, token_hash, expires_at)
        VALUES ($1, $2, $3)
        "#,
    )
    .bind(user_id)
    .bind(&token_hash)
    .bind(expires_at)
    .execute(pool)
    .await
    .map_err(|e| AppError::InternalError(format!("Failed to store refresh token: {}", e)))?;

    Ok(raw_token)
}

/// Validate a refresh token and return the associated token record
async fn validate_refresh_token(pool: &PgPool, raw_token: &str) -> Result<RefreshToken, AppError> {
    let token_hash = hash_refresh_token(raw_token);

    let token = sqlx::query_as::<_, RefreshToken>(
        r#"
        SELECT id, user_id, token_hash, expires_at, created_at, revoked_at
        FROM refresh_tokens
        WHERE token_hash = $1
          AND expires_at > NOW()
          AND revoked_at IS NULL
        "#,
    )
    .bind(&token_hash)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::InternalError(e.to_string()))?
    .ok_or_else(|| AppError::Unauthorized("Invalid or expired refresh token".to_string()))?;

    Ok(token)
}

/// Revoke a specific refresh token
async fn revoke_refresh_token(pool: &PgPool, token_id: Uuid) -> Result<(), AppError> {
    sqlx::query(
        r#"
        UPDATE refresh_tokens
        SET revoked_at = NOW()
        WHERE id = $1
        "#,
    )
    .bind(token_id)
    .execute(pool)
    .await
    .map_err(|e| AppError::InternalError(format!("Failed to revoke token: {}", e)))?;

    Ok(())
}

/// Revoke all refresh tokens for a user (logout from all devices)
async fn revoke_all_user_tokens(pool: &PgPool, user_id: Uuid) -> Result<u64, AppError> {
    let result = sqlx::query(
        r#"
        UPDATE refresh_tokens
        SET revoked_at = NOW()
        WHERE user_id = $1 AND revoked_at IS NULL
        "#,
    )
    .bind(user_id)
    .execute(pool)
    .await
    .map_err(|e| AppError::InternalError(format!("Failed to revoke tokens: {}", e)))?;

    Ok(result.rows_affected())
}

// ============================================================================
// Auth Endpoints
// ============================================================================

#[post("/auth/register")]
pub async fn register(
    pool: web::Data<PgPool>,
    jwt_secret: web::Data<String>,
    body: web::Json<CreateUserDto>,
) -> Result<HttpResponse, AppError> {
    // Validate input
    body.validate()
        .map_err(|e| AppError::ValidationError(e.to_string()))?;

    // Check if email already exists
    let existing_user = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM users WHERE email = $1")
        .bind(&body.email)
        .fetch_one(pool.get_ref())
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?;

    if existing_user > 0 {
        return Err(AppError::Conflict("Email already exists".to_string()));
    }

    // Hash password
    let password_hash = hash_password(&body.password)?;

    // Insert user
    let user = sqlx::query_as::<_, User>(
        r#"
        INSERT INTO users (email, password_hash, full_name)
        VALUES ($1, $2, $3)
        RETURNING id, email, password_hash, full_name, created_at, updated_at
        "#,
    )
    .bind(&body.email)
    .bind(&password_hash)
    .bind(&body.full_name)
    .fetch_one(pool.get_ref())
    .await
    .map_err(|e| AppError::InternalError(e.to_string()))?;

    // Create tokens
    let access_token = create_access_token(&user, jwt_secret.get_ref())?;
    let refresh_token = create_refresh_token(pool.get_ref(), user.id).await?;

    Ok(HttpResponse::Created().json(AuthTokenResponse::new(access_token, refresh_token, &user)))
}

#[post("/auth/login")]
pub async fn login(
    pool: web::Data<PgPool>,
    jwt_secret: web::Data<String>,
    body: web::Json<LoginDto>,
) -> Result<HttpResponse, AppError> {
    // Find user by email
    let user = sqlx::query_as::<_, User>(
        "SELECT id, email, password_hash, full_name, created_at, updated_at FROM users WHERE email = $1",
    )
    .bind(&body.email)
    .fetch_optional(pool.get_ref())
    .await
    .map_err(|e| AppError::InternalError(e.to_string()))?
    .ok_or_else(|| AppError::Unauthorized("Invalid email or password".to_string()))?;

    // Verify password
    let is_valid = verify_password(&body.password, &user.password_hash)?;
    if !is_valid {
        return Err(AppError::Unauthorized(
            "Invalid email or password".to_string(),
        ));
    }

    // Create tokens
    let access_token = create_access_token(&user, jwt_secret.get_ref())?;
    let refresh_token = create_refresh_token(pool.get_ref(), user.id).await?;

    Ok(HttpResponse::Ok().json(AuthTokenResponse::new(access_token, refresh_token, &user)))
}

#[post("/auth/refresh")]
pub async fn refresh(
    pool: web::Data<PgPool>,
    jwt_secret: web::Data<String>,
    body: web::Json<RefreshTokenDto>,
) -> Result<HttpResponse, AppError> {
    // Validate the refresh token
    let token_record = validate_refresh_token(pool.get_ref(), &body.refresh_token).await?;

    // Get the user
    let user = sqlx::query_as::<_, User>(
        "SELECT id, email, password_hash, full_name, created_at, updated_at FROM users WHERE id = $1",
    )
    .bind(token_record.user_id)
    .fetch_optional(pool.get_ref())
    .await
    .map_err(|e| AppError::InternalError(e.to_string()))?
    .ok_or_else(|| AppError::Unauthorized("User not found".to_string()))?;

    // Revoke the old refresh token (token rotation for security)
    revoke_refresh_token(pool.get_ref(), token_record.id).await?;

    // Create new tokens
    let access_token = create_access_token(&user, jwt_secret.get_ref())?;
    let new_refresh_token = create_refresh_token(pool.get_ref(), user.id).await?;

    Ok(HttpResponse::Ok().json(AuthTokenResponse::new(
        access_token,
        new_refresh_token,
        &user,
    )))
}

#[post("/auth/logout")]
pub async fn logout(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    jwt_secret: web::Data<String>,
    body: Option<web::Json<RefreshTokenDto>>,
) -> Result<HttpResponse, AppError> {
    // Try to get user ID from access token
    let token = extract_token(&req)?;
    let claims = decode_token(&token, jwt_secret.get_ref())?;

    // If refresh token provided, revoke only that token
    // Otherwise, revoke all tokens for the user
    if let Some(refresh_body) = body {
        let token_record =
            validate_refresh_token(pool.get_ref(), &refresh_body.refresh_token).await;
        if let Ok(record) = token_record {
            // Verify the token belongs to this user
            if record.user_id == claims.sub {
                revoke_refresh_token(pool.get_ref(), record.id).await?;
            }
        }
        Ok(HttpResponse::Ok().json(serde_json::json!({
            "message": "Logged out successfully"
        })))
    } else {
        // Revoke all tokens for the user
        let count = revoke_all_user_tokens(pool.get_ref(), claims.sub).await?;
        Ok(HttpResponse::Ok().json(serde_json::json!({
            "message": "Logged out from all devices",
            "revoked_sessions": count
        })))
    }
}

#[get("/auth/me")]
pub async fn me(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    jwt_secret: web::Data<String>,
) -> Result<HttpResponse, AppError> {
    let token = extract_token(&req)?;
    let claims = decode_token(&token, jwt_secret.get_ref())?;

    let user = sqlx::query_as::<_, User>(
        "SELECT id, email, password_hash, full_name, created_at, updated_at FROM users WHERE id = $1",
    )
    .bind(claims.sub)
    .fetch_optional(pool.get_ref())
    .await
    .map_err(|e| AppError::InternalError(e.to_string()))?
    .ok_or_else(|| AppError::Unauthorized("User not found".to_string()))?;

    Ok(HttpResponse::Ok().json(UserResponseDto::from_user(&user)))
}

// ============================================================================
// Helper for extracting authenticated user (for use in other modules)
// ============================================================================

/// Extract user ID from request's JWT token
pub fn get_user_id_from_request(req: &HttpRequest, jwt_secret: &str) -> Result<Uuid, AppError> {
    let token = extract_token(req)?;
    let claims = decode_token(&token, jwt_secret)?;
    Ok(claims.sub)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_password_creates_valid_hash() {
        let password = "secure_password123";
        let hash = hash_password(password).expect("Should hash password");
        assert!(hash.starts_with("$argon2"), "Hash should be Argon2 format");
    }

    #[test]
    fn test_hash_password_different_salts() {
        let password = "same_password";
        let hash1 = hash_password(password).expect("Should hash password");
        let hash2 = hash_password(password).expect("Should hash password");
        assert_ne!(hash1, hash2, "Hashes should differ due to random salt");
    }

    #[test]
    fn test_verify_password_correct() {
        let password = "test_password";
        let hash = hash_password(password).expect("Should hash password");
        let is_valid = verify_password(password, &hash).expect("Should verify");
        assert!(is_valid, "Correct password should verify");
    }

    #[test]
    fn test_verify_password_incorrect() {
        let password = "correct_password";
        let wrong_password = "wrong_password";
        let hash = hash_password(password).expect("Should hash password");
        let is_valid = verify_password(wrong_password, &hash).expect("Should verify");
        assert!(!is_valid, "Wrong password should not verify");
    }

    #[test]
    fn test_verify_password_invalid_hash() {
        let result = verify_password("password", "invalid_hash");
        assert!(result.is_err(), "Invalid hash should return error");
    }

    #[test]
    fn test_generate_refresh_token_length() {
        let token = generate_refresh_token();
        assert_eq!(token.len(), 64, "Refresh token should be 64 hex characters");
    }

    #[test]
    fn test_generate_refresh_token_uniqueness() {
        let token1 = generate_refresh_token();
        let token2 = generate_refresh_token();
        assert_ne!(token1, token2, "Tokens should be unique");
    }

    #[test]
    fn test_hash_refresh_token_deterministic() {
        let token = "test_token_123";
        let hash1 = hash_refresh_token(token);
        let hash2 = hash_refresh_token(token);
        assert_eq!(hash1, hash2, "Same token should produce same hash");
    }

    #[test]
    fn test_hash_refresh_token_different_inputs() {
        let hash1 = hash_refresh_token("token1");
        let hash2 = hash_refresh_token("token2");
        assert_ne!(
            hash1, hash2,
            "Different tokens should produce different hashes"
        );
    }
}
