# Authentication System Completion Plan

## Current State Analysis

### Existing Implementation
- **POST /auth/register** - Creates user with Argon2 password hashing, returns JWT
- **POST /auth/login** - Validates credentials, returns JWT
- **GET /auth/me** - Extracts bearer token, returns current user
- **Files**: `src/auth.rs`, `src/models.rs`, `src/errors.rs`, `src/main.rs`

### Current Token Configuration
- Access token expiration: 24 hours (needs to change to 15 minutes)
- JWT claims: `sub` (user_id), `iat`, `exp` (needs email and name)
- No refresh token mechanism

---

## Implementation Plan

### Phase 1: Database Migration for Refresh Tokens

**File**: `migrations/YYYYMMDDHHMMSS_create_refresh_tokens_table.sql`

```sql
-- Create refresh_tokens table
CREATE TABLE IF NOT EXISTS refresh_tokens (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash VARCHAR(64) NOT NULL,  -- SHA256 hash (64 hex chars)
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    revoked_at TIMESTAMPTZ,
    CONSTRAINT unique_active_token UNIQUE (token_hash)
);

-- Index for efficient lookups
CREATE INDEX idx_refresh_tokens_user_id ON refresh_tokens(user_id);
CREATE INDEX idx_refresh_tokens_token_hash ON refresh_tokens(token_hash);
CREATE INDEX idx_refresh_tokens_expires_at ON refresh_tokens(expires_at);
```

**Dependencies to add to Cargo.toml**:
```toml
sha2 = "0.10"    # For SHA256 hashing of refresh tokens
rand = "0.8"     # For generating secure random refresh tokens
hex = "0.4"      # For hex encoding the token hash
```

---

### Phase 2: Model Updates

**File**: `src/models.rs`

#### 2.1 Update Password Validation
```rust
#[derive(Debug, Deserialize, Validate)]
pub struct CreateUserDto {
    #[validate(email)]
    pub email: String,
    #[validate(length(min = 8, message = "Password must be at least 8 characters"))]
    pub password: String,
    pub full_name: Option<String>,
}
```

#### 2.2 Enhanced TokenClaims
```rust
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TokenClaims {
    pub sub: Uuid,           // user_id
    pub email: String,       // user email
    pub name: Option<String>, // user full_name
    pub iat: usize,          // issued at
    pub exp: usize,          // expires at
}
```

#### 2.3 New RefreshToken Model
```rust
#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct RefreshToken {
    pub id: Uuid,
    pub user_id: Uuid,
    pub token_hash: String,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}
```

#### 2.4 New DTOs
```rust
#[derive(Debug, Deserialize)]
pub struct RefreshTokenDto {
    pub refresh_token: String,
}

#[derive(Debug, Serialize)]
pub struct TokenPairResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64,  // seconds until access token expires
}

#[derive(Debug, Serialize)]
pub struct AuthTokenResponse {
    pub tokens: TokenPairResponse,
    pub user: UserResponseDto,
}
```

---

### Phase 3: Token Service Updates

**File**: `src/auth.rs`

#### 3.1 Constants
```rust
const ACCESS_TOKEN_EXPIRY_SECS: u64 = 15 * 60;        // 15 minutes
const REFRESH_TOKEN_EXPIRY_SECS: u64 = 7 * 24 * 60 * 60; // 7 days
```

#### 3.2 Updated Token Creation
```rust
pub fn create_access_token(
    user: &User,
    jwt_secret: &str
) -> Result<String, AppError> {
    let now = Utc::now().timestamp() as usize;
    let expires_at = now + ACCESS_TOKEN_EXPIRY_SECS as usize;

    let claims = TokenClaims {
        sub: user.id,
        email: user.email.clone(),
        name: user.full_name.clone(),
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
```

