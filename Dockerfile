# Build stage
FROM rust:1.85-slim AS builder
WORKDIR /app

# Install build dependencies (gcc/make needed by ring crate used in rustls)
RUN apt-get update && apt-get install -y pkg-config gcc make perl && rm -rf /var/lib/apt/lists/*

# Copy manifests first for dependency caching
COPY Cargo.toml Cargo.lock ./

# Create dummy src to build dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release 2>&1 || true
RUN rm -rf src

# Copy actual source code
COPY src src
COPY migrations migrations

# Build release binary
RUN touch src/main.rs && cargo build --release

# Runtime stage
FROM debian:bookworm-slim
COPY --from=builder /app/target/release/be-rust /usr/local/bin/
COPY --from=builder /app/migrations /app/migrations
EXPOSE 8080
CMD ["be-rust"]
