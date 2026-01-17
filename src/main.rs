mod auth;
mod budget;
mod errors;
mod extractors;
mod models;

use actix_cors::Cors;
use actix_governor::{Governor, GovernorConfigBuilder};
use actix_web::{get, http::header, web, App, HttpResponse, HttpServer, Responder};
use dotenvy::dotenv;
use secrecy::Secret;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::env;
use std::time::Duration;
use tracing::info;
use tracing_actix_web::TracingLogger;

/// Health check endpoint that verifies database connectivity
#[get("/health")]
async fn health_check(pool: web::Data<PgPool>) -> impl Responder {
    match sqlx::query("SELECT 1").execute(pool.get_ref()).await {
        Ok(_) => HttpResponse::Ok().json(serde_json::json!({
            "status": "healthy",
            "database": "connected"
        })),
        Err(_) => HttpResponse::ServiceUnavailable().json(serde_json::json!({
            "status": "unhealthy",
            "database": "disconnected"
        })),
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();

    // Initialize tracing subscriber for structured logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let jwt_secret = env::var("JWT_SECRET").expect("JWT_SECRET must be set");

    // Wrap JWT secret in Secret for secure handling
    let jwt_secret = Secret::new(jwt_secret);

    // Get allowed origins from environment (comma-separated), default to localhost
    let allowed_origins = env::var("CORS_ALLOWED_ORIGINS")
        .unwrap_or_else(|_| "http://localhost:3000".to_string());

    // Configure connection pool with production-ready settings
    let pool = PgPoolOptions::new()
        .max_connections(20)
        .min_connections(5)
        .acquire_timeout(Duration::from_secs(3))
        .idle_timeout(Duration::from_secs(600))
        .max_lifetime(Duration::from_secs(1800))
        .connect(&database_url)
        .await
        .expect("Failed to create pool");

    info!("Starting server at http://0.0.0.0:8080");

    // Configure rate limiting for auth endpoints
    // ~5 requests per minute with burst of 5
    let auth_governor_config = GovernorConfigBuilder::default()
        .seconds_per_request(12)
        .burst_size(5)
        .finish()
        .expect("Failed to create rate limiter config");

    HttpServer::new(move || {
        // Clone allowed_origins for this closure invocation
        let allowed_origins = allowed_origins.clone();

        // Configure CORS
        let cors = Cors::default()
            .allowed_origin_fn(move |origin, _req_head| {
                let origin_str = origin.to_str().unwrap_or("");
                allowed_origins
                    .split(',')
                    .any(|allowed| allowed.trim() == origin_str)
            })
            .allowed_methods(vec!["GET", "POST", "PATCH", "DELETE", "OPTIONS"])
            .allowed_headers(vec![header::AUTHORIZATION, header::CONTENT_TYPE])
            .max_age(3600);

        App::new()
            // Middleware (order matters: outer to inner)
            .wrap(TracingLogger::default())
            .wrap(cors)
            // Shared state
            .app_data(web::Data::new(pool.clone()))
            .app_data(web::Data::new(jwt_secret.clone()))
            // Health endpoint (no rate limiting)
            .service(health_check)
            // Auth endpoints with rate limiting
            .service(
                web::scope("")
                    .wrap(Governor::new(&auth_governor_config))
                    .service(auth::register)
                    .service(auth::login)
                    .service(auth::refresh),
            )
            // Auth endpoints without rate limiting
            .service(auth::logout)
            .service(auth::me)
            // Budget endpoints
            .service(budget::list_budgets)
            .service(budget::get_budget_by_month_year)
            .service(budget::get_budget)
            .service(budget::create_budget)
            .service(budget::update_income)
            .service(budget::update_savings_rate)
            .service(budget::update_budget)
            .service(budget::delete_budget)
    })
    .bind(("0.0.0.0", 8080))?
    .run()
    .await
}
