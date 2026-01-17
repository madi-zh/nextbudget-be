use actix_web::{test, web, App};
use secrecy::Secret;
use serde_json::Value;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use be_rust::auth::{login, register};

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

static JWT_SECRET: &str = "test_jwt_secret_for_integration_tests";

pub struct TestApp {
    pub pool: PgPool,
    pub test_id: String,
}

pub struct TestResponse {
    status: u16,
    body: bytes::Bytes,
}

impl TestResponse {
    pub fn status(&self) -> u16 {
        self.status
    }

    pub async fn json(&self) -> Value {
        serde_json::from_slice(&self.body).expect("Failed to parse JSON response")
    }
}

impl TestApp {
    pub async fn new() -> Self {
        let counter = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let test_id = format!("{timestamp}_{counter}");

        let database_url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://user:password@localhost:5432/budget_db".to_string());

        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(&database_url)
            .await
            .expect("Failed to connect to database for tests");

        TestApp { pool, test_id }
    }

    /// Generate a unique email for this test run
    pub fn unique_email(&self, prefix: &str) -> String {
        format!("{prefix}_{}_@test.com", self.test_id)
    }

    pub async fn get(&self, path: &str) -> TestResponse {
        let jwt_secret = Secret::new(JWT_SECRET.to_string());
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(self.pool.clone()))
                .app_data(web::Data::new(jwt_secret))
                .route("/health", web::get().to(health_handler))
                .service(register)
                .service(login),
        )
        .await;

        let req = test::TestRequest::get().uri(path).to_request();
        let resp = test::call_service(&app, req).await;

        let status = resp.status().as_u16();
        let body = test::read_body(resp).await;

        TestResponse { status, body }
    }

    pub async fn post(&self, path: &str, payload: &Value) -> TestResponse {
        let jwt_secret = Secret::new(JWT_SECRET.to_string());
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(self.pool.clone()))
                .app_data(web::Data::new(jwt_secret))
                .route("/health", web::get().to(health_handler))
                .service(register)
                .service(login),
        )
        .await;

        let req = test::TestRequest::post()
            .uri(path)
            .set_json(payload)
            .to_request();
        let resp = test::call_service(&app, req).await;

        let status = resp.status().as_u16();
        let body = test::read_body(resp).await;

        TestResponse { status, body }
    }
}

async fn health_handler() -> actix_web::HttpResponse {
    actix_web::HttpResponse::Ok().json(serde_json::json!({"status": "healthy"}))
}

impl Drop for TestApp {
    fn drop(&mut self) {
        // Cleanup happens automatically when pool is dropped
    }
}
