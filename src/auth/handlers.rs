use actix_web::{get, post, web, HttpRequest, HttpResponse};
use secrecy::Secret;
use sqlx::PgPool;
use validator::Validate;

use crate::errors::AppError;

use super::jwt::{
    create_access_token, decode_token, extract_token, revoke_all_user_tokens,
    revoke_refresh_token, rotate_refresh_token, validate_refresh_token,
};
use super::models::{
    AuthTokenResponse, CreateUserDto, LoginDto, RefreshTokenDto, UserResponseDto,
};
use super::service::AuthService;

/// POST /auth/register - Register a new user
#[post("/auth/register")]
pub async fn register(
    pool: web::Data<PgPool>,
    jwt_secret: web::Data<Secret<String>>,
    body: web::Json<CreateUserDto>,
) -> Result<HttpResponse, AppError> {
    body.validate()
        .map_err(|e| AppError::ValidationError(e.to_string()))?;

    let response = AuthService::register(pool.get_ref(), jwt_secret.get_ref(), &body).await?;

    Ok(HttpResponse::Created().json(response))
}

/// POST /auth/login - Authenticate and get tokens
#[post("/auth/login")]
pub async fn login(
    pool: web::Data<PgPool>,
    jwt_secret: web::Data<Secret<String>>,
    body: web::Json<LoginDto>,
) -> Result<HttpResponse, AppError> {
    let response =
        AuthService::login(pool.get_ref(), jwt_secret.get_ref(), &body.email, &body.password)
            .await?;

    Ok(HttpResponse::Ok().json(response))
}

/// POST /auth/refresh - Refresh access token using refresh token
#[post("/auth/refresh")]
pub async fn refresh(
    pool: web::Data<PgPool>,
    jwt_secret: web::Data<Secret<String>>,
    body: web::Json<RefreshTokenDto>,
) -> Result<HttpResponse, AppError> {
    // Validate the refresh token
    let token_record = validate_refresh_token(pool.get_ref(), &body.refresh_token).await?;

    // Get the user
    let user = AuthService::get_user_by_id(pool.get_ref(), token_record.user_id).await?;

    // Rotate refresh token atomically (revoke old, create new)
    let new_refresh_token =
        rotate_refresh_token(pool.get_ref(), token_record.id, user.id).await?;

    // Create new access token
    let access_token = create_access_token(&user, jwt_secret.get_ref())?;

    Ok(HttpResponse::Ok().json(AuthTokenResponse::new(
        access_token,
        new_refresh_token,
        &user,
    )))
}

/// POST /auth/logout - Revoke refresh tokens
#[post("/auth/logout")]
pub async fn logout(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    jwt_secret: web::Data<Secret<String>>,
    body: Option<web::Json<RefreshTokenDto>>,
) -> Result<HttpResponse, AppError> {
    // Get user ID from access token
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

/// GET /auth/me - Get current user info
#[get("/auth/me")]
pub async fn me(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    jwt_secret: web::Data<Secret<String>>,
) -> Result<HttpResponse, AppError> {
    let token = extract_token(&req)?;
    let claims = decode_token(&token, jwt_secret.get_ref())?;

    let user = AuthService::get_user_by_id(pool.get_ref(), claims.sub).await?;

    Ok(HttpResponse::Ok().json(UserResponseDto::from_user(&user)))
}
