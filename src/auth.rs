use actix_web::{get, post, web, HttpRequest, HttpResponse};
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use chrono::Utc;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::Serialize;
use sqlx::PgPool;
use validator::Validate;

use crate::errors::AppError;
use crate::models::{CreateUserDto, LoginDto, TokenClaims, User, UserResponseDto};

#[derive(Serialize)]
pub struct AuthResponse {
    pub token: String,
    pub user: UserResponseDto,
}

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

pub fn create_token(user_id: uuid::Uuid, jwt_secret: &str) -> Result<String, AppError> {
    let now = Utc::now().timestamp() as usize;
    let expires_at = now + (24 * 60 * 60); // 24 hours

    let claims = TokenClaims {
        sub: user_id,
        iat: now,
        exp: expires_at,
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(jwt_secret.as_bytes()),
    )
    .map_err(|e| AppError::InternalError(format!("Failed to create token: {}", e)))
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

fn extract_token(req: &HttpRequest) -> Result<String, AppError> {
    req.headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .map(|t| t.to_string())
        .ok_or_else(|| {
            AppError::Unauthorized("Missing or invalid Authorization header".to_string())
        })
}

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

    // Create JWT
    let token = create_token(user.id, jwt_secret.get_ref())?;

    Ok(HttpResponse::Created().json(AuthResponse {
        token,
        user: UserResponseDto::from_user(user),
    }))
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

    // Create JWT
    let token = create_token(user.id, jwt_secret.get_ref())?;

    Ok(HttpResponse::Ok().json(AuthResponse {
        token,
        user: UserResponseDto::from_user(user),
    }))
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

    Ok(HttpResponse::Ok().json(UserResponseDto::from_user(user)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn test_hash_password_creates_valid_hash() {
        let password = "secure_password123";
        let hash = hash_password(password).expect("Should hash password");

        // Argon2 hashes start with $argon2
        assert!(hash.starts_with("$argon2"), "Hash should be Argon2 format");
    }

    #[test]
    fn test_hash_password_different_salts() {
        let password = "same_password";
        let hash1 = hash_password(password).expect("Should hash password");
        let hash2 = hash_password(password).expect("Should hash password");

        // Same password should produce different hashes (different salts)
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
    fn test_create_token_success() {
        let user_id = Uuid::new_v4();
        let jwt_secret = "test_secret_key_for_testing";

        let token = create_token(user_id, jwt_secret).expect("Should create token");

        // JWT tokens have 3 parts separated by dots
        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3, "JWT should have 3 parts");
    }

    #[test]
    fn test_decode_token_success() {
        let user_id = Uuid::new_v4();
        let jwt_secret = "test_secret_key_for_testing";

        let token = create_token(user_id, jwt_secret).expect("Should create token");
        let claims = decode_token(&token, jwt_secret).expect("Should decode token");

        assert_eq!(claims.sub, user_id, "User ID should match");
    }

    #[test]
    fn test_decode_token_wrong_secret() {
        let user_id = Uuid::new_v4();
        let jwt_secret = "correct_secret";
        let wrong_secret = "wrong_secret";

        let token = create_token(user_id, jwt_secret).expect("Should create token");
        let result = decode_token(&token, wrong_secret);

        assert!(result.is_err(), "Wrong secret should fail verification");
    }

    #[test]
    fn test_decode_token_invalid_token() {
        let jwt_secret = "test_secret";
        let result = decode_token("invalid.token.here", jwt_secret);

        assert!(result.is_err(), "Invalid token should fail");
    }

    #[test]
    fn test_token_contains_correct_claims() {
        let user_id = Uuid::new_v4();
        let jwt_secret = "test_secret_key";

        let token = create_token(user_id, jwt_secret).expect("Should create token");
        let claims = decode_token(&token, jwt_secret).expect("Should decode token");

        // Check expiration is ~24 hours from now
        let now = Utc::now().timestamp() as usize;
        let expected_exp = now + (24 * 60 * 60);

        assert!(
            claims.exp >= expected_exp - 5 && claims.exp <= expected_exp + 5,
            "Expiration should be ~24 hours from now"
        );
        assert!(
            claims.iat >= now - 5 && claims.iat <= now + 5,
            "Issued at should be close to now"
        );
    }
}
