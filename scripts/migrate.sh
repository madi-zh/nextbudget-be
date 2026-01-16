#!/bin/bash

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

cd "$PROJECT_ROOT"

# Load .env if it exists
if [ -f .env ]; then
    export $(grep -v '^#' .env | xargs)
fi

# Check DATABASE_URL
if [ -z "$DATABASE_URL" ]; then
    echo "Error: DATABASE_URL is not set"
    echo "Set it in .env or export it: export DATABASE_URL=postgres://user:pass@localhost:5432/budget_db"
    exit 1
fi

echo "Running migrations..."
sqlx migrate run

echo "Migrations completed successfully"
