-- Remote asset presence, download requests, transfer work, upload intent.
create table if not exists remote_asset(
    kind    TEXT not null check(kind in ('book', 'thumbnail')),
    hash    TEXT not null,
    primary key (kind, hash)
);

-- Durable download intent for content-addressed assets.
create table if not exists download_requests(
    target_kind     TEXT not null check(target_kind in ('book', 'thumbnail')),
    target_key      TEXT not null,
    origin          TEXT not null default 'background' check(origin in ('background', 'user_initiated')),
    created_at      INTEGER not null,
    primary key (target_kind, target_key)
);


CREATE TABLE asset_work(id INTEGER PRIMARY KEY AUTOINCREMENT, content_hash TEXT NOT NULL UNIQUE);

CREATE TABLE IF NOT EXISTS local_book_versions(
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        content_hash TEXT NOT NULL REFERENCES book(content_hash) ON DELETE CASCADE,
        checksum TEXT NOT NULL,
        size_bytes INTEGER NOT NULL CHECK(size_bytes>0),
        origin TEXT NOT NULL CHECK(origin IN ('local','remote')),
        UNIQUE(content_hash,checksum)
    );
    CREATE TABLE IF NOT EXISTS local_book_upload(
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        content_hash TEXT NOT NULL UNIQUE REFERENCES book(content_hash) ON DELETE CASCADE,
        checksum TEXT NOT NULL,
        size_bytes INTEGER NOT NULL CHECK(size_bytes>0)
    );
    CREATE TABLE IF NOT EXISTS local_publication_current(
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        dir_id TEXT NOT NULL,
        content_hash TEXT NOT NULL REFERENCES book(content_hash) ON DELETE CASCADE,
        checksum TEXT NOT NULL,
        origin TEXT NOT NULL CHECK(origin IN ('local','remote')),
        UNIQUE(dir_id,content_hash)
    );

CREATE TABLE IF NOT EXISTS rejected_asset_upload(
            kind TEXT NOT NULL CHECK(kind IN ('book','thumbnail')),
            hash TEXT NOT NULL,
            reason TEXT NOT NULL,
            rejected_at INTEGER NOT NULL,
            PRIMARY KEY(kind, hash)
        );

