use rand::Rng;
use secrecy::Secret;
use sqlx::PgPool;
use uuid::Uuid;

use crate::errors::AppError;

use super::jwt::{create_access_token, create_refresh_token};
use super::models::{AuthTokenResponse, CreateUserDto, GoogleTokenInfo, User};
use super::password::{hash_password, verify_password};

/// Google token verification endpoint
const GOOGLE_TOKEN_INFO_URL: &str = "https://oauth2.googleapis.com/tokeninfo";

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

    /// Authenticate with Google OAuth ID token
    pub async fn login_with_google(
        pool: &PgPool,
        jwt_secret: &Secret<String>,
        id_token: &str,
    ) -> Result<AuthTokenResponse, AppError> {
        // Verify the Google ID token
        let google_user = Self::verify_google_token(id_token).await?;

        // Check if email is verified
        if google_user.email_verified != "true" {
            return Err(AppError::Unauthorized(
                "Google account email is not verified".to_string(),
            ));
        }

        // Find or create user by email
        let user = Self::find_or_create_google_user(pool, &google_user).await?;

        // Create tokens
        let access_token = create_access_token(&user, jwt_secret)?;
        let refresh_token = create_refresh_token(pool, user.id).await?;

        Ok(AuthTokenResponse::new(access_token, refresh_token, &user))
    }

    /// Verify Google ID token with Google's tokeninfo endpoint
    async fn verify_google_token(id_token: &str) -> Result<GoogleTokenInfo, AppError> {
        let client = reqwest::Client::new();
        let url = format!("{}?id_token={}", GOOGLE_TOKEN_INFO_URL, id_token);

        let response = client
            .get(&url)
            .send()
            .await
            .map_err(|e| AppError::InternalError(format!("Failed to verify Google token: {}", e)))?;

        if !response.status().is_success() {
            return Err(AppError::Unauthorized(
                "Invalid Google ID token".to_string(),
            ));
        }

        response
            .json::<GoogleTokenInfo>()
            .await
            .map_err(|e| AppError::InternalError(format!("Failed to parse Google response: {}", e)))
    }

    /// Find existing user by email or create a new one for Google OAuth
    async fn find_or_create_google_user(
        pool: &PgPool,
        google_user: &GoogleTokenInfo,
    ) -> Result<User, AppError> {
        // Try to find existing user by email
        let existing_user = sqlx::query_as::<_, User>(
            "SELECT id, email, password_hash, full_name, created_at, updated_at FROM users WHERE email = $1",
        )
        .bind(&google_user.email)
        .fetch_optional(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?;

        if let Some(user) = existing_user {
            return Ok(user);
        }

        // Create new user with a random password hash (they'll use Google to login)
        let random_password: String = rand::thread_rng()
            .sample_iter(&rand::distributions::Alphanumeric)
            .take(32)
            .map(char::from)
            .collect();
        let password_hash = hash_password(&random_password)?;

        let user = sqlx::query_as::<_, User>(
            r#"
            INSERT INTO users (email, password_hash, full_name)
            VALUES ($1, $2, $3)
            RETURNING id, email, password_hash, full_name, created_at, updated_at
            "#,
        )
        .bind(&google_user.email)
        .bind(&password_hash)
        .bind(&google_user.name)
        .fetch_one(pool)
        .await
        .map_err(|e| AppError::InternalError(e.to_string()))?;

        Ok(user)
    }
}
