-- Model prices and token usage, to track what language models cost against a budget.
-- See docs/architecture/data-model.md (Provider model, Model usage).

-- Prices set by the user, per million tokens, in millionths of the currency unit
-- (3 dollars per million tokens is 3000000). NULL: unknown.
ALTER TABLE provider_model ADD COLUMN input_price_micros INTEGER CHECK (input_price_micros >= 0);
ALTER TABLE provider_model ADD COLUMN output_price_micros INTEGER CHECK (output_price_micros >= 0);

-- One row per reply a provider sent, with the tokens it reported. Kept when the
-- provider or the model is removed, so the month's spend stays right: no foreign keys.
CREATE TABLE model_usage (
    id            INTEGER PRIMARY KEY,
    at            INTEGER NOT NULL,
    provider_id   TEXT NOT NULL,
    model_id      TEXT NOT NULL,
    -- The task the call was made for; NULL for calls outside a task.
    task          TEXT,
    input_tokens  INTEGER NOT NULL CHECK (input_tokens >= 0),
    output_tokens INTEGER NOT NULL CHECK (output_tokens >= 0),
    -- Cost in millionths of the currency unit, from the model's prices at the time of the
    -- call; NULL when a price was unknown.
    cost_micros   INTEGER CHECK (cost_micros >= 0)
) STRICT;

CREATE INDEX model_usage_by_time ON model_usage (at);
