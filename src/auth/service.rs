use secrecy::Secret;
use sqlx::PgPool;
use uuid::Uuid;

use crate::errors::AppError;

use super::jwt::{create_access_token, create_refresh_token};
use super::models::{AuthTokenResponse, CreateUserDto, User};
use super::password::{hash_password, verify_password};

/// Authentication service handling user registration and login logic
pub struct AuthService;

impl AuthService {
    /// Register a new user and return auth tokens
    pub async fn register(
        pool: &PgPool,
        jwt_secret: &Secret<String>,
        dto: &CreateUserDto,
    ) -> Result<AuthTokenResponse, AppError> {
        // Check if email already exists
        let existing_user =
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM users WHERE email = $1")
                .bind(&dto.email)
                .fetch_one(pool)
                .await
                .map_err(|e| AppError::InternalError(e.to_string()))?;

        if existing_user > 0 {
            return Err(AppError::Conflict("Email already exists".to_string()));
        }

        // Hash password
        let password_hash = hash_password(&dto.password)?;

        // Insert user
        let user = sqlx::query_as::<_, User>(
            r#"
            INSERT INTO users (email, password_hash, full_name)
            VALUES ($1, $2, $3)
            RETURNING id, email, password_hash, full_name, created_at, updated_at
            "#,
        )
        .bind(&dto.email)
        .bind(&password_hash)
        .bind(&dto.full_name)
        .fetch_one(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?;

        // Create tokens
        let access_token = create_access_token(&user, jwt_secret)?;
        let refresh_token = create_refresh_token(pool, user.id).await?;

        Ok(AuthTokenResponse::new(access_token, refresh_token, &user))
    }

    /// Authenticate a user by email and password, return auth tokens
    pub async fn login(
        pool: &PgPool,
        jwt_secret: &Secret<String>,
        email: &str,
        password: &str,
    ) -> Result<AuthTokenResponse, AppError> {
        // Find user by email
        let user = sqlx::query_as::<_, User>(
            "SELECT id, email, password_hash, full_name, created_at, updated_at FROM users WHERE email = $1",
        )
        .bind(email)
        .fetch_optional(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?
        .ok_or_else(|| AppError::Unauthorized("Invalid email or password".to_string()))?;

        // Verify password
        let is_valid = verify_password(password, &user.password_hash)?;
        if !is_valid {
            return Err(AppError::Unauthorized(
                "Invalid email or password".to_string(),
            ));
        }

        // Create tokens
        let access_token = create_access_token(&user, jwt_secret)?;
        let refresh_token = create_refresh_token(pool, user.id).await?;

        Ok(AuthTokenResponse::new(access_token, refresh_token, &user))
    }

    /// Get user by ID
    pub async fn get_user_by_id(pool: &PgPool, user_id: Uuid) -> Result<User, AppError> {
        sqlx::query_as::<_, User>(
            "SELECT id, email, password_hash, full_name, created_at, updated_at FROM users WHERE id = $1",
        )
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?
        .ok_or_else(|| AppError::Unauthorized("User not found".to_string()))
    }
}
