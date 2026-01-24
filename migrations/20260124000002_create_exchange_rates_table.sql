CREATE TABLE IF NOT EXISTS exchange_rates (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    base_currency CHAR(3) NOT NULL REFERENCES currencies(code),
    target_currency CHAR(3) NOT NULL REFERENCES currencies(code),
    rate NUMERIC(18,8) NOT NULL,
    effective_date DATE NOT NULL,
    source VARCHAR(50) NOT NULL DEFAULT 'openexchangerates',
    fetched_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT chk_exchange_rates_positive CHECK (rate > 0),
    CONSTRAINT chk_exchange_rates_different CHECK (base_currency != target_currency),
    CONSTRAINT uq_exchange_rates_pair_date UNIQUE (base_currency, target_currency, effective_date)
);

CREATE INDEX idx_exchange_rates_date ON exchange_rates(effective_date DESC);
CREATE INDEX idx_exchange_rates_pair ON exchange_rates(base_currency, target_currency, effective_date DESC);
