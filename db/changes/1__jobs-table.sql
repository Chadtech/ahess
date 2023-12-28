CREATE TABLE IF NOT EXISTS job (
    uuid UUID PRIMARY KEY NOT NULL DEFAULT uuid_generate_v7(),
    name TEXT NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    finished_at TIMESTAMP,
    error TEXT,
);