#### 3.3 Refresh Token Generation
```rust
use rand::Rng;
use sha2::{Sha256, Digest};

pub fn generate_refresh_token() -> (String, String) {
    // Generate 32 random bytes, encode as hex (64 chars)
    let token_bytes: [u8; 32] = rand::thread_rng().gen();
    let token = hex::encode(token_bytes);

    // Hash the token for storage
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    let token_hash = hex::encode(hasher.finalize());

    (token, token_hash)
}

pub fn hash_refresh_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}
```

#### 3.4 Database Operations for Refresh Tokens
```rust
pub async fn store_refresh_token(
    pool: &PgPool,
    user_id: Uuid,
    token_hash: &str,
) -> Result<RefreshToken, AppError> {
    let expires_at = Utc::now() + chrono::Duration::seconds(REFRESH_TOKEN_EXPIRY_SECS as i64);

    sqlx::query_as::<_, RefreshToken>(
        r#"
        INSERT INTO refresh_tokens (user_id, token_hash, expires_at)
        VALUES ($1, $2, $3)
        RETURNING id, user_id, token_hash, expires_at, created_at, revoked_at
        "#,
    )
    .bind(user_id)
    .bind(token_hash)
    .bind(expires_at)
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::InternalError(e.to_string()))
}

pub async fn validate_refresh_token(
    pool: &PgPool,
    token_hash: &str,
) -> Result<RefreshToken, AppError> {
    sqlx::query_as::<_, RefreshToken>(
        r#"
        SELECT id, user_id, token_hash, expires_at, created_at, revoked_at
        FROM refresh_tokens
        WHERE token_hash = $1
          AND revoked_at IS NULL
          AND expires_at > NOW()
        "#,
    )
    .bind(token_hash)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::InternalError(e.to_string()))?
    .ok_or_else(|| AppError::Unauthorized("Invalid or expired refresh token".to_string()))
}

pub async fn revoke_refresh_token(
    pool: &PgPool,
    token_hash: &str,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE refresh_tokens SET revoked_at = NOW() WHERE token_hash = $1"
    )
    .bind(token_hash)
    .execute(pool)
    .await
    .map_err(|e| AppError::InternalError(e.to_string()))?;

    Ok(())
}

pub async fn revoke_all_user_tokens(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE refresh_tokens SET revoked_at = NOW() WHERE user_id = $1 AND revoked_at IS NULL"
    )
    .bind(user_id)
    .execute(pool)
    .await
    .map_err(|e| AppError::InternalError(e.to_string()))?;

    Ok(())
}
```

---

### Phase 4: New Endpoint Handlers

**File**: `src/auth.rs`

#### 4.1 POST /auth/refresh
```rust
#[derive(Serialize)]
pub struct RefreshResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64,
}

#[post("/auth/refresh")]
pub async fn refresh(
    pool: web::Data<PgPool>,
    jwt_secret: web::Data<String>,
    body: web::Json<RefreshTokenDto>,
) -> Result<HttpResponse, AppError> {
    // Hash the incoming refresh token
    let token_hash = hash_refresh_token(&body.refresh_token);

    // Validate the refresh token exists and is not revoked/expired
    let stored_token = validate_refresh_token(pool.get_ref(), &token_hash).await?;

    // Fetch the user
    let user = sqlx::query_as::<_, User>(
        "SELECT id, email, password_hash, full_name, created_at, updated_at
         FROM users WHERE id = $1",
    )
    .bind(stored_token.user_id)
    .fetch_optional(pool.get_ref())
    .await
    .map_err(|e| AppError::InternalError(e.to_string()))?
    .ok_or_else(|| AppError::Unauthorized("User not found".to_string()))?;

    // Revoke the old refresh token (rotation)
    revoke_refresh_token(pool.get_ref(), &token_hash).await?;

    // Generate new token pair
    let access_token = create_access_token(&user, jwt_secret.get_ref())?;
    let (new_refresh_token, new_token_hash) = generate_refresh_token();
    store_refresh_token(pool.get_ref(), user.id, &new_token_hash).await?;

    Ok(HttpResponse::Ok().json(RefreshResponse {
        access_token,
        refresh_token: new_refresh_token,
        expires_in: ACCESS_TOKEN_EXPIRY_SECS,
    }))
}
```

