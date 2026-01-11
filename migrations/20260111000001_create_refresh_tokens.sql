-- Create refresh_tokens table for JWT refresh token management
-- Stores hashed tokens (SHA-256) for security

CREATE TABLE IF NOT EXISTS refresh_tokens (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash VARCHAR(64) NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    revoked_at TIMESTAMPTZ
);

-- Unique constraint on token_hash prevents duplicate tokens
CREATE UNIQUE INDEX idx_refresh_tokens_hash ON refresh_tokens(token_hash);

-- Index for finding active tokens by user (for logout-all-devices)
CREATE INDEX idx_refresh_tokens_user_active
    ON refresh_tokens(user_id)
    WHERE revoked_at IS NULL;

-- Index for cleanup job to find expired tokens
CREATE INDEX idx_refresh_tokens_expires
    ON refresh_tokens(expires_at)
    WHERE revoked_at IS NULL;
