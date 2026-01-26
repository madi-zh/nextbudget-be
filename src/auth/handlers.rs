use actix_web::{get, post, web, HttpRequest, HttpResponse};
use secrecy::Secret;
use sqlx::PgPool;
use validator::Validate;

use crate::errors::{AppError, ErrorResponse};

use super::jwt::{
    create_access_token, decode_token, extract_token, revoke_all_user_tokens, revoke_refresh_token,
    rotate_refresh_token, validate_refresh_token,
};
use super::models::{
    AuthTokenResponse, CreateUserDto, GoogleLoginDto, LoginDto, RefreshTokenDto, UserResponseDto,
};
use super::service::AuthService;

/// POST /auth/register - Register a new user
#[utoipa::path(
    post,
    path = "/auth/register",
    tag = "Auth",
    request_body = CreateUserDto,
    responses(
        (status = 201, description = "User registered successfully", body = AuthTokenResponse),
        (status = 400, description = "Validation error", body = ErrorResponse),
        (status = 409, description = "Email already exists", body = ErrorResponse)
    )
)]
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
#[utoipa::path(
    post,
    path = "/auth/login",
    tag = "Auth",
    request_body = LoginDto,
    responses(
        (status = 200, description = "Login successful", body = AuthTokenResponse),
        (status = 401, description = "Invalid credentials", body = ErrorResponse)
    )
)]
#[post("/auth/login")]
pub async fn login(
    pool: web::Data<PgPool>,
    jwt_secret: web::Data<Secret<String>>,
    body: web::Json<LoginDto>,
) -> Result<HttpResponse, AppError> {
    let response = AuthService::login(
        pool.get_ref(),
        jwt_secret.get_ref(),
        &body.email,
        &body.password,
    )
    .await?;

    Ok(HttpResponse::Ok().json(response))
}

/// POST /auth/google - Authenticate with Google OAuth
#[utoipa::path(
    post,
    path = "/auth/google",
    tag = "Auth",
    request_body = GoogleLoginDto,
    responses(
        (status = 200, description = "Google login successful", body = AuthTokenResponse),
        (status = 401, description = "Invalid Google token", body = ErrorResponse)
    )
)]
#[post("/auth/google")]
pub async fn google_login(
    pool: web::Data<PgPool>,
    jwt_secret: web::Data<Secret<String>>,
    body: web::Json<GoogleLoginDto>,
) -> Result<HttpResponse, AppError> {
    let response =
        AuthService::login_with_google(pool.get_ref(), jwt_secret.get_ref(), &body.id_token)
            .await?;

    Ok(HttpResponse::Ok().json(response))
}

/// POST /auth/refresh - Refresh access token using refresh token
#[utoipa::path(
    post,
    path = "/auth/refresh",
    tag = "Auth",
    request_body = RefreshTokenDto,
    responses(
        (status = 200, description = "Token refreshed successfully", body = AuthTokenResponse),
        (status = 401, description = "Invalid or expired refresh token", body = ErrorResponse)
    )
)]
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
    let new_refresh_token = rotate_refresh_token(pool.get_ref(), token_record.id, user.id).await?;

    // Create new access token
    let access_token = create_access_token(&user, jwt_secret.get_ref())?;

    Ok(HttpResponse::Ok().json(AuthTokenResponse::new(
        access_token,
        new_refresh_token,
        &user,
    )))
}

/// POST /auth/logout - Revoke refresh tokens
#[utoipa::path(
    post,
    path = "/auth/logout",
    tag = "Auth",
    request_body(content = Option<RefreshTokenDto>, description = "Optional refresh token to revoke. If not provided, all sessions are revoked."),
    responses(
        (status = 200, description = "Logged out successfully"),
        (status = 401, description = "Invalid access token", body = ErrorResponse)
    ),
    security(
        ("bearer_auth" = [])
    )
)]
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
#[utoipa::path(
    get,
    path = "/auth/me",
    tag = "Auth",
    responses(
        (status = 200, description = "Current user info", body = UserResponseDto),
        (status = 401, description = "Invalid access token", body = ErrorResponse)
    ),
    security(
        ("bearer_auth" = [])
    )
)]
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
