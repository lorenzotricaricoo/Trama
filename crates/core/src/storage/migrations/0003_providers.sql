-- AI providers, their model catalogue and the model chosen for each task.
-- API keys are never stored here: `key_ref` names the Keychain item that holds the key.
-- See docs/architecture/data-model.md.

CREATE TABLE provider (
    id                 TEXT PRIMARY KEY NOT NULL,
    name               TEXT NOT NULL,
    base_url           TEXT NOT NULL,
    -- Keychain service of the API key; empty for providers that need no key.
    key_ref            TEXT,
    -- Last successful read of the provider's model catalogue.
    catalog_fetched_at INTEGER,
    created_at         INTEGER NOT NULL,
    updated_at         INTEGER NOT NULL
) STRICT;

CREATE TABLE provider_model (
    provider_id                TEXT NOT NULL REFERENCES provider (id) ON DELETE CASCADE,
    -- The model id as the provider names it.
    model_id                   TEXT NOT NULL,
    -- Listed by the provider at the last catalogue read (0: removed since, or added by hand).
    listed                     INTEGER NOT NULL CHECK (listed IN (0, 1)),
    -- Limits read from the catalogue, when the provider exposes them.
    context_tokens             INTEGER CHECK (context_tokens > 0),
    max_output_tokens          INTEGER CHECK (max_output_tokens > 0),
    -- Limits set by the user; they win over the catalogue.
    context_tokens_override    INTEGER CHECK (context_tokens_override > 0),
    max_output_tokens_override INTEGER CHECK (max_output_tokens_override > 0),
    updated_at                 INTEGER NOT NULL,
    PRIMARY KEY (provider_id, model_id)
) STRICT;

-- The task names are checked by the core, so the curator's tasks (phase 2) need no
-- migration.
CREATE TABLE task_model (
    task        TEXT PRIMARY KEY NOT NULL,
    provider_id TEXT NOT NULL,
    model_id    TEXT NOT NULL,
    updated_at  INTEGER NOT NULL,
    FOREIGN KEY (provider_id, model_id)
        REFERENCES provider_model (provider_id, model_id) ON DELETE CASCADE
) STRICT;

CREATE INDEX task_model_by_model ON task_model (provider_id, model_id);
