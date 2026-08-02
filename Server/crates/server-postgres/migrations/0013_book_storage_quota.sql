-- Book storage is charged logically per user/hash while blob_object remains a
-- globally deduplicated catalogue of physical content-addressed objects.
CREATE TABLE IF NOT EXISTS blob_object (
    content_hash TEXT PRIMARY KEY CHECK (content_hash ~ '^[0-9a-f]{64}$'),
    size_bytes BIGINT NOT NULL CHECK (size_bytes >= 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS user_storage_account (
    user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    quota_bytes BIGINT NOT NULL DEFAULT 5368709120 CHECK (quota_bytes >= 0),
    used_bytes BIGINT NOT NULL DEFAULT 0 CHECK (used_bytes >= 0),
    reserved_bytes BIGINT NOT NULL DEFAULT 0 CHECK (reserved_bytes >= 0),
    CHECK (used_bytes + reserved_bytes <= quota_bytes)
);

CREATE TABLE IF NOT EXISTS user_blob_charge (
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    content_hash TEXT NOT NULL REFERENCES blob_object(content_hash) ON DELETE RESTRICT,
    charged_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, content_hash)
);

CREATE TABLE IF NOT EXISTS book_upload_reservation (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    library_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    content_hash TEXT NOT NULL CHECK (content_hash ~ '^[0-9a-f]{64}$'),
    size_bytes BIGINT NOT NULL CHECK (size_bytes >= 0),
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (user_id, content_hash)
);

CREATE INDEX IF NOT EXISTS idx_book_upload_reservation_expiry
    ON book_upload_reservation(expires_at);
