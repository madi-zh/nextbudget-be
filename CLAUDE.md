# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build and Development Commands

```bash
cargo build                    # Build the project
cargo run                      # Run the server (starts on http://0.0.0.0:8080)
cargo watch -x run             # Hot reload development (requires cargo-watch)
cargo test                     # Run all tests
cargo test test_name           # Run a specific test
cargo clippy                   # Lint the code
cargo fmt                      # Format the code
```

### Database

```bash
sqlx migrate run               # Run pending migrations
sqlx migrate add <name>        # Create a new migration
```

Requires `DATABASE_URL` environment variable (see `.env.example`).

### Docker Development

```bash
docker build -f Dockerfile.dev -t be-rust-dev .
```

## Architecture Overview

This is a Rust backend service for a budget tracking application using **Actix-web** as the HTTP framework and **SQLx** for PostgreSQL database access.

### Tech Stack

- **Actix-web 4** - HTTP server framework
- **SQLx** - Async PostgreSQL with compile-time query checking
- **Argon2** - Password hashing (configured, not yet implemented)
- **jsonwebtoken** - JWT authentication (configured, not yet implemented)
- **Validator** - Request validation with derive macros

### Project Structure

```
src/
├── main.rs     # Server setup, app state (PgPool), route registration
└── models.rs   # Domain models (User) and DTOs (CreateUserDto, LoginDto, TokenClaims)
migrations/     # SQLx migrations (timestamp-prefixed SQL files)
```

### Key Patterns

- **AppState** struct in `main.rs` holds the database pool and is shared via `web::Data`
- DTOs use `validator` derive macros for request validation
- `User` model has `#[serde(skip_serializing)]` on `password_hash` to prevent accidental exposure
- `UserResponseDto::from_user()` converts domain models to API responses

### Environment Variables

- `DATABASE_URL` - PostgreSQL connection string
- `RUST_LOG` - Logging level (e.g., `info`, `debug`)
- `JWT_SECRET` - Secret key for JWT signing
