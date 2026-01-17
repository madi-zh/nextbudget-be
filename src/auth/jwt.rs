use actix_web::HttpRequest;
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use rand::Rng;
use secrecy::{ExposeSecret, Secret};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

use crate::errors::AppError;

use super::models::{RefreshToken, TokenClaims, User};

// Token expiration constants
pub const ACCESS_TOKEN_EXPIRY_MINUTES: i64 = 15;
pub const REFRESH_TOKEN_EXPIRY_DAYS: i64 = 7;

// ============================================================================
// JWT Access Token Utilities
// ============================================================================

/// Create a new JWT access token for a user
pub fn create_access_token(user: &User, jwt_secret: &Secret<String>) -> Result<String, AppError> {
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
        &EncodingKey::from_secret(jwt_secret.expose_secret().as_bytes()),
    )
    .map_err(|e| AppError::InternalError(format!("Failed to create access token: {e}")))
}

/// Decode and validate a JWT access token
pub fn decode_token(token: &str, jwt_secret: &Secret<String>) -> Result<TokenClaims, AppError> {
    decode::<TokenClaims>(
        token,
        &DecodingKey::from_secret(jwt_secret.expose_secret().as_bytes()),
        &Validation::default(),
    )
    .map(|data| data.claims)
    .map_err(|e| AppError::Unauthorized(format!("Invalid token: {e}")))
}

/// Extract Bearer token from Authorization header
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

/// Generate a random refresh token string (64 hex characters)
pub fn generate_refresh_token() -> String {
    let mut rng = rand::thread_rng();
    let bytes: [u8; 32] = rng.gen();
    hex::encode(bytes)
}

/// Hash a refresh token for secure storage
pub fn hash_refresh_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

/// Create and store a new refresh token in the database
pub async fn create_refresh_token(pool: &PgPool, user_id: Uuid) -> Result<String, AppError> {
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
    .map_err(|e| AppError::InternalError(format!("Failed to store refresh token: {e}")))?;

    Ok(raw_token)
}

/// Validate a refresh token and return the associated token record
pub async fn validate_refresh_token(
    pool: &PgPool,
    raw_token: &str,
) -> Result<RefreshToken, AppError> {
    let token_hash = hash_refresh_token(raw_token);

    sqlx::query_as::<_, RefreshToken>(
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
    .ok_or_else(|| AppError::Unauthorized("Invalid or expired refresh token".to_string()))
}

/// Revoke a specific refresh token
pub async fn revoke_refresh_token(pool: &PgPool, token_id: Uuid) -> Result<(), AppError> {
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
    .map_err(|e| AppError::InternalError(format!("Failed to revoke token: {e}")))?;

    Ok(())
}

/// Revoke all refresh tokens for a user (logout from all devices)
pub async fn revoke_all_user_tokens(pool: &PgPool, user_id: Uuid) -> Result<u64, AppError> {
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
    .map_err(|e| AppError::InternalError(format!("Failed to revoke tokens: {e}")))?;

    Ok(result.rows_affected())
}

/// Rotate refresh token atomically (revoke old, create new) within a transaction
pub async fn rotate_refresh_token(
    pool: &PgPool,
    old_token_id: Uuid,
    user_id: Uuid,
) -> Result<String, AppError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| AppError::InternalError(format!("Failed to begin transaction: {e}")))?;

    // Revoke old token
    sqlx::query(
        r#"
        UPDATE refresh_tokens
        SET revoked_at = NOW()
        WHERE id = $1
        "#,
    )
    .bind(old_token_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::InternalError(format!("Failed to revoke token: {e}")))?;

    // Create new token
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
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::InternalError(format!("Failed to store refresh token: {e}")))?;

    tx.commit()
        .await
        .map_err(|e| AppError::InternalError(format!("Failed to commit transaction: {e}")))?;

    Ok(raw_token)
}

#[cfg(test)]
mod tests {
    use super::*;

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
