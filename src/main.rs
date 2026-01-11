mod auth;
mod errors;
mod models;

use actix_web::{get, web, App, HttpResponse, HttpServer, Responder};
use dotenv::dotenv;
use sqlx::postgres::PgPoolOptions;
use std::env;

#[get("/health")]
async fn health_check() -> impl Responder {
    HttpResponse::Ok().json(serde_json::json!({"status": "healthy"}))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();
    env_logger::init();

    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let jwt_secret = env::var("JWT_SECRET").expect("JWT_SECRET must be set");

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("Failed to create pool");

    println!("Starting server at http://0.0.0.0:8080");

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .app_data(web::Data::new(jwt_secret.clone()))
            .service(health_check)
            // Auth endpoints
            .service(auth::register)
            .service(auth::login)
            .service(auth::refresh)
            .service(auth::logout)
            .service(auth::me)
    })
    .bind(("0.0.0.0", 8080))?
    .run()
    .await
}