#### 4.2 POST /auth/logout
```rust
#[post("/auth/logout")]
pub async fn logout(
    req: HttpRequest,
    pool: web::Data<PgPool>,
    jwt_secret: web::Data<String>,
    body: web::Json<RefreshTokenDto>,
) -> Result<HttpResponse, AppError> {
    // Optionally verify the access token to ensure the user is authenticated
    let token = extract_token(&req)?;
    let claims = decode_token(&token, jwt_secret.get_ref())?;

    // Revoke the refresh token
    let token_hash = hash_refresh_token(&body.refresh_token);

    // Verify the refresh token belongs to this user before revoking
    let stored_token = validate_refresh_token(pool.get_ref(), &token_hash).await?;
    if stored_token.user_id != claims.sub {
        return Err(AppError::Unauthorized("Token does not belong to user".to_string()));
    }

    revoke_refresh_token(pool.get_ref(), &token_hash).await?;

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "message": "Successfully logged out"
    })))
}
```

#### 4.3 Update register and login handlers
Both handlers need to return a token pair instead of just access token:

```rust
#[derive(Serialize)]
pub struct AuthResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64,
    pub user: UserResponseDto,
}

// In register and login handlers, after creating the user/validating credentials:
let access_token = create_access_token(&user, jwt_secret.get_ref())?;
let (refresh_token, token_hash) = generate_refresh_token();
store_refresh_token(pool.get_ref(), user.id, &token_hash).await?;

Ok(HttpResponse::Ok().json(AuthResponse {
    access_token,
    refresh_token,
    expires_in: ACCESS_TOKEN_EXPIRY_SECS,
    user: UserResponseDto::from_user(user),
}))
```

---

### Phase 5: Auth Middleware

**New File**: `src/middleware/auth.rs`

```rust
use actix_web::{
    dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform},
    error::ErrorUnauthorized,
    http::header::AUTHORIZATION,
    Error, HttpMessage,
};
use futures::future::{ok, LocalBoxFuture, Ready};
use std::rc::Rc;

use crate::auth::decode_token;
use crate::models::TokenClaims;

/// Extractor for authenticated user claims
/// Usage in handlers: `claims: AuthenticatedUser`
#[derive(Debug, Clone)]
pub struct AuthenticatedUser(pub TokenClaims);

impl std::ops::Deref for AuthenticatedUser {
    type Target = TokenClaims;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl actix_web::FromRequest for AuthenticatedUser {
    type Error = Error;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(
        req: &actix_web::HttpRequest,
        _payload: &mut actix_web::dev::Payload,
    ) -> Self::Future {
        match req.extensions().get::<TokenClaims>() {
            Some(claims) => ok(AuthenticatedUser(claims.clone())),
            None => futures::future::err(ErrorUnauthorized("Not authenticated")),
        }
    }
}

/// Middleware factory for JWT authentication
pub struct JwtAuth {
    jwt_secret: String,
}

impl JwtAuth {
    pub fn new(jwt_secret: String) -> Self {
        Self { jwt_secret }
    }
}

impl<S, B> Transform<S, ServiceRequest> for JwtAuth
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Transform = JwtAuthMiddleware<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ok(JwtAuthMiddleware {
            service: Rc::new(service),
            jwt_secret: self.jwt_secret.clone(),
        })
    }
}

pub struct JwtAuthMiddleware<S> {
    service: Rc<S>,
    jwt_secret: String,
}

impl<S, B> Service<ServiceRequest> for JwtAuthMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        // Extract token from Authorization header
        let auth_header = req
            .headers()
            .get(AUTHORIZATION)
            .and_then(|h| h.to_str().ok())
            .and_then(|h| h.strip_prefix("Bearer "));

        let token = match auth_header {
            Some(t) => t.to_string(),
            None => {
                return Box::pin(async { Err(ErrorUnauthorized("Missing authorization header")) });
            }
        };

        // Validate token
        let claims = match decode_token(&token, &self.jwt_secret) {
            Ok(c) => c,
            Err(_) => {
                return Box::pin(async { Err(ErrorUnauthorized("Invalid or expired token")) });
            }
        };

        // Insert claims into request extensions
        req.extensions_mut().insert(claims);

        let service = Rc::clone(&self.service);
        Box::pin(async move { service.call(req).await })
    }
}
```

