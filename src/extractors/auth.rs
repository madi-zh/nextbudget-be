use actix_web::{dev::Payload, web, FromRequest, HttpRequest};
use futures::future::{err, ok, Ready};
use secrecy::Secret;
use uuid::Uuid;

use crate::auth::decode_token;
use crate::errors::AppError;

/// Extractor that validates JWT and provides the authenticated user's ID.
pub struct AuthenticatedUser {
    pub user_id: Uuid,
}

impl FromRequest for AuthenticatedUser {
    type Error = AppError;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
        // Extract JWT secret from app data
        let jwt_secret = match req.app_data::<web::Data<Secret<String>>>() {
            Some(secret) => secret.get_ref().clone(),
            None => {
                return err(AppError::InternalError(
                    "JWT secret not configured".to_string(),
                ))
            }
        };

        // Extract token from Authorization header
        let token = match req
            .headers()
            .get("Authorization")
            .and_then(|h| h.to_str().ok())
            .and_then(|h| h.strip_prefix("Bearer "))
        {
            Some(t) => t.to_string(),
            None => {
                return err(AppError::Unauthorized(
                    "Missing or invalid Authorization header".to_string(),
                ))
            }
        };

        // Decode and validate token
        match decode_token(&token, &jwt_secret) {
            Ok(claims) => ok(AuthenticatedUser {
                user_id: claims.sub,
            }),
            Err(e) => err(e),
        }
    }
}
