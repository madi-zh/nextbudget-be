mod handlers;
mod jwt;
mod models;
mod password;
mod service;

// Re-export handlers for use in main.rs
pub use handlers::{login, logout, me, refresh, register};

// Re-export for use in extractors
pub use jwt::decode_token;
