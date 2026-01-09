# be-rust

Rust backend for BudgetFlow app. This is a collaborative project with me, learning Rust with Claude Code just to get the hang of it.

## Stack

- Actix-web
- PostgreSQL + SQLx
- JWT auth with Argon2 password hashing

## Setup

1. Copy `.env.example` to `.env` and fill in your database URL
2. Run migrations: `sqlx migrate run`
3. Start server: `cargo run`

Server runs on `http://localhost:8080`

## Endpoints

- `GET /health` - health check
- `POST /auth/register` - create account
- `POST /auth/login` - get JWT token

## Dev

```bash
cargo watch -x run    # hot reload
cargo test            # run tests
cargo clippy          # lint
```

## Docker

```bash
docker compose up
```
