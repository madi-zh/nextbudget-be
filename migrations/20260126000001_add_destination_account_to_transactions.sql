-- Add destination_account_id for transfer transactions
-- This enables proper two-account transfer tracking

-- Add the column (optional, only used for transfers)
ALTER TABLE transactions
ADD COLUMN destination_account_id UUID REFERENCES accounts(id) ON DELETE SET NULL;

-- Add constraint: destination_account_id is only allowed for transfer type
-- For non-transfers, it must be NULL
ALTER TABLE transactions
ADD CONSTRAINT chk_transfer_destination
CHECK (
    (transaction_type != 'transfer' AND destination_account_id IS NULL) OR
    (transaction_type = 'transfer')
);

-- Create index for querying transfers involving a destination account
CREATE INDEX idx_transactions_destination_account ON transactions(destination_account_id)
    WHERE destination_account_id IS NOT NULL;