**Update `src/main.rs`** to add the middleware module:
```rust
mod auth;
mod errors;
mod middleware;  // New module
mod models;
```

**Create `src/middleware/mod.rs`**:
```rust
mod auth;
pub use auth::{AuthenticatedUser, JwtAuth};
```

---

### Phase 6: Update main.rs Route Configuration

```rust
use middleware::JwtAuth;

HttpServer::new(move || {
    App::new()
        .app_data(web::Data::new(pool.clone()))
        .app_data(web::Data::new(jwt_secret.clone()))
        // Public routes
        .service(health_check)
        .service(auth::register)
        .service(auth::login)
        .service(auth::refresh)
        // Protected routes with middleware
        .service(
            web::scope("/api")
                .wrap(JwtAuth::new(jwt_secret.clone()))
                .service(auth::me)
                .service(auth::logout)
        )
})
```

Note: The `/auth/me` and `/auth/logout` routes should move under the protected scope, updating their paths to `/api/auth/me` and `/api/auth/logout`, OR keep them at `/auth/*` and apply middleware individually.

Alternative approach (keeping current paths):
```rust
// Using a guard-based approach instead of middleware for selective protection
```

---

### Phase 7: Error Handling Updates

**File**: `src/errors.rs`

Add new error variant for token-related errors:
```rust
#[derive(Debug)]
pub enum AppError {
    ValidationError(String),
    Unauthorized(String),
    Conflict(String),
    InternalError(String),
    TokenExpired,           // New
    InvalidToken(String),   // New
}
```

Update the `ResponseError` implementation:
```rust
AppError::TokenExpired => (
    actix_web::http::StatusCode::UNAUTHORIZED,
    "TOKEN_EXPIRED",
    "Access token has expired".to_string(),
),
AppError::InvalidToken(msg) => (
    actix_web::http::StatusCode::UNAUTHORIZED,
    "INVALID_TOKEN",
    msg.clone(),
),
```

---

## File Organization Summary

```
src/
├── main.rs           # Server setup, route registration
├── auth.rs           # Auth handlers and token utilities
├── models.rs         # Domain models and DTOs
├── errors.rs         # AppError enum and ResponseError impl
└── middleware/
    ├── mod.rs        # Module exports
    └── auth.rs       # JwtAuth middleware and AuthenticatedUser extractor

migrations/
├── 20250101000000_create_users_table.sql
└── YYYYMMDDHHMMSS_create_refresh_tokens_table.sql
```

---

## Handler Function Signatures Summary

| Endpoint | Method | Handler Signature |
|----------|--------|-------------------|
| `/auth/register` | POST | `async fn register(pool, jwt_secret, body: Json<CreateUserDto>) -> Result<HttpResponse, AppError>` |
| `/auth/login` | POST | `async fn login(pool, jwt_secret, body: Json<LoginDto>) -> Result<HttpResponse, AppError>` |
| `/auth/refresh` | POST | `async fn refresh(pool, jwt_secret, body: Json<RefreshTokenDto>) -> Result<HttpResponse, AppError>` |
| `/auth/logout` | POST | `async fn logout(req, pool, jwt_secret, body: Json<RefreshTokenDto>) -> Result<HttpResponse, AppError>` |
| `/auth/me` | GET | `async fn me(claims: AuthenticatedUser, pool) -> Result<HttpResponse, AppError>` |

