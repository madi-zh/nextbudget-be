use serde_json::{json, Value};

mod common;
use common::TestApp;

#[actix_rt::test]
async fn test_health_endpoint() {
    let app = TestApp::new().await;

    let response = app.get("/health").await;

    assert_eq!(response.status(), 200);
    let body: Value = response.json().await;
    assert_eq!(body["status"], "healthy");
}

#[actix_rt::test]
async fn test_register_success() {
    let app = TestApp::new().await;
    let email = app.unique_email("newuser");

    let payload = json!({
        "email": email,
        "password": "password123",
        "full_name": "New User"
    });

    let response = app.post("/auth/register", &payload).await;

    assert_eq!(response.status(), 201);
    let body: Value = response.json().await;
    assert!(body["token"].is_string());
    assert_eq!(body["user"]["email"], email);
    assert_eq!(body["user"]["full_name"], "New User");
}

#[actix_rt::test]
async fn test_register_duplicate_email() {
    let app = TestApp::new().await;
    let email = app.unique_email("duplicate");

    let payload = json!({
        "email": email,
        "password": "password123"
    });

    // First registration should succeed
    let response1 = app.post("/auth/register", &payload).await;
    assert_eq!(response1.status(), 201);

    // Second registration with same email should fail
    let response2 = app.post("/auth/register", &payload).await;
    assert_eq!(response2.status(), 409);
    let body: Value = response2.json().await;
    assert_eq!(body["error"], "CONFLICT");
}

#[actix_rt::test]
async fn test_register_invalid_email() {
    let app = TestApp::new().await;

    let payload = json!({
        "email": "not-an-email",
        "password": "password123"
    });

    let response = app.post("/auth/register", &payload).await;

    assert_eq!(response.status(), 400);
    let body: Value = response.json().await;
    assert_eq!(body["error"], "VALIDATION_ERROR");
}

#[actix_rt::test]
async fn test_register_short_password() {
    let app = TestApp::new().await;
    let email = app.unique_email("shortpass");

    let payload = json!({
        "email": email,
        "password": "short"
    });

    let response = app.post("/auth/register", &payload).await;

    assert_eq!(response.status(), 400);
    let body: Value = response.json().await;
    assert_eq!(body["error"], "VALIDATION_ERROR");
    assert!(body["message"].as_str().unwrap().contains("6 characters"));
}

#[actix_rt::test]
async fn test_login_success() {
    let app = TestApp::new().await;
    let email = app.unique_email("login");

    // First register a user
    let register_payload = json!({
        "email": email,
        "password": "password123",
        "full_name": "Login Test"
    });
    app.post("/auth/register", &register_payload).await;

    // Then login
    let login_payload = json!({
        "email": email,
        "password": "password123"
    });

    let response = app.post("/auth/login", &login_payload).await;

    assert_eq!(response.status(), 200);
    let body: Value = response.json().await;
    assert!(body["token"].is_string());
    assert_eq!(body["user"]["email"], email);
}

#[actix_rt::test]
async fn test_login_wrong_password() {
    let app = TestApp::new().await;
    let email = app.unique_email("wrongpass");

    // Register a user
    let register_payload = json!({
        "email": email,
        "password": "correct_password"
    });
    app.post("/auth/register", &register_payload).await;

    // Try to login with wrong password
    let login_payload = json!({
        "email": email,
        "password": "wrong_password"
    });

    let response = app.post("/auth/login", &login_payload).await;

    assert_eq!(response.status(), 401);
    let body: Value = response.json().await;
    assert_eq!(body["error"], "UNAUTHORIZED");
}

#[actix_rt::test]
async fn test_login_nonexistent_user() {
    let app = TestApp::new().await;
    let email = app.unique_email("nonexistent");

    let payload = json!({
        "email": email,
        "password": "password123"
    });

    let response = app.post("/auth/login", &payload).await;

    assert_eq!(response.status(), 401);
    let body: Value = response.json().await;
    assert_eq!(body["error"], "UNAUTHORIZED");
}

#[actix_rt::test]
async fn test_token_is_valid_jwt() {
    let app = TestApp::new().await;
    let email = app.unique_email("jwt");

    let payload = json!({
        "email": email,
        "password": "password123"
    });

    let response = app.post("/auth/register", &payload).await;
    let body: Value = response.json().await;
    let token = body["token"].as_str().unwrap();

    // JWT should have 3 parts
    let parts: Vec<&str> = token.split('.').collect();
    assert_eq!(parts.len(), 3, "Token should be valid JWT format");
}