---

## Database Queries Summary

### Refresh Token Operations
1. **Insert refresh token**: Store new token with user_id, token_hash, expires_at
2. **Validate refresh token**: Find by token_hash where not revoked and not expired
3. **Revoke single token**: Update revoked_at for specific token_hash
4. **Revoke all user tokens**: Update revoked_at for all tokens belonging to user_id

### Cleanup (optional cron job / scheduled task)
```sql
-- Delete expired/revoked tokens older than 30 days
DELETE FROM refresh_tokens
WHERE (expires_at < NOW() - INTERVAL '30 days')
   OR (revoked_at IS NOT NULL AND revoked_at < NOW() - INTERVAL '30 days');
```

---

## Test Cases to Add

### Unit Tests (src/auth.rs)

1. **Token Creation Tests**
   - `test_create_access_token_includes_email_and_name`
   - `test_access_token_expires_in_15_minutes`
   - `test_refresh_token_generation_produces_valid_hex`
   - `test_hash_refresh_token_consistent`

2. **Token Validation Tests**
   - `test_decode_expired_access_token_fails`
   - `test_decode_token_with_email_claim`

### Integration Tests (tests/auth_tests.rs)

1. **Register Flow**
   - `test_register_returns_both_tokens`
   - `test_register_password_min_8_chars`
   - `test_register_password_7_chars_rejected`

2. **Login Flow**
   - `test_login_returns_both_tokens`
   - `test_login_access_token_valid`

3. **Refresh Flow**
   - `test_refresh_valid_token_returns_new_pair`
   - `test_refresh_revokes_old_token`
   - `test_refresh_expired_token_rejected`
   - `test_refresh_revoked_token_rejected`
   - `test_refresh_invalid_token_rejected`

4. **Logout Flow**
   - `test_logout_revokes_refresh_token`
   - `test_logout_requires_valid_access_token`
   - `test_logout_only_revokes_own_token`
   - `test_refreshing_after_logout_fails`

5. **Protected Routes**
   - `test_me_without_token_returns_401`
   - `test_me_with_expired_token_returns_401`
   - `test_me_with_valid_token_returns_user`
   - `test_middleware_attaches_claims_to_request`

---

## Implementation Order

1. **Phase 1**: Create migration for refresh_tokens table
2. **Phase 2**: Update models.rs with new structs and DTOs
3. **Phase 3**: Update auth.rs with token utilities
4. **Phase 4**: Add new endpoints (refresh, logout)
5. **Phase 4b**: Update register/login to return token pairs
6. **Phase 5**: Create middleware module
7. **Phase 6**: Update main.rs with new routes and middleware
8. **Phase 7**: Update error types
9. **Phase 8**: Write and run tests
10. **Phase 9**: Update existing tests for new token format

---

## Dependencies Update (Cargo.toml)

```toml
[dependencies]
# ... existing dependencies ...
sha2 = "0.10"
rand = "0.8"
hex = "0.4"
futures = "0.3"  # For middleware futures
```

---

## Security Considerations

1. **Refresh Token Rotation**: Each refresh generates a new token pair and revokes the old refresh token
2. **Token Hash Storage**: Only SHA256 hashes are stored, never raw tokens
3. **Expiration Enforcement**: Both database query and JWT validation check expiration
4. **User Binding**: Logout verifies token belongs to authenticated user
5. **Cascade Delete**: User deletion cascades to refresh tokens (ON DELETE CASCADE)

---

## Migration Path for Existing Users

Since this is an enhancement, existing JWT tokens will become invalid when:
1. The claims structure changes (adding email/name)
2. The expiration changes from 24h to 15min

**Recommendation**: Deploy during low-traffic period, or implement a gradual rollout where the old token format is accepted for a transition period.
